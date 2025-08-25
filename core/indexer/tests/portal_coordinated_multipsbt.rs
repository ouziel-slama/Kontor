use anyhow::Result;
use bitcoin::Witness;
use bitcoin::amount::Amount;
use bitcoin::key::Secp256k1;
use bitcoin::script::Instruction;
use bitcoin::transaction::Version;
use bitcoin::{Network, OutPoint, Psbt, Sequence, Transaction, TxIn, TxOut, absolute::LockTime};
use bitcoin::{TapSighashType, consensus::encode::serialize as serialize_tx};
use clap::Parser;
use futures_util::future::join_all;
use indexer::api::compose::build_tap_script_and_script_address;
use indexer::config::TestConfig;
use indexer::multi_psbt_test_utils::{
    add_portal_input_and_output_to_psbt, add_single_node_input_and_output_to_psbt, estimate_single_input_single_output_reveal_vbytes, get_node_addresses, get_portal_info, log_node_commit_witness, log_node_sign_size_and_fee_breakdown, log_portal_commit, log_total_size_and_fee_breakdown, mock_fetch_portal_utxo, mock_fetch_utxos_for_addresses, tx_vbytes, PortalCommitLogCtx
};
use indexer::{bitcoin_client::Client, logging, test_utils};
use rand::Rng;
use tracing::info;

#[macro_export]
macro_rules! log_portal {
    ($fmt:expr, $($args:expr),+ ) => {
        tracing::info!(target: "portal", "\x1b[32m{}\x1b[0m", format!($fmt, $($args),+));
    };
    ($fmt:expr) => {
        tracing::info!(target: "portal", "\x1b[32m{}\x1b[0m", format!($fmt));
    };
}

#[macro_export]
macro_rules! log_node {
    ($fmt:expr, $($args:expr),+ ) => {
        tracing::info!(target: "node", "\x1b[38;5;208m{}\x1b[0m", format!($fmt, $($args),+));
    };
    ($fmt:expr) => {
        tracing::info!(target: "node", "\x1b[38;5;208m{}\x1b[0m", format!($fmt));
    };
}

#[tokio::test]
async fn test_portal_coordinated_commit_reveal_flow() -> Result<()> {
    // Setup
    logging::setup();
    let mut test_cfg = TestConfig::try_parse()?;
    test_cfg.network = Network::Testnet4;
    let client = Client::new_from_config(&test_cfg)?;
    let secp = Secp256k1::new();
    let dust_limit_sat: u64 = 330;

    // Generate a random sats/vbyte
    let min_sat_per_vb: u64 = rand::rng().random_range(1..11);
    info!("Random sats/vbyte: {}", min_sat_per_vb);

    // Phase 1: Nodes sign up for agreement with address + x-only pubkey
    let (signups, _) = get_node_addresses(&secp, &test_cfg)?;

    // Phase 2: Portal fetches node utxos and constructs COMMIT PSBT using nodes' outpoints/prevouts
    let node_utxos: Vec<(OutPoint, TxOut)> = mock_fetch_utxos_for_addresses(&signups);
    info!("portal fetching node utxos and constructing commit/reveal psbts");

    let mut commit_psbt = Psbt::from_unsigned_tx(Transaction {
        version: Version(2),
        lock_time: LockTime::ZERO,
        input: vec![],
        output: vec![],
    })?;

    // Portal appends each node's input and script output; calculate node change such that each pays their own commit and reveal deltas
    let mut node_input_indices: Vec<usize> = Vec::with_capacity(signups.len());
    let mut node_script_vouts: Vec<usize> = Vec::with_capacity(signups.len());
    let mut node_reveal_fees: Vec<u64> = Vec::with_capacity(signups.len());

    for (index, node_info) in signups.iter().enumerate() {
        let (node_reveal_fee, node_input_index, node_script_vout) =
            add_single_node_input_and_output_to_psbt(
                &mut commit_psbt,
                &node_utxos,
                index,
                min_sat_per_vb,
                node_info,
                dust_limit_sat,
            )?;
        node_input_indices.push(node_input_index);
        node_script_vouts.push(node_script_vout);
        node_reveal_fees.push(node_reveal_fee);
    }

    let (portal_info, portal_change_value, portal_input_index) =
        add_portal_input_and_output_to_psbt(
            &mut commit_psbt,
            min_sat_per_vb,
            dust_limit_sat,
            &secp,
            &test_cfg,
        )?;

    // Prepare prevouts for commit signing
    let all_prevouts_c: Vec<TxOut> = commit_psbt
        .inputs
        .iter()
        .map(|i| i.witness_utxo.clone().unwrap())
        .collect();
    info!("portal finalizing commit psbt");

    // Phase 3: Portal constructs REVEAL PSBT referencing fixed commit txid
    let commit_txid = commit_psbt.unsigned_tx.compute_txid();
    let mut reveal_tx = Transaction {
        version: Version(2),
        lock_time: LockTime::ZERO,
        input: vec![],
        output: vec![],
    };

    info!("portal constructing reveal psbt");
    // For each node, add script spend input and a send to node's address as output
    for (i, s) in signups.iter().enumerate() {
        let script_vout = node_script_vouts[i] as u32;
        reveal_tx.input.push(TxIn {
            previous_output: OutPoint {
                txid: commit_txid,
                vout: script_vout,
            },
            script_sig: bitcoin::script::ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::new(),
        });
        reveal_tx.output.push(TxOut {
            value: Amount::from_sat(dust_limit_sat),
            script_pubkey: s.address.script_pubkey(),
        });
    }
    let mut reveal_psbt = Psbt::from_unsigned_tx(reveal_tx.clone())?;
    for (i, s) in signups.iter().enumerate() {
        reveal_psbt.inputs[i].witness_utxo =
            Some(commit_psbt.unsigned_tx.output[node_script_vouts[i]].clone());
        reveal_psbt.inputs[i].tap_internal_key = Some(s.internal_key);
    }
    // Add portal reveal input/output
    let portal_script_vout = (commit_psbt.unsigned_tx.output.len() - 1) as u32
        - if portal_change_value > 0 { 1 } else { 0 };
    reveal_psbt.unsigned_tx.input.push(TxIn {
        previous_output: OutPoint {
            txid: commit_txid,
            vout: portal_script_vout,
        },
        script_sig: bitcoin::script::ScriptBuf::new(),
        sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
        witness: Witness::new(),
    });
    reveal_psbt.unsigned_tx.output.push(TxOut {
        value: Amount::from_sat(dust_limit_sat),
        script_pubkey: portal_info.address.script_pubkey(),
    });
    reveal_psbt.inputs.push(Default::default());
    let nodes_n = signups.len();
    reveal_psbt.inputs[nodes_n].witness_utxo =
        Some(commit_psbt.unsigned_tx.output[portal_script_vout as usize].clone());
    reveal_psbt.inputs[nodes_n].tap_internal_key = Some(portal_info.internal_key);

    let (_, node_secrets) = get_node_addresses(&secp, &test_cfg)?;

    // Phase 4: Portal sends both PSBTs to nodes; nodes sign commit input (key-spend, SIGHASH_ALL) and reveal input (script-spend, SIGHASH_ALL)
    // Each node signs asynchronously and returns only its own witnesses; portal merges them
    let commit_base_tx = commit_psbt.unsigned_tx.clone();
    let commit_base_psbt = commit_psbt.clone();
    let reveal_base_psbt = reveal_psbt.clone();
    let all_prevouts_clone = all_prevouts_c.clone();

    let node_sign_futs = signups.iter().enumerate().map(|(i, s)| {
        let secp = Secp256k1::new();
        let keypair = node_secrets[i].keypair;
        let mut commit_tx_local = commit_base_tx.clone();
        let mut commit_psbt_local = commit_base_psbt.clone();
        let mut reveal_psbt_local = reveal_base_psbt.clone();
        let prevouts_commit = all_prevouts_clone.clone();
        let node_input_indices_async = node_input_indices.clone();
        let input_index = node_input_indices_async[i];
        async move {
            // Commit: sign only this node's input
            test_utils::sign_key_spend(
                &secp,
                &mut commit_tx_local,
                &prevouts_commit,
                &keypair,
                input_index,
                Some(TapSighashType::Default),
            )?;
            let commit_witness = commit_tx_local.input[input_index].witness.clone();
            log_node_commit_witness(i, &commit_witness);
            // Attach to local commit psbt
            commit_psbt_local.inputs[input_index].final_script_witness = Some(commit_witness);

            // Reveal: sign only this node's reveal input and log sizes/fees
            let (tap_script, tap_info, _addr) =
                build_tap_script_and_script_address(s.internal_key, b"node-data".to_vec())?;
            let prevouts_reveal: Vec<TxOut> = reveal_psbt_local
                .inputs
                .iter()
                .map(|inp| inp.witness_utxo.clone().expect("wutxo"))
                .collect();
            let mut reveal_tx_local = reveal_psbt_local.unsigned_tx.clone();
            test_utils::sign_script_spend_with_sighash(
                &secp,
                &tap_info,
                &tap_script,
                &mut reveal_tx_local,
                &prevouts_reveal,
                &keypair,
                i,
                TapSighashType::Default,
            )?;

            log_node_sign_size_and_fee_breakdown(
                &reveal_psbt_local,
                i,
                &mut reveal_tx_local,
                min_sat_per_vb,
            );

            let reveal_witness = reveal_tx_local.input[i].witness.clone();
            // Attach to local reveal psbt
            reveal_psbt_local.inputs[i].final_script_witness = Some(reveal_witness);

            Ok::<(usize, Psbt, Psbt), anyhow::Error>((i, commit_psbt_local, reveal_psbt_local))
        }
    });

    let node_psbts: Vec<(usize, Psbt, Psbt)> = join_all(node_sign_futs)
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;

    // Merge node witnesses back into the original PSBTs
    for (i, c_psbt, r_psbt) in node_psbts {
        let idx = node_input_indices[i];
        commit_psbt.inputs[idx].final_script_witness =
            c_psbt.inputs[idx].final_script_witness.clone();
        reveal_psbt.inputs[i].final_script_witness = r_psbt.inputs[i].final_script_witness.clone();
        log_portal!(
            "merged node {} commit ({} elems), reveal ({} elems)",
            i,
            commit_psbt.inputs[idx]
                .final_script_witness
                .as_ref()
                .map(|w| w.len())
                .unwrap_or(0),
            reveal_psbt.inputs[i]
                .final_script_witness
                .as_ref()
                .map(|w| w.len())
                .unwrap_or(0)
        );
    }

    // Sign portal commit input
    {
        let mut tx_to_sign_portal = commit_psbt.unsigned_tx.clone();
        test_utils::sign_key_spend(
            &secp,
            &mut tx_to_sign_portal,
            &all_prevouts_c,
            &portal_info.keypair,
            portal_input_index,
            Some(TapSighashType::Default),
        )?;
        commit_psbt.inputs[portal_input_index].final_script_witness =
            Some(tx_to_sign_portal.input[portal_input_index].witness.clone());
        log_portal!(
            "portal added commit witness (stack_elems={})",
            tx_to_sign_portal.input[portal_input_index].witness.len()
        );
    }

    // Portal signs reveal input
    {
        let (tap_script, tap_info, _addr) =
            build_tap_script_and_script_address(portal_info.internal_key, b"portal-data".to_vec())?;
        let prevouts: Vec<TxOut> = reveal_psbt
            .inputs
            .iter()
            .map(|inp| inp.witness_utxo.clone().expect("wutxo"))
            .collect();
        let mut txp = reveal_psbt.unsigned_tx.clone();
        test_utils::sign_script_spend_with_sighash(
            &secp,
            &tap_info,
            &tap_script,
            &mut txp,
            &prevouts,
            &portal_info.keypair,
            nodes_n,
            TapSighashType::Default,
        )?;
        // Log portal reveal
        let mut reveal_before = reveal_psbt.unsigned_tx.clone();
        for j in 0..reveal_before.input.len() {
            if let Some(wit) = &reveal_psbt.inputs[j].final_script_witness {
                reveal_before.input[j].witness = wit.clone();
            }
        }
        let before_vb_r_portal = tx_vbytes(&reveal_before);
        let mut reveal_after = reveal_before.clone();
        reveal_after.input[nodes_n].witness = txp.input[nodes_n].witness.clone();
        let after_vb_r_portal = tx_vbytes(&reveal_after);
        let delta_vb_r_portal = after_vb_r_portal.saturating_sub(before_vb_r_portal);
        let in_val_r_portal = reveal_psbt.inputs[nodes_n]
            .witness_utxo
            .as_ref()
            .expect("wutxo")
            .value
            .to_sat();
        let out_val_r_portal = reveal_psbt.unsigned_tx.output[nodes_n].value.to_sat();
        let fee_paid_r_portal = in_val_r_portal.saturating_sub(out_val_r_portal);
        let needed_fee_portal = delta_vb_r_portal.saturating_mul(min_sat_per_vb);
        log_portal!(
            "portal reveal_vb_now={} vB; reveal_delta={} vB; reveal_fee_paid={} sat; reveal_fee_needed={} sat",
            after_vb_r_portal,
            delta_vb_r_portal,
            fee_paid_r_portal,
            needed_fee_portal
        );
        assert!(
            fee_paid_r_portal >= needed_fee_portal,
            "portal reveal fee insufficient: paid={} < needed={}",
            fee_paid_r_portal,
            needed_fee_portal
        );
        reveal_psbt.inputs[nodes_n].final_script_witness = Some(txp.input[nodes_n].witness.clone());
        log_portal!(
            "portal added reveal witness (stack_elems={})",
            txp.input[nodes_n].witness.len()
        );
    }

    // Phase 5: Verify the x-only pubkeys are revealed in reveal witnesses
    for (i, s) in signups.iter().enumerate() {
        let wit = reveal_psbt.inputs[i]
            .final_script_witness
            .as_ref()
            .expect("node reveal witness");
        assert!(wit.len() >= 2, "witness must contain signature and script");
        let script_bytes = wit.iter().nth(1).expect("script");
        let script = bitcoin::script::ScriptBuf::from_bytes(script_bytes.to_vec());
        let mut it = script.instructions();
        if let Some(Ok(Instruction::PushBytes(bytes))) = it.next() {
            assert_eq!(
                bytes.as_bytes(),
                &s.internal_key.serialize(),
                "node xonly pubkey not revealed correctly"
            );
        } else {
            panic!("node tapscript missing leading pubkey push");
        }
    }

    log_total_size_and_fee_breakdown(&commit_psbt, &reveal_psbt, min_sat_per_vb);

    let commit_tx = commit_psbt.extract_tx()?;
    let reveal_tx = reveal_psbt.extract_tx()?;

    // Phase 6: Broadcast commit then reveal together
    let commit_hex = hex::encode(serialize_tx(&commit_tx));
    let reveal_hex = hex::encode(serialize_tx(&reveal_tx));
    let res = client
        .test_mempool_accept(&[commit_hex, reveal_hex])
        .await?;
    assert_eq!(res.len(), 2, "Expected results for both transactions");
    assert!(
        res[0].allowed,
        "Commit rejected: {:?}",
        res[0].reject_reason
    );
    assert!(
        res[1].allowed,
        "Reveal rejected: {:?}",
        res[1].reject_reason
    );

    Ok(())
}

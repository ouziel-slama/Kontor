use anyhow::Result;
use bitcoin::address::Address;
use bitcoin::amount::Amount;
use bitcoin::consensus::encode::serialize as serialize_tx;
use bitcoin::key::{Keypair, Secp256k1};
use bitcoin::secp256k1::All;
use bitcoin::transaction::Version;
use bitcoin::{OutPoint, Sequence, Transaction, TxIn, TxOut, XOnlyPublicKey, absolute::LockTime};
use bitcoin::{Psbt, Txid, Witness};
use tracing::info;

use std::str::FromStr;

use crate::api::compose::build_tap_script_and_script_address;
use crate::config::TestConfig;
use crate::test_utils;

#[derive(Clone, Debug)]
pub struct NodeInfo {
    pub address: Address,
    pub internal_key: XOnlyPublicKey,
}

#[derive(Clone, Debug)]
pub struct NodeSecrets {
    pub keypair: Keypair,
}

#[derive(Clone, Debug)]
pub struct PortalInfo {
    pub address: Address,
    pub internal_key: XOnlyPublicKey,
    pub keypair: Keypair,
}

// NODE AND PORTAL SETUP HELPERS
pub fn get_node_addresses(
    secp: &Secp256k1<All>,
    test_cfg: &TestConfig,
) -> Result<(Vec<NodeInfo>, Vec<NodeSecrets>)> {
    let mut infos = Vec::new();
    let mut secrets = Vec::new();
    for i in 0..3 {
        let (address, child_key, _compressed) =
            test_utils::generate_taproot_address_from_mnemonic(secp, test_cfg, i as u32)?;
        let keypair = Keypair::from_secret_key(secp, &child_key.private_key);
        let (internal_key, _parity) = keypair.x_only_public_key();
        infos.push(NodeInfo {
            address,
            internal_key,
        });
        secrets.push(NodeSecrets { keypair });
    }
    Ok((infos, secrets))
}

pub fn get_portal_info(secp: &Secp256k1<All>, test_cfg: &TestConfig) -> Result<PortalInfo> {
    let (address, child_key, _compressed) =
        test_utils::generate_taproot_address_from_mnemonic(secp, test_cfg, 4)?;
    let keypair = Keypair::from_secret_key(secp, &child_key.private_key);
    let (internal_key, _parity) = keypair.x_only_public_key();
    Ok(PortalInfo {
        address,
        internal_key,
        keypair,
    })
}

pub fn mock_fetch_utxos_for_addresses(signups: &[NodeInfo]) -> Vec<(OutPoint, TxOut)> {
    signups
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let (txid_str, vout_u32, value_sat): (&str, u32, u64) = match i {
                0 => (
                    "dac8f123136bb59926e559e9da97eccc9f46726c3e7daaf2ab3502ef3a47fa46",
                    0,
                    500_000,
                ),
                1 => (
                    "465de2192b246635df14ff81c3b6f37fb864f308ad271d4f91a29dcf476640ba",
                    0,
                    500_000,
                ),
                2 => (
                    "49e327c2945f88908f67586de66af3bfc2567fe35ec7c5f1769f973f9fe8e47e",
                    0,
                    500_000,
                ),
                _ => unreachable!(),
            };
            (
                OutPoint {
                    txid: Txid::from_str(txid_str).unwrap(),
                    vout: vout_u32,
                },
                TxOut {
                    value: Amount::from_sat(value_sat),
                    script_pubkey: s.address.script_pubkey(),
                },
            )
        })
        .collect()
}

pub fn mock_fetch_portal_utxo(portal: &PortalInfo) -> (OutPoint, TxOut) {
    let (txid_str, vout_u32, value_sat): (&str, u32, u64) = (
        "09c741dd08af774cb5d1c26bfdc28eaa4ae42306a6a07d7be01de194979ff8df",
        0,
        500_000,
    );
    (
        OutPoint {
            txid: Txid::from_str(txid_str).unwrap(),
            vout: vout_u32,
        },
        TxOut {
            value: Amount::from_sat(value_sat),
            script_pubkey: portal.address.script_pubkey(),
        },
    )
}

// SIZE ESTIMATION HELPERS
pub fn tx_vbytes(tx: &Transaction) -> u64 {
    let mut no_wit = tx.clone();
    for inp in &mut no_wit.input {
        inp.witness = Witness::new();
    }
    let base_size = serialize_tx(&no_wit).len() as u64;
    let total_size = serialize_tx(tx).len() as u64;
    let witness_size = total_size.saturating_sub(base_size);
    let weight = base_size * 4 + witness_size;
    weight.div_ceil(4)
}

pub fn estimate_single_input_single_output_reveal_vbytes(
    tap_script: &bitcoin::script::ScriptBuf,
    tap_info: &bitcoin::taproot::TaprootSpendInfo,
    recipient_spk_len: usize,
    envelope_sat: u64,
) -> u64 {
    let mut dummy_reveal = Transaction {
        version: Version(2),
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint {
                txid: Txid::from_str(
                    "0000000000000000000000000000000000000000000000000000000000000000",
                )
                .unwrap(),
                vout: 0,
            },
            script_sig: bitcoin::script::ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(envelope_sat),
            script_pubkey: bitcoin::script::ScriptBuf::from_bytes(vec![0u8; recipient_spk_len]),
        }],
    };
    let mut w = Witness::new();
    w.push(vec![0u8; 65]);
    w.push(tap_script.clone());
    w.push(
        tap_info
            .control_block(&(tap_script.clone(), bitcoin::taproot::LeafVersion::TapScript))
            .expect("cb")
            .serialize(),
    );
    dummy_reveal.input[0].witness = w;
    tx_vbytes(&dummy_reveal)
}

#[derive(Clone, Debug)]
pub struct NodeCommitLogCtx {
    pub idx: usize,
    pub post_vb: u64,
    pub delta_vb: u64,
    pub total_paid: u64,
    pub commit_fee: u64,
    pub reveal_fee: u64,
    pub buffer: u64,
}

pub fn log_node_commit(ctx: &NodeCommitLogCtx) {
    info!(
        "idx={} commit_vb_with_dummy_sig={} vB; node_delta={} vB; total_fee_paid={} sat (commit_fee={} sat, reveal_fee={} sat, buffer={} sat)",
        ctx.idx,
        ctx.post_vb,
        ctx.delta_vb,
        ctx.total_paid,
        ctx.commit_fee,
        ctx.reveal_fee,
        ctx.buffer
    );
}

#[derive(Clone, Debug)]
pub struct PortalCommitLogCtx {
    pub post_vb: u64,
    pub delta_vb: u64,
    pub total_paid: u64,
    pub commit_fee: u64,
    pub reveal_fee: u64,
    pub buffer: u64,
}

pub fn log_portal_commit(ctx: &PortalCommitLogCtx) {
    info!(
        "portal commit_vb_with_dummy_sig={} vB; portal_delta={} vB; total_fee_paid={} sat (commit_fee={} sat, reveal_fee={} sat, buffer={} sat)",
        ctx.post_vb, ctx.delta_vb, ctx.total_paid, ctx.commit_fee, ctx.reveal_fee, ctx.buffer
    );
}

pub fn log_node_commit_witness(idx: usize, witness: &Witness) {
    info!(
        "idx={} produced commit witness (stack_elems={})",
        idx,
        witness.len()
    );
}

pub fn log_node_sign_size_and_fee_breakdown(
    reveal_psbt_local: &Psbt,
    i: usize,
    reveal_tx_local: &mut Transaction,
    min_sat_per_vb: u64,
) {
    // Logging: compute delta for this node
    let mut reveal_before = reveal_psbt_local.unsigned_tx.clone();
    for j in 0..reveal_before.input.len() {
        if let Some(wit) = &reveal_psbt_local.inputs[j].final_script_witness {
            reveal_before.input[j].witness = wit.clone();
        }
    }
    let before_vb_r = tx_vbytes(&reveal_before);
    let mut reveal_after = reveal_before.clone();
    reveal_after.input[i].witness = reveal_tx_local.input[i].witness.clone();
    let after_vb_r = tx_vbytes(&reveal_after);
    let delta_vb_r = after_vb_r.saturating_sub(before_vb_r);
    let in_val_r = reveal_psbt_local.inputs[i]
        .witness_utxo
        .as_ref()
        .expect("wutxo")
        .value
        .to_sat();
    let out_val_r = reveal_psbt_local.unsigned_tx.output[i].value.to_sat();
    let fee_paid_r_i = in_val_r.saturating_sub(out_val_r);
    // Compute fair share of base (non-witness) bytes across inputs
    let mut base_no_witness = reveal_psbt_local.unsigned_tx.clone();
    for inp in &mut base_no_witness.input {
        inp.witness = Witness::new();
    }
    let base_vb = tx_vbytes(&base_no_witness);
    let num_inputs = reveal_psbt_local.unsigned_tx.input.len() as u64;
    let base_share_vb = if num_inputs > 0 {
        base_vb / num_inputs
    } else {
        0
    };
    let fair_needed_fee_node =
        (base_share_vb.saturating_add(delta_vb_r)).saturating_mul(min_sat_per_vb);
    let witness_only_needed = delta_vb_r.saturating_mul(min_sat_per_vb);
    info!(
        "idx={} reveal_vb_now={} vB; node_reveal_delta={} vB; base_share={} vB; reveal_fee_paid={} sat; witness_only_needed={} sat; fair_needed={} sat",
        i,
        after_vb_r,
        delta_vb_r,
        base_share_vb,
        fee_paid_r_i,
        witness_only_needed,
        fair_needed_fee_node
    );
    assert!(
        fee_paid_r_i >= fair_needed_fee_node,
        "node {} reveal fee insufficient: paid={} < fair_needed={}",
        i,
        fee_paid_r_i,
        fair_needed_fee_node
    );

    let reveal_witness = reveal_tx_local.input[i].witness.clone();
    info!(
        "idx={} produced reveal witness (stack_elems={})",
        i,
        reveal_witness.len()
    );
}

pub fn log_total_size_and_fee_breakdown(
    commit_psbt: &Psbt,
    reveal_psbt: &Psbt,
    min_sat_per_vb: u64,
)
// Final fee accounting: compute full signed sizes and required fees, and assert total paid >= total required
{
    // Commit actual size and required fee
    let mut commit_tx_f = commit_psbt.unsigned_tx.clone();
    for i in 0..commit_psbt.inputs.len() {
        if let Some(w) = &commit_psbt.inputs[i].final_script_witness {
            commit_tx_f.input[i].witness = w.clone();
        }
    }
    let commit_vb_actual = tx_vbytes(&commit_tx_f);
    let commit_req_fee_actual = commit_vb_actual.saturating_mul(min_sat_per_vb);
    let commit_in_total: u64 = commit_psbt
        .inputs
        .iter()
        .map(|inp| inp.witness_utxo.as_ref().unwrap().value.to_sat())
        .sum();
    let commit_out_total: u64 = commit_psbt
        .unsigned_tx
        .output
        .iter()
        .map(|o| o.value.to_sat())
        .sum();
    let commit_paid_total = commit_in_total.saturating_sub(commit_out_total);

    // Reveal actual size and required fee
    let mut reveal_tx_f = reveal_psbt.unsigned_tx.clone();
    for i in 0..reveal_psbt.inputs.len() {
        if let Some(w) = &reveal_psbt.inputs[i].final_script_witness {
            reveal_tx_f.input[i].witness = w.clone();
        }
    }
    let reveal_vb_actual = tx_vbytes(&reveal_tx_f);
    let reveal_req_fee_actual = reveal_vb_actual.saturating_mul(min_sat_per_vb);
    let reveal_in_total: u64 = reveal_psbt
        .inputs
        .iter()
        .map(|inp| inp.witness_utxo.as_ref().unwrap().value.to_sat())
        .sum();
    let reveal_out_total: u64 = reveal_psbt
        .unsigned_tx
        .output
        .iter()
        .map(|o| o.value.to_sat())
        .sum();
    let reveal_paid_total = reveal_in_total.saturating_sub(reveal_out_total);

    let required_total = commit_req_fee_actual.saturating_add(reveal_req_fee_actual);
    let paid_total = commit_paid_total.saturating_add(reveal_paid_total);
    info!(
        "final: commit_size={} vB, commit_required={} sat, commit_paid={} sat; reveal_size={} vB, reveal_required={} sat, reveal_paid={} sat; overall_required={} sat, overall_paid={} sat",
        commit_vb_actual,
        commit_req_fee_actual,
        commit_paid_total,
        reveal_vb_actual,
        reveal_req_fee_actual,
        reveal_paid_total,
        required_total,
        paid_total
    );
    assert!(
        paid_total >= required_total,
        "overall fee insufficient: paid={} < required={}",
        paid_total,
        required_total
    );
}

pub fn add_single_node_input_and_output_to_psbt(
    commit_psbt: &mut Psbt,
    node_utxos: &[(OutPoint, TxOut)],
    idx: usize,
    min_sat_per_vb: u64,
    node_info: &NodeInfo,
    dust_limit_sat: u64,
) -> Result<(u64, usize, usize)> {
    info!("node idx={} appending to COMMIT", idx);
    let (node_outpoint, node_prevout) = node_utxos[idx].clone();
    // Snapshot size before adding this node to charge full delta (non-witness + witness + optional change)
    let base_before_vb = tx_vbytes(&commit_psbt.unsigned_tx);
    let node_input_index = commit_psbt.unsigned_tx.input.len();
    commit_psbt.unsigned_tx.input.push(TxIn {
        previous_output: node_outpoint,
        script_sig: bitcoin::script::ScriptBuf::new(),
        sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
        witness: Witness::new(),
    });
    commit_psbt.inputs.push(Default::default());
    commit_psbt.inputs[node_input_index].witness_utxo = Some(node_prevout.clone());
    commit_psbt.inputs[node_input_index].tap_internal_key = Some(node_info.internal_key);

    // Append script output for node at the end
    let (tap_script, tap_info, script_addr) =
        build_tap_script_and_script_address(node_info.internal_key, b"node-data".to_vec())?;

    // Estimate reveal fee the node will need to pay later (1-in script + 1-out to self)
    let reveal_vb = estimate_single_input_single_output_reveal_vbytes(
        &tap_script,
        &tap_info,
        node_info.address.script_pubkey().len(),
        dust_limit_sat,
    );
    let reveal_fee = reveal_vb.saturating_mul(min_sat_per_vb);
    let node_reveal_fee = reveal_fee;
    info!(
        "node idx={} estimated_reveal_size={} vB; estimated_reveal_fee={} sat ",
        idx, reveal_vb, reveal_fee
    );

    let script_value = dust_limit_sat + reveal_fee;

    commit_psbt.unsigned_tx.output.push(TxOut {
        value: Amount::from_sat(script_value),
        script_pubkey: script_addr.script_pubkey(),
    });
    commit_psbt.outputs.push(Default::default());

    // Estimate full commit delta for this node (input + script output + witness + optional change)
    let mut temp = commit_psbt.unsigned_tx.clone();
    let mut dw = Witness::new();
    dw.push(vec![0u8; 65]);
    temp.input[node_input_index].witness = dw;
    temp.output.push(TxOut {
        value: Amount::from_sat(0),
        script_pubkey: node_info.address.script_pubkey(),
    });
    let after_with_change_vb = tx_vbytes(&temp);
    let full_delta_vb = after_with_change_vb.saturating_sub(base_before_vb);
    let fee_full_delta = full_delta_vb.saturating_mul(min_sat_per_vb);

    let mut node_change_value = node_prevout
        .value
        .to_sat()
        .saturating_sub(script_value + fee_full_delta);

    // Include change only if above dust
    if node_change_value > dust_limit_sat {
        commit_psbt.unsigned_tx.output.push(TxOut {
            value: Amount::from_sat(node_change_value),
            script_pubkey: node_info.address.script_pubkey(),
        });
        commit_psbt.outputs.push(Default::default());
    } else {
        node_change_value = 0;
    }

    let node_input_index: usize = node_input_index;
    // script output was appended just before optional change; so it is at len-1 if no change, or len-2 if change was added
    let script_vout = if node_change_value > 0 {
        commit_psbt.unsigned_tx.output.len() - 2
    } else {
        commit_psbt.unsigned_tx.output.len() - 1
    };
    let node_script_vout = script_vout;

    let ctx = NodeCommitLogCtx {
        idx,
        post_vb: tx_vbytes(&temp),
        delta_vb: full_delta_vb,
        total_paid: node_prevout.value.to_sat() - (script_value + node_change_value),
        commit_fee: fee_full_delta,
        reveal_fee,
        buffer: fee_full_delta - full_delta_vb.saturating_mul(min_sat_per_vb),
    };
    log_node_commit(&ctx);

    Ok((node_reveal_fee, node_input_index, node_script_vout))
}

pub fn add_portal_input_and_output_to_psbt(
    commit_psbt: &mut Psbt,
    min_sat_per_vb: u64,
    dust_limit_sat: u64,
    secp: &Secp256k1<All>,
    test_cfg: &TestConfig,
) -> Result<(PortalInfo, u64, usize)> {
    // Portal participation: append portal input/output (script reveal) and charge full-delta fee like nodes
    info!("portal appending to COMMIT");
    let portal_info = get_portal_info(secp, test_cfg)?;
    let (portal_outpoint, portal_prevout) = mock_fetch_portal_utxo(&portal_info);
    let base_before_portal_vb = tx_vbytes(&commit_psbt.unsigned_tx);
    let portal_input_index = commit_psbt.unsigned_tx.input.len();
    commit_psbt.unsigned_tx.input.push(TxIn {
        previous_output: portal_outpoint,
        script_sig: bitcoin::script::ScriptBuf::new(),
        sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
        witness: Witness::new(),
    });
    commit_psbt.inputs.push(Default::default());
    commit_psbt.inputs[portal_input_index].witness_utxo = Some(portal_prevout.clone());
    commit_psbt.inputs[portal_input_index].tap_internal_key = Some(portal_info.internal_key);

    // Portal tapscript output to reveal its x-only pubkey
    let (portal_tap_script, portal_tap_info, portal_script_addr) =
        build_tap_script_and_script_address(portal_info.internal_key, b"portal-data".to_vec())?;
    let portal_reveal_vb = estimate_single_input_single_output_reveal_vbytes(
        &portal_tap_script,
        &portal_tap_info,
        portal_info.address.script_pubkey().len(),
        dust_limit_sat,
    );
    let portal_reveal_fee = portal_reveal_vb.saturating_mul(min_sat_per_vb);
    let portal_script_value = dust_limit_sat + portal_reveal_fee;
    commit_psbt.unsigned_tx.output.push(TxOut {
        value: Amount::from_sat(portal_script_value),
        script_pubkey: portal_script_addr.script_pubkey(),
    });
    commit_psbt.outputs.push(Default::default());

    // Full-delta commit fee for portal
    let mut temp_portal = commit_psbt.unsigned_tx.clone();
    let mut dwp = Witness::new();
    dwp.push(vec![0u8; 65]);
    temp_portal.input[portal_input_index].witness = dwp;
    temp_portal.output.push(TxOut {
        value: Amount::from_sat(0),
        script_pubkey: portal_info.address.script_pubkey(),
    });
    let after_with_change_portal_vb = tx_vbytes(&temp_portal);
    let portal_full_delta_vb = after_with_change_portal_vb.saturating_sub(base_before_portal_vb);
    let portal_fee_full_delta = portal_full_delta_vb.saturating_mul(min_sat_per_vb);

    let mut portal_change_value = portal_prevout
        .value
        .to_sat()
        .saturating_sub(portal_script_value + portal_fee_full_delta);
    if portal_change_value > dust_limit_sat {
        commit_psbt.unsigned_tx.output.push(TxOut {
            value: Amount::from_sat(portal_change_value),
            script_pubkey: portal_info.address.script_pubkey(),
        });
        commit_psbt.outputs.push(Default::default());
    } else {
        portal_change_value = 0;
    }
    let ctx = PortalCommitLogCtx {
        post_vb: tx_vbytes(&temp_portal),
        delta_vb: portal_full_delta_vb,
        total_paid: portal_prevout.value.to_sat() - (portal_script_value + portal_change_value),
        commit_fee: portal_fee_full_delta,
        reveal_fee: portal_reveal_fee,
        buffer: portal_fee_full_delta - portal_full_delta_vb.saturating_mul(min_sat_per_vb),
    };
    log_portal_commit(&ctx);
    Ok((portal_info, portal_change_value, portal_input_index))
}

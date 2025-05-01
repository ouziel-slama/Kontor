use anyhow::{Result, anyhow};
use bitcoin::{
    Address, Amount, FeeRate, KnownHrp, OutPoint, ScriptBuf, TxOut, Witness,
    absolute::LockTime,
    opcodes::{
        OP_0, OP_FALSE,
        all::{OP_CHECKSIG, OP_ENDIF, OP_IF},
    },
    script::{Builder, PushBytesBuf},
    secp256k1::{Secp256k1, XOnlyPublicKey},
    taproot::{LeafVersion, TaprootBuilder},
    transaction::{Transaction, TxIn, Version},
};

pub fn compose(
    sender_address: &Address,
    internal_key: &XOnlyPublicKey,
    sender_utxos: Vec<(OutPoint, TxOut)>,
    data: &[u8],
    sat_per_vb: u64,
) -> Result<(Transaction, Transaction, ScriptBuf)> {
    let fee_rate = FeeRate::from_sat_per_vb(sat_per_vb).unwrap();

    let inputs: Vec<TxIn> = sender_utxos
        .iter()
        .map(|(outpoint, _)| TxIn {
            previous_output: *outpoint,
            ..Default::default()
        })
        .collect();

    let total_input_amount: u64 = sender_utxos
        .iter()
        .map(|(_, txout)| txout.value.to_sat())
        .sum();

    let mut outputs = Vec::new();

    let secp = Secp256k1::new();
    let tap_script = Builder::new()
        .push_slice(internal_key.serialize())
        .push_opcode(OP_CHECKSIG)
        .push_opcode(OP_FALSE)
        .push_opcode(OP_IF)
        .push_slice(b"kon")
        .push_opcode(OP_0)
        .push_slice(PushBytesBuf::try_from(data.to_vec())?)
        .push_opcode(OP_ENDIF)
        .into_script();

    let taproot_spend_info = TaprootBuilder::new()
        .add_leaf(0, tap_script.clone())
        .map_err(|e| anyhow!("Failed to add leaf: {}", e))?
        .finalize(&secp, *internal_key)
        .map_err(|e| anyhow!("Failed to finalize Taproot tree: {:?}", e))?;

    let output_key = taproot_spend_info.output_key();
    let script_spendable_address = Address::p2tr_tweaked(output_key, KnownHrp::Mainnet);

    let dust = 546;
    let data_commitment_out = TxOut {
        value: Amount::from_sat(dust),
        script_pubkey: script_spendable_address.script_pubkey(),
    };

    outputs.push(data_commitment_out);

    const SCHNORR_SIGNATURE_SIZE: usize = 64;
    let fee = calculate_fee(
        |_, witness| {
            witness.push(vec![0; SCHNORR_SIGNATURE_SIZE]);
        },
        inputs.clone(),
        outputs.clone(),
        fee_rate,
        true,
    );

    let change_amount = total_input_amount
        .checked_sub(dust + fee)
        .ok_or(anyhow!("Change amount is negative"))?;

    if change_amount < dust {
        return Err(anyhow!("Change amount is dust"));
    }

    outputs.push(TxOut {
        value: Amount::from_sat(change_amount),
        script_pubkey: sender_address.script_pubkey(),
    });

    let commit_transaction = Transaction {
        version: Version(2),
        lock_time: LockTime::ZERO,
        input: inputs,
        output: outputs,
    };

    let mut reveal_transaction = Transaction {
        version: Version(2),
        lock_time: LockTime::ZERO,
        input: vec![
            TxIn {
                previous_output: OutPoint {
                    txid: commit_transaction.compute_txid(),
                    vout: 1,
                },
                ..Default::default()
            },
            TxIn {
                previous_output: OutPoint {
                    txid: commit_transaction.compute_txid(),
                    vout: 0,
                },
                ..Default::default()
            },
        ],
        output: vec![],
    };
    let control_block = taproot_spend_info
        .control_block(&(tap_script.clone(), LeafVersion::TapScript))
        .ok_or(anyhow!("Failed to create control block"))?;

    let f = |i: usize, witness: &mut Witness| {
        if i == 0 {
            witness.push(vec![0; SCHNORR_SIGNATURE_SIZE]);
        } else {
            witness.push(vec![0; SCHNORR_SIGNATURE_SIZE]);
            witness.push(tap_script.clone());
            witness.push(control_block.serialize());
        }
    };

    let fee = calculate_fee(f, reveal_transaction.input.clone(), vec![], fee_rate, false);
    let commit_outputs_sum: u64 = commit_transaction
        .output
        .iter()
        .map(|o| o.value.to_sat())
        .sum();

    let reveal_change = commit_outputs_sum
        .checked_sub(fee)
        .ok_or(anyhow!("Reveal change amount is negative"))?;

    if reveal_change > dust {
        let fee = calculate_fee(f, reveal_transaction.input.clone(), vec![], fee_rate, true);

        let reveal_change = commit_outputs_sum
            .checked_sub(fee)
            .ok_or(anyhow!("Reveal change amount is negative"))?;

        if reveal_change > dust {
            reveal_transaction.output.push(TxOut {
                value: Amount::from_sat(reveal_change),
                script_pubkey: sender_address.script_pubkey(),
            });
        }
    }

    Ok((commit_transaction, reveal_transaction, tap_script))
}

fn calculate_fee<F>(
    f: F,
    mut inputs: Vec<TxIn>,
    outputs: Vec<TxOut>,
    fee_rate: FeeRate,
    change_output: bool,
) -> u64
where
    F: Fn(usize, &mut Witness),
{
    inputs.iter_mut().enumerate().for_each(|(i, txin)| {
        f(i, &mut txin.witness);
    });

    let mut dummy_tx = Transaction {
        version: Version(2),
        lock_time: LockTime::ZERO,
        input: inputs,
        output: outputs,
    };

    if change_output {
        dummy_tx.output.push(TxOut {
            value: Amount::from_sat(0),
            script_pubkey: ScriptBuf::from_bytes(vec![0; 34]),
        });
    }

    let vsize = dummy_tx.vsize() as u64;
    fee_rate.fee_vb(vsize).unwrap().to_sat()
}

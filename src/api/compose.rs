use anyhow::{Result, anyhow};
use bitcoin::{
    Address, Amount, FeeRate, KnownHrp, OutPoint, ScriptBuf, TxOut, Witness,
    absolute::LockTime,
    opcodes::{
        OP_0, OP_FALSE,
        all::{OP_CHECKSIG, OP_ENDIF, OP_IF},
    },
    script::{Builder, PushBytesBuf},
    secp256k1::{All, Secp256k1, XOnlyPublicKey},
    taproot::{LeafVersion, TaprootBuilder, TaprootSpendInfo},
    transaction::{Transaction, TxIn, Version},
};
use bon::builder;

// USE BON BUILDER ON STRUCTS!

pub struct ComposeParams<'a> {
    // rename ComposeInputs
    pub sender_address: &'a Address,
    pub internal_key: &'a XOnlyPublicKey,
    pub sender_utxos: Vec<(OutPoint, TxOut)>,
    pub data: &'a [u8],                // rename script_data
    pub sat_per_vb: u64,               // TAKE FEE_RATE DIRECTLY
    pub second_data: Option<&'a [u8]>, // rename chained_script_data
}

impl<'a> ComposeParams<'a> {
    pub fn new(
        sender_address: &'a Address,
        internal_key: &'a XOnlyPublicKey,
        sender_utxos: Vec<(OutPoint, TxOut)>,
        data: &'a [u8],
        sat_per_vb: u64,
    ) -> Self {
        Self {
            sender_address,
            internal_key,
            sender_utxos,
            data,
            sat_per_vb,
            second_data: None,
        }
    }

    // Optional method to set second_data
    pub fn with_second_data(mut self, second_data: &'a [u8]) -> Self {
        self.second_data = Some(second_data);
        self
    }
}
pub struct ComposeReturn {
    // rename ComposeOutputs
    pub commit_transaction: Transaction,
    pub reveal_transaction: Transaction,
    pub first_tap_script: ScriptBuf,
    pub second_tap_script: Option<ScriptBuf>,
    pub second_reveal_transaction: Option<Transaction>,
}

impl ComposeReturn {
    pub fn new(
        commit_transaction: Transaction,
        reveal_transaction: Transaction,
        first_tap_script: ScriptBuf,
    ) -> Self {
        Self {
            commit_transaction,
            reveal_transaction,
            first_tap_script,
            second_tap_script: None,
            second_reveal_transaction: None,
        }
    }

    pub fn with_second_tap_script(
        mut self,
        second_tap_script: ScriptBuf,
        second_reveal_transaction: Transaction,
    ) -> Self {
        self.second_tap_script = Some(second_tap_script);
        self.second_reveal_transaction = Some(second_reveal_transaction);
        self
    }
}

/*
impl From<String> for UserId {
    fn from(value: String) -> Self {
        UserId(value)
    }
}
    do this for commit params !!
*/
pub struct CommitParams<'a> {
    pub sender_address: &'a Address,
    pub internal_key: &'a XOnlyPublicKey,
    pub sender_utxos: Vec<(OutPoint, TxOut)>,
    pub data: &'a [u8],
    pub fee_rate: FeeRate,
}

impl<'a> CommitParams<'a> {
    pub fn from_compose_params(params: &ComposeParams<'a>, fee_rate: FeeRate) -> Self {
        Self {
            sender_address: params.sender_address,
            internal_key: params.internal_key,
            sender_utxos: params.sender_utxos.clone(),
            data: params.data,
            fee_rate,
        }
    }

    pub fn new(
        sender_address: &'a Address,
        internal_key: &'a XOnlyPublicKey,
        sender_utxos: Vec<(OutPoint, TxOut)>,
        data: &'a [u8],
        fee_rate: FeeRate,
    ) -> Self {
        Self {
            sender_address,
            internal_key,
            sender_utxos,
            data,
            fee_rate,
        }
    }
}

pub struct CommitReturn {
    pub commit_transaction: Transaction,
    pub tap_script: ScriptBuf,
    pub taproot_spend_info: TaprootSpendInfo,
}

impl CommitReturn {
    pub fn new(
        commit_transaction: Transaction,
        tap_script: ScriptBuf,
        taproot_spend_info: TaprootSpendInfo,
    ) -> Self {
        Self {
            commit_transaction,
            tap_script,
            taproot_spend_info,
        }
    }
}

pub struct RevealParams<'a> {
    pub internal_key: &'a XOnlyPublicKey,
    pub sender_address: &'a Address,
    pub commit_transaction: &'a Transaction,
    pub tap_script: &'a ScriptBuf,
    pub taproot_spend_info: &'a TaprootSpendInfo,
    pub fee_rate: FeeRate,
    pub second_data: Option<&'a [u8]>, // rename chained_script_data
                                       // add op_return data
}

impl<'a> RevealParams<'a> {
    pub fn new(
        internal_key: &'a XOnlyPublicKey,
        sender_address: &'a Address,
        commit_transaction: &'a Transaction,
        tap_script: &'a ScriptBuf,
        taproot_spend_info: &'a TaprootSpendInfo,
        fee_rate: FeeRate,
    ) -> Self {
        Self {
            internal_key,
            sender_address,
            commit_transaction,
            tap_script,
            taproot_spend_info,
            fee_rate,
            second_data: None,
        }
    }

    pub fn with_second_data(mut self, second_data: &'a [u8]) -> Self {
        self.second_data = Some(second_data);
        self
    }
}

pub struct RevealReturn {
    pub reveal_transaction: Transaction,
    pub second_tap_script: Option<ScriptBuf>,
    pub second_taproot_spend_info: Option<TaprootSpendInfo>,
}

impl RevealReturn {
    pub fn new(reveal_transaction: Transaction) -> Self {
        Self {
            reveal_transaction,
            second_tap_script: None,
            second_taproot_spend_info: None,
        }
    }

    // Optional method to set second_data
    pub fn with_second_data(
        mut self,
        second_tap_script: ScriptBuf,
        second_taproot_spend_info: TaprootSpendInfo,
    ) -> Self {
        self.second_tap_script = Some(second_tap_script);
        self.second_taproot_spend_info = Some(second_taproot_spend_info);
        self
    }
}

pub fn compose(params: ComposeParams) -> Result<ComposeReturn> {
    let fee_rate = FeeRate::from_sat_per_vb(params.sat_per_vb).unwrap();

    let commit_return = compose_commit(CommitParams::from_compose_params(&params, fee_rate))?;
    let commit_transaction = commit_return.commit_transaction;
    let tap_script = commit_return.tap_script;
    let taproot_spend_info = commit_return.taproot_spend_info;

    let mut reveal_params = RevealParams::new(
        params.internal_key,
        params.sender_address,
        &commit_transaction,
        &tap_script,
        &taproot_spend_info,
        fee_rate,
    );
    if let Some(second_data) = params.second_data {
        reveal_params = reveal_params.with_second_data(second_data);
    }

    let reveal_return = compose_reveal(reveal_params)?;
    let reveal_transaction = reveal_return.reveal_transaction;
    let second_tap_script = reveal_return.second_tap_script;
    let second_taproot_spend_info = reveal_return.second_taproot_spend_info;
    // instantiate compose return and tack on extras in if, then return at end
    if second_tap_script.is_none() && second_taproot_spend_info.is_none() {
        // if chained_script is none
        Ok(ComposeReturn::new(
            commit_transaction,
            reveal_transaction,
            tap_script,
        ))
    } else {
        // do this first in a single if block
        let second_tap_script = second_tap_script.unwrap();
        let second_taproot_spend_info = second_taproot_spend_info.unwrap();
        let second_reveal_transaction = compose_reveal(RevealParams::new(
            params.internal_key,
            params.sender_address,
            &reveal_transaction,
            &second_tap_script,
            &second_taproot_spend_info,
            fee_rate,
        ))?;
        let second_reveal_transaction = second_reveal_transaction.reveal_transaction;
        // ALWAYS RETURN THIS EITHER WITH  SOME OR NONE
        Ok(
            ComposeReturn::new(commit_transaction, reveal_transaction, tap_script)
                .with_second_tap_script(second_tap_script, second_reveal_transaction),
        )
    }
}

pub fn compose_commit(params: CommitParams) -> Result<CommitReturn> {
    let sender_address = params.sender_address;
    let internal_key = params.internal_key;
    let sender_utxos = params.sender_utxos;
    let data = params.data;
    let fee_rate = params.fee_rate;

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
    let (tap_script, taproot_spend_info) =
        build_tap_script_and_spend_info(&secp, internal_key, data)?;
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

    Ok(CommitReturn::new(
        commit_transaction,
        tap_script,
        taproot_spend_info, // can be generated, don't need to return REMOVE !
    ))
}

pub fn compose_reveal(params: RevealParams) -> Result<RevealReturn> {
    let sender_address = params.sender_address;
    let commit_transaction = params.commit_transaction;
    let first_tap_script = params.tap_script;
    let first_taproot_spend_info = params.taproot_spend_info;
    let fee_rate = params.fee_rate;
    let second_data = params.second_data;

    const SCHNORR_SIGNATURE_SIZE: usize = 64;
    let dust = 546;

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
    let secp = Secp256k1::new();
    if let Some(second_data) = second_data {
        // PULL OUT TO HELPER
        let (_, second_taproot_spend_info) =
            build_tap_script_and_spend_info(&secp, params.internal_key, second_data)?;

        let output_key = second_taproot_spend_info.output_key();
        let second_data_out = TxOut {
            value: Amount::from_sat(dust),
            script_pubkey: Address::p2tr_tweaked(output_key, KnownHrp::Mainnet).script_pubkey(),
        };
        reveal_transaction.output.push(second_data_out);
    }

    let control_block = first_taproot_spend_info
        .control_block(&(first_tap_script.clone(), LeafVersion::TapScript))
        .ok_or(anyhow!("Failed to create control block"))?;

    let f = |i: usize, witness: &mut Witness| {
        if i == 0 {
            witness.push(vec![0; SCHNORR_SIGNATURE_SIZE]);
        } else {
            witness.push(vec![0; SCHNORR_SIGNATURE_SIZE]);
            witness.push(first_tap_script.clone());
            witness.push(control_block.serialize());
        }
    };

    let fee = calculate_fee(
        f,
        reveal_transaction.input.clone(),
        reveal_transaction.output.clone(),
        fee_rate,
        false,
    );
    let commit_outputs_sum: u64 = commit_transaction
        .output
        .iter()
        .map(|o| o.value.to_sat())
        .sum();

    let reveal_change = commit_outputs_sum
        .checked_sub(fee)
        .ok_or(anyhow!("Reveal change amount is negative"))?;

    if reveal_change > dust {
        let fee = calculate_fee(
            f,
            reveal_transaction.input.clone(),
            reveal_transaction.output.clone(),
            fee_rate,
            true,
        );

        let reveal_change = commit_outputs_sum.checked_sub(fee);

        if let Some(v) = reveal_change {
            if v > dust {
                reveal_transaction.output.push(TxOut {
                    value: Amount::from_sat(v),
                    script_pubkey: sender_address.script_pubkey(),
                });
            }
        };
    }

    let mut reveal_return = RevealReturn::new(reveal_transaction);

    if let Some(second_data) = second_data {
        let (second_tap_script, second_taproot_spend_info) =
            build_tap_script_and_spend_info(&secp, params.internal_key, second_data)?;
        reveal_return =
            reveal_return.with_second_data(second_tap_script, second_taproot_spend_info);
    }

    Ok(reveal_return)
}

fn build_tap_script_and_spend_info(
    secp: &Secp256k1<All>,
    internal_key: &XOnlyPublicKey,
    data: &[u8],
) -> Result<(ScriptBuf, TaprootSpendInfo)> {
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
        .finalize(secp, *internal_key)
        .map_err(|e| anyhow!("Failed to finalize Taproot tree: {:?}", e))?;

    Ok((tap_script, taproot_spend_info))
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

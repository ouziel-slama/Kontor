#![no_std]
contract!(name = "filestoragemock");

use serde::Deserialize;
use stdlib::*;

// ─────────────────────────────────────────────────────────────────
// Postcard Payload Types (for deserialization)
// ─────────────────────────────────────────────────────────────────

/// Top-level batch structure
/// [batch_id, aggregated_bls_signature, bls_public_keys, instructions]
/// Aligned with Horizon-Portal's compose.rs format.
#[derive(Deserialize)]
struct ApiBatch {
    batch_id: Vec<u8>,
    #[allow(dead_code)]
    aggregated_bls_signature: Vec<u8>,
    #[allow(dead_code)]
    bls_public_keys: Vec<Vec<u8>>,
    instructions: Vec<InstructionTuple>,
}

/// Each instruction is a tuple: [pubkey_index, instruction_array]
/// Matches Horizon-Portal's format: (usize, Vec<String>)
#[derive(Deserialize)]
struct InstructionTuple(usize, Vec<String>);

// ─────────────────────────────────────────────────────────────────
// State Types
// ─────────────────────────────────────────────────────────────────

#[derive(Clone, Default, Storage)]
struct RegisterNodeData {
    pub node_id: String,
    pub bls_public_key: String,
    pub xpubkey: String,
    pub signature: String,
}

#[derive(Clone, Default, Storage)]
struct CreateAgreementData {
    pub user_id: String,
    pub file_id: String,
    pub filename: String,
    pub original_size: String,
    pub data_symbols: String,
    pub parity_symbols: String,
    pub merkle_root: String,
    pub padded_len: String,
    pub content_hash: String,
    pub blob_size: String,
    pub bls_public_key_index: u64,
}

#[derive(Clone, Default, Storage)]
struct JoinAgreementData {
    pub node_id: String,
    pub agreement_id: String,
}

#[derive(Clone, Default, StorageRoot)]
struct MockState {
    pub register_node_calls: Map<u64, RegisterNodeData>,
    pub register_node_count: u64,
    pub create_agreement_calls: Map<u64, CreateAgreementData>,
    pub create_agreement_count: u64,
    pub join_agreement_calls: Map<u64, JoinAgreementData>,
    pub join_agreement_count: u64,
    pub processed_batch_ids: Map<u64, String>,
    pub batch_count: u64,
}

// ─────────────────────────────────────────────────────────────────
// Contract Implementation
// ─────────────────────────────────────────────────────────────────

impl Guest for Filestoragemock {
    fn init(ctx: &ProcContext) {
        MockState::default().init(ctx);
    }

    fn api_calls(ctx: &ProcContext, data: Vec<u8>) -> Result<(), Error> {
        // Parse the postcard payload
        let batch: ApiBatch = postcard::from_bytes(&data)
            .map_err(|e| Error::Message(format!("Failed to parse postcard: {:?}", e)))?;

        let model = ctx.model();

        // Store batch_id (convert Vec<u8> to hex string for storage)
        let batch_idx = model.batch_count();
        let batch_id_hex = batch.batch_id.iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();
        model.processed_batch_ids().set(batch_idx, batch_id_hex);
        model.set_batch_count(batch_idx + 1);

        // Process each instruction
        // Format from Horizon-Portal: (pubkey_index, instruction)
        for instruction_tuple in batch.instructions {
            let bls_key_index = Some(instruction_tuple.0 as u64);
            let instruction = &instruction_tuple.1;

            if instruction.is_empty() {
                continue;
            }

            let instruction_type = &instruction[0];

            match instruction_type.as_str() {
                "register_node" => {
                    if instruction.len() < 5 {
                        return Err(Error::Message(
                            "register_node requires 5 fields".to_string(),
                        ));
                    }
                    let data = RegisterNodeData {
                        node_id: instruction[1].clone(),
                        bls_public_key: instruction[2].clone(),
                        xpubkey: instruction[3].clone(),
                        signature: instruction[4].clone(),
                    };
                    let idx = model.register_node_count();
                    model.register_node_calls().set(idx, data);
                    model.set_register_node_count(idx + 1);
                }
                "create_agreement" => {
                    if instruction.len() < 11 {
                        return Err(Error::Message(
                            "create_agreement requires 11 fields".to_string(),
                        ));
                    }
                    let data = CreateAgreementData {
                        user_id: instruction[1].clone(),
                        file_id: instruction[2].clone(),
                        filename: instruction[3].clone(),
                        original_size: instruction[4].clone(),
                        data_symbols: instruction[5].clone(),
                        parity_symbols: instruction[6].clone(),
                        merkle_root: instruction[7].clone(),
                        padded_len: instruction[8].clone(),
                        content_hash: instruction[9].clone(),
                        blob_size: instruction[10].clone(),
                        bls_public_key_index: bls_key_index.unwrap_or(0),
                    };
                    let idx = model.create_agreement_count();
                    model.create_agreement_calls().set(idx, data);
                    model.set_create_agreement_count(idx + 1);
                }
                "join_agreement" => {
                    if instruction.len() < 3 {
                        return Err(Error::Message(
                            "join_agreement requires 3 fields".to_string(),
                        ));
                    }
                    let data = JoinAgreementData {
                        node_id: instruction[1].clone(),
                        agreement_id: instruction[2].clone(),
                    };
                    let idx = model.join_agreement_count();
                    model.join_agreement_calls().set(idx, data);
                    model.set_join_agreement_count(idx + 1);
                }
                _ => {
                    // Unknown instruction type - ignore for mock
                }
            }
        }

        Ok(())
    }

    fn get_register_node_calls(ctx: &ViewContext) -> Vec<RegisterNodeCall> {
        let model = ctx.model();
        let count = model.register_node_count();
        (0..count)
            .filter_map(|i| {
                model.register_node_calls().get(&i).map(|d| RegisterNodeCall {
                    node_id: d.node_id(),
                    bls_public_key: d.bls_public_key(),
                    xpubkey: d.xpubkey(),
                    signature: d.signature(),
                })
            })
            .collect()
    }

    fn get_create_agreement_calls(ctx: &ViewContext) -> Vec<CreateAgreementCall> {
        let model = ctx.model();
        let count = model.create_agreement_count();
        (0..count)
            .filter_map(|i| {
                model
                    .create_agreement_calls()
                    .get(&i)
                    .map(|d| CreateAgreementCall {
                        user_id: d.user_id(),
                        file_id: d.file_id(),
                        filename: d.filename(),
                        original_size: d.original_size(),
                        data_symbols: d.data_symbols(),
                        parity_symbols: d.parity_symbols(),
                        merkle_root: d.merkle_root(),
                        padded_len: d.padded_len(),
                        content_hash: d.content_hash(),
                        blob_size: d.blob_size(),
                        bls_public_key_index: d.bls_public_key_index(),
                    })
            })
            .collect()
    }

    fn get_join_agreement_calls(ctx: &ViewContext) -> Vec<JoinAgreementCall> {
        let model = ctx.model();
        let count = model.join_agreement_count();
        (0..count)
            .filter_map(|i| {
                model
                    .join_agreement_calls()
                    .get(&i)
                    .map(|d| JoinAgreementCall {
                        node_id: d.node_id(),
                        agreement_id: d.agreement_id(),
                    })
            })
            .collect()
    }

    fn get_processed_batch_ids(ctx: &ViewContext) -> Vec<String> {
        let model = ctx.model();
        let count = model.batch_count();
        (0..count)
            .filter_map(|i| model.processed_batch_ids().get(&i))
            .collect()
    }

    fn call_counts(ctx: &ViewContext) -> CallCountsResult {
        let model = ctx.model();
        CallCountsResult {
            register_node_count: model.register_node_count(),
            create_agreement_count: model.create_agreement_count(),
            join_agreement_count: model.join_agreement_count(),
        }
    }
}

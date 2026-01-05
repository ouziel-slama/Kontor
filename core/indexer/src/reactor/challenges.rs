//! Challenge generation and processing for the file storage protocol.
//!
//! This module handles:
//! - Deterministic challenge generation based on block hash
//! - Expiration processing for unanswered challenges

use anyhow::Result;
use libsql::Connection;
use sha2::{Digest, Sha256};
use tracing::info;

use crate::database::{
    queries::{expire_challenges_at_height, insert_challenge},
    types::{ChallengeRow, ChallengeStatus},
};

/// Configuration for challenge generation
pub struct ChallengeConfig {
    /// Probability threshold (0-255) for challenging an agreement per block
    /// e.g., 10 means ~4% chance per block
    pub challenge_probability: u8,
    /// Number of blocks a node has to respond to a challenge
    pub deadline_blocks: i64,
}

impl Default for ChallengeConfig {
    fn default() -> Self {
        Self {
            challenge_probability: 10, // ~4% per block
            deadline_blocks: 2016,     // ~2 weeks at 10 min/block
        }
    }
}

/// Represents an active agreement with its nodes for challenge selection
#[derive(Debug, Clone)]
pub struct ActiveAgreement {
    pub agreement_id: String,
    pub file_depth: u64,
    pub nodes: Vec<String>,
}

/// Generate challenges for a block.
///
/// This is called at the start of block processing and deterministically
/// generates challenges based on the block hash.
pub async fn generate_challenges(
    conn: &Connection,
    block_height: i64,
    block_hash: &[u8; 32],
    active_agreements: &[ActiveAgreement],
    config: &ChallengeConfig,
) -> Result<Vec<ChallengeRow>> {
    let mut generated = Vec::new();

    for agreement in active_agreements {
        if agreement.nodes.is_empty() {
            continue;
        }

        // Derive deterministic seed for this agreement
        let seed = derive_challenge_seed(block_hash, &agreement.agreement_id);

        // Check if this agreement should be challenged this block
        if seed[0] >= config.challenge_probability {
            continue;
        }

        // Select node deterministically
        let node_index =
            u64::from_le_bytes(seed[1..9].try_into().unwrap()) as usize % agreement.nodes.len();
        let selected_node = &agreement.nodes[node_index];

        // Select chunk deterministically
        let chunk_count = 1u64 << agreement.file_depth; // 2^depth
        let chunk_index = u64::from_le_bytes(seed[9..17].try_into().unwrap()) % chunk_count;

        // Generate unique challenge ID
        let challenge_id = compute_challenge_id(&seed, selected_node, chunk_index);

        let challenge = ChallengeRow::builder()
            .challenge_id(challenge_id)
            .agreement_id(agreement.agreement_id.clone())
            .node_id(selected_node.clone())
            .chunk_index(chunk_index as i64)
            .issued_height(block_height)
            .deadline_height(block_height + config.deadline_blocks)
            .status(ChallengeStatus::Pending)
            .build();

        insert_challenge(conn, &challenge).await?;
        generated.push(challenge);
    }

    if !generated.is_empty() {
        info!(
            "Generated {} challenges at height {}",
            generated.len(),
            block_height
        );
    }

    Ok(generated)
}

/// Process expired challenges at the end of block processing.
///
/// Marks all pending challenges past their deadline as expired.
pub async fn process_expired_challenges(conn: &Connection, current_height: i64) -> Result<u64> {
    let expired_count = expire_challenges_at_height(conn, current_height).await?;

    if expired_count > 0 {
        info!(
            "Expired {} challenges at height {}",
            expired_count, current_height
        );
    }

    Ok(expired_count)
}

/// Derive a deterministic seed from block hash and agreement ID.
///
/// This ensures all indexers generate the same challenges.
fn derive_challenge_seed(block_hash: &[u8; 32], agreement_id: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(block_hash);
    hasher.update(b"kontor_challenge");
    hasher.update(agreement_id.as_bytes());
    hasher.finalize().into()
}

/// Compute a unique challenge ID from seed, node, and chunk.
fn compute_challenge_id(seed: &[u8; 32], node_id: &str, chunk_index: u64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(seed);
    hasher.update(node_id.as_bytes());
    hasher.update(chunk_index.to_le_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_challenge_seed_deterministic() {
        let block_hash = [1u8; 32];
        let agreement_id = "test_agreement";

        let seed1 = derive_challenge_seed(&block_hash, agreement_id);
        let seed2 = derive_challenge_seed(&block_hash, agreement_id);

        assert_eq!(seed1, seed2);
    }

    #[test]
    fn test_derive_challenge_seed_differs_by_agreement() {
        let block_hash = [1u8; 32];

        let seed1 = derive_challenge_seed(&block_hash, "agreement_1");
        let seed2 = derive_challenge_seed(&block_hash, "agreement_2");

        assert_ne!(seed1, seed2);
    }

    #[test]
    fn test_compute_challenge_id_unique() {
        let seed = [1u8; 32];

        let id1 = compute_challenge_id(&seed, "node_1", 0);
        let id2 = compute_challenge_id(&seed, "node_1", 1);
        let id3 = compute_challenge_id(&seed, "node_2", 0);

        assert_ne!(id1, id2);
        assert_ne!(id1, id3);
        assert_ne!(id2, id3);
    }
}

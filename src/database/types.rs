use bitcoin::BlockHash;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Block {
    pub height: u64,
    pub hash: BlockHash,
}

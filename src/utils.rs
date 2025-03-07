use bitcoin::{Txid, hashes::Hash};

use crate::block::HasTxid;

#[derive(Clone, Debug, PartialEq)]
pub struct MockTransaction {
    txid: Txid,
}

impl MockTransaction {
    pub fn new(txid_num: u32) -> Self {
        let mut bytes = [0u8; 32];
        bytes[0..4].copy_from_slice(&txid_num.to_le_bytes()); // Use the 4 bytes of txid_num
        MockTransaction {
            txid: Txid::from_slice(&bytes).unwrap(),
        }
    }
}

impl HasTxid for MockTransaction {
    fn txid(&self) -> Txid {
        self.txid
    }
}

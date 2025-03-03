use std::fmt;

use bitcoin::{Block, BlockHash, Transaction, Txid};

#[derive(Debug)]
pub enum FollowEvent {
    ZmqConnected,
    ZmqDisconnected(anyhow::Error),
    MempoolTransactionAdded(Transaction),
    MempoolTransactionsRemoved(Vec<Txid>),
    BlockConnected(Block),
    Rollback(BlockHash),
}

impl fmt::Display for FollowEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FollowEvent::ZmqConnected => write!(f, "ZMQ connected"),
            FollowEvent::ZmqDisconnected(e) => write!(f, "ZMQ disconnected with error: {}", e),
            FollowEvent::MempoolTransactionAdded(tx) => {
                write!(f, "Mempool transaction added: {}", tx.compute_txid())
            }
            FollowEvent::MempoolTransactionsRemoved(txids) => {
                write!(f, "Mempool transactions removed: {}", txids.len())
            }
            FollowEvent::BlockConnected(block) => write!(
                f,
                "Block connected: {} {}",
                block.bip34_block_height().unwrap(),
                block.block_hash()
            ),
            FollowEvent::Rollback(block_hash) => write!(f, "Rollback from block: {}", block_hash),
        }
    }
}

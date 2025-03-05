use std::fmt;

use bitcoin::{Block, Transaction, Txid};

use super::message::SequenceMessage;

#[derive(Debug)]
pub enum ZmqEvent {
    Connected,
    Disconnected(anyhow::Error),
    SequenceMessage(SequenceMessage),
    MempoolTransactions(Vec<Transaction>),
    BlockConnected(Block),
}

impl fmt::Display for ZmqEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ZmqEvent::Connected => write!(f, "ZMQ connected"),
            ZmqEvent::Disconnected(e) => write!(f, "ZMQ disconnected with error: {}", e),
            ZmqEvent::SequenceMessage(sequence_message) => {
                write!(f, "ZMQ sequence message: {:?}", sequence_message)
            }
            ZmqEvent::MempoolTransactions(txs) => {
                write!(f, "ZMQ mempool transactions: {}", txs.len())
            }
            ZmqEvent::BlockConnected(block) => {
                write!(f, "ZMQ block connected: {}", block.block_hash())
            }
        }
    }
}

#[derive(Debug)]
pub enum Event {
    MempoolUpdates {
        added: Vec<Transaction>,
        removed: Vec<Txid>,
    },
    Block(Block),
    Rollback(u64),
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::MempoolUpdates { added, removed } => write!(
                f,
                "Mempool updates: added {} removed {}",
                added.len(),
                removed.len()
            ),
            Event::Rollback(block_hash) => {
                write!(f, "Rollback: {}", block_hash)
            }
            Event::Block(block) => {
                write!(f, "Block: {}", block.block_hash())
            }
        }
    }
}

use std::fmt;

use bitcoin::Transaction;

use super::zmq::SequenceMessage;

#[derive(Debug)]
pub enum ZmqEvent {
    Connected,
    Disconnected(anyhow::Error),
    SequenceMessage(SequenceMessage),
    MempoolTransactions(Vec<Transaction>),
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
        }
    }
}

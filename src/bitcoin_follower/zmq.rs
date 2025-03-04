use std::thread;

use anyhow::{Context, Result, anyhow};
use bitcoin::{BlockHash, Transaction, Txid, hashes::Hash};
use scopeguard::defer;
use tokio::{
    select,
    sync::mpsc::{self, UnboundedSender},
    task::{self, JoinHandle},
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use zmq::Socket;

use crate::{
    bitcoin_client,
    config::Config,
    retry::{new_backoff_limited, new_backoff_unlimited, retry},
};

use super::event::ZmqEvent;

#[derive(Debug, PartialEq)]
pub enum MonitorMessage {
    Connected,               // 0x0001
    ConnectDelayed,          // 0x0002
    ConnectRetried,          // 0x0004
    Listening,               // 0x0008
    BindFailed,              // 0x0010
    Accepted,                // 0x0020
    AcceptFailed,            // 0x0040
    Closed,                  // 0x0080
    CloseFailed,             // 0x0100
    Disconnected,            // 0x0200
    MonitorStopped,          // 0x0400
    HandshakeFailedNoDetail, // 0x0800
    HandshakeSucceeded,      // 0x1000
    HandshakeFailedProtocol, // 0x2000
    HandshakeFailedAuth,     // 0x4000
    Unknown(u16),            // Catch-all
}

impl MonitorMessage {
    pub fn from_raw(event_type: u16) -> Self {
        match event_type {
            0x0001 => MonitorMessage::Connected,
            0x0002 => MonitorMessage::ConnectDelayed,
            0x0004 => MonitorMessage::ConnectRetried,
            0x0008 => MonitorMessage::Listening,
            0x0010 => MonitorMessage::BindFailed,
            0x0020 => MonitorMessage::Accepted,
            0x0040 => MonitorMessage::AcceptFailed,
            0x0080 => MonitorMessage::Closed,
            0x0100 => MonitorMessage::CloseFailed,
            0x0200 => MonitorMessage::Disconnected,
            0x0400 => MonitorMessage::MonitorStopped,
            0x0800 => MonitorMessage::HandshakeFailedNoDetail,
            0x1000 => MonitorMessage::HandshakeSucceeded,
            0x2000 => MonitorMessage::HandshakeFailedProtocol,
            0x4000 => MonitorMessage::HandshakeFailedAuth,
            other => MonitorMessage::Unknown(other),
        }
    }

    pub fn to_raw(&self) -> u16 {
        match self {
            MonitorMessage::Connected => 0x0001,
            MonitorMessage::ConnectDelayed => 0x0002,
            MonitorMessage::ConnectRetried => 0x0004,
            MonitorMessage::Listening => 0x0008,
            MonitorMessage::BindFailed => 0x0010,
            MonitorMessage::Accepted => 0x0020,
            MonitorMessage::AcceptFailed => 0x0040,
            MonitorMessage::Closed => 0x0080,
            MonitorMessage::CloseFailed => 0x0100,
            MonitorMessage::Disconnected => 0x0200,
            MonitorMessage::MonitorStopped => 0x0400,
            MonitorMessage::HandshakeFailedNoDetail => 0x0800,
            MonitorMessage::HandshakeSucceeded => 0x1000,
            MonitorMessage::HandshakeFailedProtocol => 0x2000,
            MonitorMessage::HandshakeFailedAuth => 0x4000,
            MonitorMessage::Unknown(val) => *val,
        }
    }

    pub fn is_failure(&self) -> bool {
        matches!(
            self,
            MonitorMessage::ConnectRetried
                | MonitorMessage::Closed
                | MonitorMessage::CloseFailed
                | MonitorMessage::Disconnected
                | MonitorMessage::HandshakeFailedNoDetail
                | MonitorMessage::HandshakeFailedProtocol
                | MonitorMessage::HandshakeFailedAuth
        )
    }

    pub fn all_events_mask() -> i32 {
        0xFFFF
    }

    pub fn failure_events_mask() -> i32 {
        0x0004 | 0x0100 | 0x0200 | 0x0800 | 0x2000 | 0x4000
        // CONNECT_RETRIED | CLOSE_FAILED | DISCONNECTED |
        // HANDSHAKE_FAILED_NO_DETAIL | HANDSHAKE_FAILED_PROTOCOL | HANDSHAKE_FAILED_AUTH
    }

    pub fn from_zmq_message(multipart: Vec<Vec<u8>>) -> Result<Self> {
        if multipart.is_empty() || multipart[0].len() < 2 {
            return Err(anyhow!("Received invalid multipart message"));
        }
        let event_type = u16::from_le_bytes(multipart[0][0..2].try_into().unwrap());
        Ok(MonitorMessage::from_raw(event_type))
    }
}

fn run_monitor_socket(
    socket: Socket,
    cancel_token: CancellationToken,
    tx: UnboundedSender<Result<MonitorMessage>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        loop {
            if cancel_token.is_cancelled() {
                info!("Cancelling monitor socket thread");
                break;
            }

            match socket.recv_multipart(0) {
                Ok(multipart) => {
                    if tx
                        .send(MonitorMessage::from_zmq_message(multipart))
                        .is_err()
                    {
                        info!("Send channel is closed, exiting monitor socket thread");
                        break;
                    }
                }
                Err(zmq::Error::EAGAIN) => {
                    continue;
                }
                Err(e) => {
                    if tx.send(Err(e.into())).is_err() {
                        info!("Send channel is closed, exiting monitor socket thread");
                        break;
                    }
                }
            }
        }

        info!("Monitor socket thread exited");
    })
}

const SEQUENCE: &str = "sequence";

#[derive(Debug)]
pub enum SequenceMessage {
    BlockConnected(BlockHash),
    BlockDisconnected(BlockHash),
    TransactionAdded {
        txid: Txid,
        mempool_sequence_number: u64,
    },
    TransactionRemoved {
        txid: Txid,
        mempool_sequence_number: u64,
    },
}

impl SequenceMessage {
    pub fn from_zmq_message(mut multipart: Vec<Vec<u8>>) -> Result<(u32, Self)> {
        if multipart.len() != 3 || multipart[0] != SEQUENCE.as_bytes() {
            return Err(anyhow!("Received invalid multipart message"));
        }

        let sequence_number = u32::from_le_bytes(multipart[2][..].try_into()?);

        let data = &mut multipart[1];
        let len = data.len();
        if len < 33 {
            return Err(anyhow!("Received message of invalid length"));
        }

        let flag = data[32];
        data[..32].reverse();
        let hash_slice = &data[..32];
        match (flag, len) {
            (b'C', 33) => Ok((
                sequence_number,
                SequenceMessage::BlockConnected(BlockHash::from_slice(hash_slice)?),
            )),
            (b'D', 33) => Ok((
                sequence_number,
                SequenceMessage::BlockDisconnected(BlockHash::from_slice(hash_slice)?),
            )),
            (b'A', 41) => Ok((
                sequence_number,
                SequenceMessage::TransactionAdded {
                    txid: Txid::from_slice(hash_slice)?,
                    mempool_sequence_number: u64::from_le_bytes(data[33..41].try_into()?),
                },
            )),
            (b'R', 41) => Ok((
                sequence_number,
                SequenceMessage::TransactionRemoved {
                    txid: Txid::from_slice(hash_slice)?,
                    mempool_sequence_number: u64::from_le_bytes(data[33..41].try_into()?),
                },
            )),
            _ => Err(anyhow!("Received message with unknown flag")),
        }
    }
}

fn run_socket(
    socket: Socket,
    cancel_token: CancellationToken,
    tx: UnboundedSender<Result<(u32, SequenceMessage)>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        loop {
            if cancel_token.is_cancelled() {
                info!("Cancelling socket thread");
                break;
            }

            match socket.recv_multipart(0) {
                Ok(zmq_message) => {
                    if tx
                        .send(SequenceMessage::from_zmq_message(zmq_message))
                        .is_err()
                    {
                        info!("Send channel is closed, exiting socket thread");
                        break;
                    }
                }
                Err(zmq::Error::EAGAIN) => {
                    continue;
                }
                Err(e) => {
                    if tx.send(Err(e.into())).is_err() {
                        info!("Send channel is closed, exiting socket thread");
                        break;
                    }
                }
            }
        }

        info!("Socket thread exited");
    })
}

pub async fn run(
    config: Config,
    cancel_token: CancellationToken,
    bitcoin: bitcoin_client::Client,
    tx: UnboundedSender<ZmqEvent>,
) -> Result<JoinHandle<Result<()>>> {
    let (socket_tx, mut socket_rx) = mpsc::unbounded_channel();
    let (monitor_tx, mut monitor_rx) = mpsc::unbounded_channel();
    let socket_cancel_token = CancellationToken::new();
    let ctx = zmq::Context::new();
    let socket = ctx.socket(zmq::SUB).expect("Failed to create ZMQ socket");
    socket.set_subscribe(SEQUENCE.as_bytes())?;
    socket.set_rcvhwm(0)?;
    socket.set_rcvtimeo(1000)?;

    let monitor_endpoint = format!("inproc://{}-monitor", SEQUENCE);
    socket
        .monitor(&monitor_endpoint, MonitorMessage::all_events_mask())
        .context("Failed to set up socket monitor")?;
    let monitor_socket = ctx
        .socket(zmq::PAIR)
        .context("Failed to create monitor socket")?;
    monitor_socket
        .connect(&monitor_endpoint)
        .context("Failed to connect monitor socket")?;
    monitor_socket.set_rcvhwm(0)?;
    monitor_socket.set_rcvtimeo(1000)?;
    let monitor_socket_handle =
        run_monitor_socket(monitor_socket, socket_cancel_token.clone(), monitor_tx);

    socket
        .connect(&config.zmq_pub_sequence_address)
        .context("Could not connect to ZMQ address")?;
    let socket_handle = run_socket(socket, socket_cancel_token.clone(), socket_tx.clone());

    info!("Getting mempool transactions...");
    let mempool_txs = retry(
        || bitcoin.get_raw_mempool(),
        "get raw mempool",
        new_backoff_unlimited(),
        cancel_token.clone(),
    )
    .await?;
    let mut txs: Vec<Transaction> = vec![];
    for txids in mempool_txs.chunks(100) {
        let results = retry(
            || bitcoin.get_raw_transactions(txids),
            "get raw transactions",
            new_backoff_limited(),
            cancel_token.clone(),
        )
        .await?;
        txs.extend(results.into_iter().filter_map(Result::ok));
    }
    let _ = tx.send(ZmqEvent::MempoolTransactions(txs));

    Ok(task::spawn(async move {
        defer! {
            socket_cancel_token.cancel();
            if socket_handle.join().is_err() {
                error!("Socket thread panicked on join");
            }
            if monitor_socket_handle.join().is_err() {
                error!("Monitor socket thread panicked on join");
            }

            info!("Exited");
        }

        let mut last_sequence_number: Option<u32> = None;

        loop {
            select! {
                biased;
                _ = cancel_token.cancelled() => {
                    info!("Cancelled");
                    return Ok(())
                },
                option_monitor_event = monitor_rx.recv() => {
                    match option_monitor_event {
                        Some(Ok(event)) => {
                            if event.is_failure() {
                                return Err(anyhow!("Received failure event from monitor socket: {:?}", event));
                            }
                            if let MonitorMessage::HandshakeSucceeded = event {
                                if tx.send(ZmqEvent::Connected).is_err() {
                                    info!("Send channel is closed, exiting")
                                }
                            }
                        },
                        Some(Err(e)) => {
                            return Err(e.context("Received Err from monitor socket thread, exiting"));
                        },
                        None => {
                            warn!("Received None message from monitor socket thread, exiting");
                            return Ok(());
                        },
                    }
                },
                option_message = socket_rx.recv() => {
                    match option_message {
                        Some(Ok((sequence_number, sequence_message))) => {
                            if let Some(n) = last_sequence_number {
                                if sequence_number != n.wrapping_add(1) {
                                    return Err(anyhow!(
                                        "Received out of sequence messages: {} {}",
                                        n, sequence_number
                                    ));
                                }
                            }
                            last_sequence_number = Some(sequence_number);
                            let event = match sequence_message {
                                SequenceMessage::BlockConnected(block_hash) => {
                                    let block = retry(
                                        || bitcoin.get_block(&block_hash),
                                        "get block",
                                        new_backoff_limited(),
                                        cancel_token.clone(),
                                    )
                                    .await
                                    .context("Failed to get block handling BlockConnected sequence message")?;
                                    ZmqEvent::BlockConnected(block)
                                }
                                _ => ZmqEvent::SequenceMessage(sequence_message),
                            };
                            if tx.send(event).is_err() {
                                info!("Send channel is closed, exiting")
                            }
                        },
                        Some(Err(e)) => {
                            return Err(e.context("Received Err from socket thread, exiting"));
                        },
                        None => {
                            warn!("Received None message from socket thread, exiting");
                            return Ok(());
                        },
                    }
                },
            }
        }
    }))
}

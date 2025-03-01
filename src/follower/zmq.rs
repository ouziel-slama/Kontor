use std::{collections::HashSet, thread};

use anyhow::{Context, Result, anyhow};
use bitcoin::{BlockHash, Txid, hashes::Hash};
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
    retry::{new_backoff_unlimited, retry},
};

#[derive(Debug, PartialEq)]
pub enum ZmqMonitorEvent {
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

impl ZmqMonitorEvent {
    pub fn from_raw(event_type: u16) -> Self {
        match event_type {
            0x0001 => ZmqMonitorEvent::Connected,
            0x0002 => ZmqMonitorEvent::ConnectDelayed,
            0x0004 => ZmqMonitorEvent::ConnectRetried,
            0x0008 => ZmqMonitorEvent::Listening,
            0x0010 => ZmqMonitorEvent::BindFailed,
            0x0020 => ZmqMonitorEvent::Accepted,
            0x0040 => ZmqMonitorEvent::AcceptFailed,
            0x0080 => ZmqMonitorEvent::Closed,
            0x0100 => ZmqMonitorEvent::CloseFailed,
            0x0200 => ZmqMonitorEvent::Disconnected,
            0x0400 => ZmqMonitorEvent::MonitorStopped,
            0x0800 => ZmqMonitorEvent::HandshakeFailedNoDetail,
            0x1000 => ZmqMonitorEvent::HandshakeSucceeded,
            0x2000 => ZmqMonitorEvent::HandshakeFailedProtocol,
            0x4000 => ZmqMonitorEvent::HandshakeFailedAuth,
            other => ZmqMonitorEvent::Unknown(other),
        }
    }

    pub fn to_raw(&self) -> u16 {
        match self {
            ZmqMonitorEvent::Connected => 0x0001,
            ZmqMonitorEvent::ConnectDelayed => 0x0002,
            ZmqMonitorEvent::ConnectRetried => 0x0004,
            ZmqMonitorEvent::Listening => 0x0008,
            ZmqMonitorEvent::BindFailed => 0x0010,
            ZmqMonitorEvent::Accepted => 0x0020,
            ZmqMonitorEvent::AcceptFailed => 0x0040,
            ZmqMonitorEvent::Closed => 0x0080,
            ZmqMonitorEvent::CloseFailed => 0x0100,
            ZmqMonitorEvent::Disconnected => 0x0200,
            ZmqMonitorEvent::MonitorStopped => 0x0400,
            ZmqMonitorEvent::HandshakeFailedNoDetail => 0x0800,
            ZmqMonitorEvent::HandshakeSucceeded => 0x1000,
            ZmqMonitorEvent::HandshakeFailedProtocol => 0x2000,
            ZmqMonitorEvent::HandshakeFailedAuth => 0x4000,
            ZmqMonitorEvent::Unknown(val) => *val,
        }
    }

    pub fn is_failure(&self) -> bool {
        matches!(
            self,
            ZmqMonitorEvent::ConnectRetried
                | ZmqMonitorEvent::CloseFailed
                | ZmqMonitorEvent::Disconnected
                | ZmqMonitorEvent::HandshakeFailedNoDetail
                | ZmqMonitorEvent::HandshakeFailedProtocol
                | ZmqMonitorEvent::HandshakeFailedAuth
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
        Ok(ZmqMonitorEvent::from_raw(event_type))
    }
}

fn run_monitor_socket(
    socket: Socket,
    cancel_token: CancellationToken,
    tx: UnboundedSender<Result<ZmqMonitorEvent>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        loop {
            if cancel_token.is_cancelled() {
                info!("ZMQ listener cancelled, cancelling monitor socket thread");
                break;
            }

            match socket.recv_multipart(0) {
                Ok(multipart) => {
                    if tx
                        .send(ZmqMonitorEvent::from_zmq_message(multipart))
                        .is_err()
                    {
                        info!("ZMQ listener send channel is closed, exiting monitor socket thread");
                        break;
                    }
                }
                Err(zmq::Error::EAGAIN) => {
                    continue;
                }
                Err(e) => {
                    if tx.send(Err(e.into())).is_err() {
                        info!("ZMQ listener send channel is closed, exiting monitor socket thread");
                        break;
                    }
                }
            }
        }

        info!("ZMQ listener monitor socket thread exited");
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
                info!("ZMQ listener cancelled, cancelling socket thread");
                break;
            }

            match socket.recv_multipart(0) {
                Ok(zmq_message) => {
                    if tx
                        .send(SequenceMessage::from_zmq_message(zmq_message))
                        .is_err()
                    {
                        info!("ZMQ listener send channel is closed, exiting socket thread");
                        break;
                    }
                }
                Err(zmq::Error::EAGAIN) => {
                    continue;
                }
                Err(e) => {
                    if tx.send(Err(e.into())).is_err() {
                        info!("ZMQ listener send channel is closed, exiting socket thread");
                        break;
                    }
                }
            }
        }

        info!("ZMQ listener socket thread exited");
    })
}

async fn handle_sequence_message(
    cancel_token: CancellationToken,
    bitcoin: bitcoin_client::Client,
    set: &mut HashSet<Txid>,
    msg: SequenceMessage,
) -> Result<()> {
    match msg {
        SequenceMessage::BlockConnected(hash) => {
            let result = retry(
                || bitcoin.get_block(&hash),
                "get block after block connected event",
                new_backoff_unlimited(),
                cancel_token.clone(),
            )
            .await;
            match result {
                Err(e) => {
                    return Err(e).context("Failed to fetch block after block connected event");
                }
                Ok(block) => {
                    for tx in block.txdata {
                        let txid = tx.compute_txid();
                        set.remove(&txid);
                    }
                    info!("Block Connected: {:?}", hash);
                }
            }
        }
        SequenceMessage::BlockDisconnected(hash) => info!("Block Disconnected: {:?}", hash),
        SequenceMessage::TransactionAdded {
            txid,
            mempool_sequence_number,
            ..
        } => {
            if set.insert(txid) {
                info!("Tx Added: {:?}, seq={}", txid, mempool_sequence_number);
            }
        }
        SequenceMessage::TransactionRemoved {
            txid,
            mempool_sequence_number,
            ..
        } => {
            if set.remove(&txid) {
                info!("Tx Removed: {:?}, seq={}", txid, mempool_sequence_number);
            }
        }
    }
    Ok(())
}

pub async fn run(
    config: Config,
    cancel_token: CancellationToken,
    bitcoin: bitcoin_client::Client,
) -> Result<JoinHandle<Result<()>>> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (monitor_tx, mut monitor_rx) = mpsc::unbounded_channel();
    let socket_cancel_token = CancellationToken::new();
    let ctx = zmq::Context::new();
    let socket = ctx.socket(zmq::SUB).expect("Failed to create ZMQ socket");
    socket.set_subscribe(SEQUENCE.as_bytes())?;
    socket.set_rcvhwm(0)?;
    socket.set_rcvtimeo(1000)?;

    let monitor_endpoint = format!("inproc://{}-monitor", SEQUENCE);
    socket
        .monitor(&monitor_endpoint, ZmqMonitorEvent::all_events_mask())
        .context("Failed to set up socket monitor")?;
    let monitor_socket = ctx
        .socket(zmq::PAIR)
        .context("Failed to create monitor socket")?;
    monitor_socket
        .connect(&monitor_endpoint)
        .context("Failed to connect monitor socket")?;
    monitor_socket.set_rcvhwm(0)?;
    monitor_socket.set_rcvtimeo(1000)?;

    socket
        .connect(&config.zmq_pub_sequence_address)
        .context("Could not connect to ZMQ address")?;
    info!(
        "Connected to Bitcoin ZMQ @ {}",
        config.zmq_pub_sequence_address
    );
    let socket_handle = run_socket(socket, socket_cancel_token.clone(), tx.clone());
    let monitor_socket_handle =
        run_monitor_socket(monitor_socket, socket_cancel_token.clone(), monitor_tx);

    Ok(task::spawn(async move {
        defer! {
            socket_cancel_token.cancel();
            if socket_handle.join().is_err() {
                error!("ZMQ socket thread panicked");
            }
            if monitor_socket_handle.join().is_err() {
                error!("ZMQ monitor socket thread panicked");
            }

            info!("ZMQ listener exited");
        }

        let mut set = HashSet::new();
        let mut last_sequence_number: Option<u32> = None;

        loop {
            select! {
                _ = cancel_token.cancelled() => {
                    info!("ZMQ listener cancelled");
                    return Ok(())
                },
                option_monitor_event = monitor_rx.recv() => {
                    match option_monitor_event {
                        Some(Ok(event)) => {
                            if event.is_failure() {
                                return Err(anyhow!("ZMQ listener received failure event from monitor socket: {:?}", event));
                            }
                            info!("Monitor event received: {:?}", event);
                        },
                        Some(Err(e)) => {
                            return Err(e.context("ZMQ listener received Err from monitor socket thread, exiting"));
                        },
                        None => {
                            warn!("ZMQ listener received None message from monitor socket thread, exiting");
                            return Ok(());
                        },
                    }
                },
                option_message = rx.recv() => {
                    match option_message {
                        Some(Ok((sequence_number, sequence_message))) => {
                            if let Some(n) = last_sequence_number {
                                if sequence_number != n.wrapping_add(1) {
                                    return Err(anyhow!(
                                        "ZMQ listener received out of sequence messages: {} {}",
                                        n, sequence_number
                                    ));
                                }
                            }
                            last_sequence_number = Some(sequence_number);
                            if let Err(e) = handle_sequence_message(
                                cancel_token.clone(),
                                bitcoin.clone(),
                                &mut set,
                                sequence_message,
                            )
                            .await
                            {
                                return Err(e.context("ZMQ listener failed to handle message"));
                            }
                        },
                        Some(Err(e)) => {
                            return Err(e.context("ZMQ listener received Err from socket thread, exiting"));
                        },
                        None => {
                            warn!("ZMQ listener received None message from socket thread, exiting");
                            return Ok(());
                        },
                    }
                },
            }
        }
    }))
}

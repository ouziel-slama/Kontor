use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use anyhow::Result;
use bitcoin::{Transaction, Txid};
use tokio::{
    select,
    sync::mpsc::{self, UnboundedSender},
    task::JoinHandle,
    time::sleep,
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::{
    bitcoin_client,
    bitcoin_follower::message::SequenceMessage,
    config::Config,
    retry::{new_backoff_limited, retry},
};

use super::{
    event::{Event, ZmqEvent},
    zmq,
};

async fn zmq_runner(
    config: Config,
    cancel_token: CancellationToken,
    bitcoin: bitcoin_client::Client,
    tx: UnboundedSender<ZmqEvent>,
) -> JoinHandle<Result<()>> {
    tokio::spawn(async move {
        loop {
            if cancel_token.is_cancelled() {
                return Ok(());
            }

            let handle = zmq::run(
                config.clone(),
                cancel_token.clone(),
                bitcoin.clone(),
                tx.clone(),
            )
            .await?;

            match handle.await {
                Ok(Ok(_)) => return Ok(()),
                Ok(Err(e)) => {
                    error!("ZMQ listener exited with error: {}", e);
                    if tx.send(ZmqEvent::Disconnected(e)).is_err() {
                        return Ok(());
                    }
                }
                Err(e) => {
                    error!("ZMQ listener panicked on join");
                    if tx.send(ZmqEvent::Disconnected(e.into())).is_err() {
                        return Ok(());
                    }
                }
            }

            sleep(Duration::from_secs(10)).await;
            info!("Restarting ZMQ listener");
        }
    })
}

pub async fn run(
    config: Config,
    cancel_token: CancellationToken,
    bitcoin: bitcoin_client::Client,
    initial_mempool_cache: HashSet<Txid>,
    tx: UnboundedSender<Event>,
) -> JoinHandle<()> {
    let (zmq_tx, mut zmq_rx) = mpsc::unbounded_channel::<ZmqEvent>();
    let runner_cancel_token = CancellationToken::new();
    let runner_handle = zmq_runner(
        config.clone(),
        runner_cancel_token.clone(),
        bitcoin.clone(),
        zmq_tx,
    )
    .await;

    info!(
        "Initializing reconciler with mempool cache: {}",
        initial_mempool_cache.len()
    );

    tokio::spawn(async move {
        let mut mempool_cache = initial_mempool_cache;
        let mut handle_zmq_event = async |event: ZmqEvent| -> Vec<Event> {
            match event {
                ZmqEvent::Connected => {
                    info!(
                        "Connected to Bitcoin ZMQ @ {}",
                        config.zmq_pub_sequence_address
                    );
                    vec![]
                }
                ZmqEvent::MempoolTransactions(txs) => {
                    let mut txid_to_transaction: HashMap<Txid, Transaction> =
                        txs.into_iter().map(|tx| (tx.compute_txid(), tx)).collect();
                    let txids: HashSet<Txid> = txid_to_transaction.keys().cloned().collect();
                    let removed: Vec<Txid> = mempool_cache.difference(&txids).cloned().collect();
                    let added: Vec<Transaction> = txids
                        .difference(&mempool_cache)
                        .map(|txid| txid_to_transaction.remove(txid).expect("Txid should exist"))
                        .collect();
                    mempool_cache = txids;
                    vec![Event::MempoolUpdates { added, removed }]
                }
                ZmqEvent::SequenceMessage(SequenceMessage::TransactionAdded { txid, .. }) => {
                    if mempool_cache.insert(txid) {
                        match retry(
                            || bitcoin.get_raw_transaction(&txid),
                            "get raw transaction",
                            new_backoff_limited(),
                            cancel_token.clone(),
                        )
                        .await
                        {
                            Ok(t) => vec![Event::MempoolUpdates {
                                added: vec![t],
                                removed: vec![],
                            }],
                            Err(e) => {
                                warn!(
                                    "Skipping adding mempool transaction due to get error: {}",
                                    e
                                );
                                vec![]
                            }
                        }
                    } else {
                        vec![]
                    }
                }
                ZmqEvent::SequenceMessage(SequenceMessage::TransactionRemoved { txid, .. }) => {
                    if mempool_cache.remove(&txid) {
                        vec![Event::MempoolUpdates {
                            added: vec![],
                            removed: vec![txid],
                        }]
                    } else {
                        vec![]
                    }
                }
                ZmqEvent::BlockConnected(block) => {
                    let mut removed = vec![];
                    for t in block.txdata.iter() {
                        let txid = t.compute_txid();
                        if mempool_cache.remove(&txid) {
                            removed.push(txid);
                        }
                    }
                    vec![
                        Event::MempoolUpdates {
                            added: vec![],
                            removed,
                        },
                        Event::Block(block),
                    ]
                }
                ZmqEvent::SequenceMessage(SequenceMessage::BlockDisconnected(block_hash)) => {
                    vec![Event::Rollback(block_hash)]
                }
                _ => vec![],
            }
        };

        loop {
            select! {
                option_zmq_event = zmq_rx.recv() => {
                    match option_zmq_event {
                        Some(zmq_event) => {
                            for event in handle_zmq_event(zmq_event).await {
                                if tx.send(event).is_err() {
                                    error!("Send channel closed, exiting");
                                    break;
                                }
                            }
                        },
                        None => {
                            // Occurs when runner fails to start up and drops channel sender
                            info!("Received None event, exiting");
                            break;
                        },
                    }
                },
                _ = cancel_token.cancelled() => {
                    info!("Cancelled");
                    break;
                }
            }
        }

        runner_cancel_token.cancel();
        match runner_handle.await {
            Err(_) => error!("ZMQ runner panicked on join"),
            Ok(Err(e)) => error!("ZMQ runner failed to start with error: {}", e),
            Ok(Ok(_)) => (),
        }

        info!("Exited");
    })
}

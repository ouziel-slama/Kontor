use std::{collections::HashSet, time::Duration};

use anyhow::Result;
use bitcoin::Txid;
use tokio::{
    select,
    sync::mpsc::{self, UnboundedSender},
    task::JoinHandle,
    time::sleep,
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::{bitcoin_client, config::Config};

use super::{event::ZmqEvent, zmq};

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
    mempool_cache: HashSet<Txid>,
) -> JoinHandle<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<ZmqEvent>();
    let runner_cancel_token = CancellationToken::new();
    let runner_handle = zmq_runner(
        config.clone(),
        runner_cancel_token.clone(),
        bitcoin.clone(),
        tx,
    )
    .await;

    info!("Initializing with mempool cache: {}", mempool_cache.len());

    tokio::spawn(async move {
        let handle_zmq_event = async |event: ZmqEvent| -> Result<()> {
            match event {
                ZmqEvent::Connected => info!(
                    "Connected to Bitcoin ZMQ @ {}",
                    config.zmq_pub_sequence_address
                ),
                ZmqEvent::MempoolTransactions(txs) => info!("Mempool transactions: {}", txs.len()),
                _ => info!("{}", event),
            }
            Ok(())
        };

        loop {
            select! {
                option_zmq_event = rx.recv() => {
                    match option_zmq_event {
                        Some(event) => {
                            if let Err(e) = handle_zmq_event(event).await {
                                error!("Failed to handle event: {}", e);
                                break;
                            };
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

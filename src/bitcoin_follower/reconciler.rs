use std::{collections::HashSet, time::Duration};

use anyhow::Result;
use tokio::{
    select,
    sync::mpsc::{self, UnboundedSender},
    task::JoinHandle,
    time::sleep,
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::{
    bitcoin_client,
    config::Config,
    retry::{new_backoff_unlimited, retry},
};

use super::{event::FollowEvent, zmq};

async fn zmq_runner(
    config: Config,
    cancel_token: CancellationToken,
    bitcoin: bitcoin_client::Client,
    tx: UnboundedSender<FollowEvent>,
) -> JoinHandle<Result<()>> {
    tokio::spawn(async move {
        loop {
            if cancel_token.is_cancelled() {
                return Ok(());
            }

            let mempool_cache = HashSet::from_iter(
                retry(
                    || bitcoin.get_raw_mempool(),
                    "get raw mempool",
                    new_backoff_unlimited(),
                    cancel_token.clone(),
                )
                .await?
                .into_iter(),
            );
            let handle = zmq::run(
                config.clone(),
                cancel_token.clone(),
                bitcoin.clone(),
                mempool_cache.clone(),
                tx.clone(),
            )
            .await?;

            match handle.await {
                Ok(Ok(_)) => return Ok(()),
                Ok(Err(e)) => {
                    error!("ZMQ listener exited with error: {}", e);
                    if tx.send(FollowEvent::ZmqDisconnected(e)).is_err() {
                        return Ok(());
                    }
                }
                Err(e) => {
                    error!("ZMQ listener panicked on join");
                    if tx.send(FollowEvent::ZmqDisconnected(e.into())).is_err() {
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
) -> JoinHandle<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<FollowEvent>();
    let runner_cancel_token = CancellationToken::new();
    let runner_handle = zmq_runner(
        config.clone(),
        runner_cancel_token.clone(),
        bitcoin.clone(),
        tx,
    )
    .await;
    tokio::spawn(async move {
        loop {
            select! {
                option_follow_event = rx.recv() => {
                    match option_follow_event {
                        Some(event) => {
                            info!("{}", event);
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

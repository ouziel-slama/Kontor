use std::collections::HashSet;

use anyhow::Result;
use kontor::{bitcoin_client, bitcoin_follower, config::Config, logging, stopper};
use tokio::{select, sync::mpsc};
use tokio_util::sync::CancellationToken;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    logging::setup();
    info!("Hello, World!");
    let config = Config::load()?;
    let bitcoin = bitcoin_client::Client::new_from_config(config.clone())?;
    let cancel_token = CancellationToken::new();
    let stopper_handle = stopper::run(cancel_token.clone());
    let (tx, mut rx) = mpsc::unbounded_channel();
    let reconciler_handle = tokio::spawn({
        let cancel_token = cancel_token.clone();
        async move {
            let _ = bitcoin_follower::reconciler::run(
                config.clone(),
                cancel_token.clone(),
                bitcoin.clone(),
                HashSet::new(),
                tx,
            )
            .await
            .await;
            cancel_token.cancel();
        }
    });
    let consumer_handle = tokio::spawn({
        let cancel_token = cancel_token.clone();
        async move {
            loop {
                select! {
                    _ = cancel_token.cancelled() => {
                        info!("Consumer cancelled");
                        break;
                    }
                    option_event = rx.recv() => {
                        match option_event {
                            Some(event) => {
                                info!("Event: {}", event);
                            },
                            None => {
                                info!("Consumer received None event, exiting");
                                break;
                            },
                        }
                    }
                }
            }
            info!("Consumer exiting");
        }
    });
    let _ = consumer_handle.await;
    let _ = reconciler_handle.await;
    let _ = stopper_handle.await;
    info!("Goodbye.");
    Ok(())
}

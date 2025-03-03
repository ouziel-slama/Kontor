use anyhow::Result;
use kontor::{bitcoin_client, bitcoin_follower, config::Config, logging, stopper};
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
    let reconciler_handle = tokio::spawn(async move {
        let _ = bitcoin_follower::reconciler::run(
            config.clone(),
            cancel_token.clone(),
            bitcoin.clone(),
        )
        .await
        .await;
        cancel_token.cancel();
    });
    let _ = reconciler_handle.await;
    let _ = stopper_handle.await;
    info!("Goodbye.");
    Ok(())
}

use anyhow::Result;
use kontor::{bitcoin_client, config::Config, follower, logging, stopper};
use tokio_util::sync::CancellationToken;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    logging::setup();
    info!("Hello, World!");
    let config = Config::load()?;
    let client = bitcoin_client::Client::new_from_config(config.clone())?;
    let cancel_token = CancellationToken::new();
    let zmq_handle =
        follower::zmq::run(config.clone(), cancel_token.clone(), client.clone()).await?;
    let stopper_handle = stopper::run(cancel_token.clone());
    zmq_handle.await??;
    stopper_handle.await?;
    Ok(())
}

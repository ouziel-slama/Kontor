use anyhow::Result;
use kontor::{logging, stopper};
use tokio::{select, task};
use tokio_util::sync::CancellationToken;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    logging::setup();
    info!("Hello, World!");
    let cancel_token = CancellationToken::new();
    let cancel_token_clone = cancel_token.clone();
    let task_handle = task::spawn(async move {
        info!("Task started");
        select! {
            _ = cancel_token_clone.cancelled() => {
                info!("Task cancelled");
            }
        }
        info!("Exiting task");
    });
    let stopper_handle = stopper::run(cancel_token);
    for handle in [task_handle, stopper_handle] {
        handle.await?
    }
    info!("Goodbye.");
    Ok(())
}

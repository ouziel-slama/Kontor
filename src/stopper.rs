use tokio::{
    select,
    signal::{
        ctrl_c,
        unix::{SignalKind, signal},
    },
    task::{self, JoinHandle},
};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

pub fn run(cancel_token: CancellationToken) -> JoinHandle<()> {
    task::spawn(async move {
        let mut sigterm = signal(SignalKind::terminate()).expect("Failed to listen for SIGTERM");
        select! {
            _ = cancel_token.cancelled() => warn!("Cancelled"),
            _ = ctrl_c() => warn!("Ctrl+C received"),
            _ = sigterm.recv() => warn!("SIGTERM received"),
        };
        info!("Initiating shutdown");
        cancel_token.cancel();
        info!("Exited");
    })
}

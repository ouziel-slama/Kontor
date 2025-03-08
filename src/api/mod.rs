pub mod router;

use std::{net::SocketAddr, time::Duration};

use anyhow::Result;
use axum_server::{Handle, tls_rustls::RustlsConfig};
use router::State;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

pub async fn run(cancel_token: CancellationToken, state: State) -> Result<JoinHandle<()>> {
    let config = RustlsConfig::from_pem_file(
        state.config.cert_dir.join("cert.pem"),
        state.config.cert_dir.join("key.pem"),
    )
    .await?;
    let addr = SocketAddr::from(([127, 0, 0, 1], state.config.api_port));
    let handle = Handle::new();
    tokio::spawn({
        let handle = handle.clone();
        async move {
            cancel_token.cancelled().await;
            handle.graceful_shutdown(Some(Duration::from_secs(10)));
        }
    });
    info!("API Server running @ https://{}", addr);
    Ok(tokio::spawn(async move {
        if axum_server::bind_rustls(addr, config)
            .handle(handle)
            .serve(router::new(state).into_make_service())
            .await
            .is_err()
        {
            error!("Panicked on join");
        }

        info!("Exited");
    }))
}

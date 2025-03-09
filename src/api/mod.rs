pub mod context;
pub mod error;
pub mod handlers;
pub mod response;
pub mod router;

use std::{net::SocketAddr, time::Duration};

use anyhow::Result;
use axum_server::{Handle, tls_rustls::RustlsConfig};
pub use context::Context;
use tokio::task::JoinHandle;
use tracing::{error, info};

pub async fn run(context: Context) -> Result<JoinHandle<()>> {
    let config = RustlsConfig::from_pem_file(
        context.config.cert_dir.join("cert.pem"),
        context.config.cert_dir.join("key.pem"),
    )
    .await?;
    let addr = SocketAddr::from(([127, 0, 0, 1], context.config.api_port));
    let handle = Handle::new();
    tokio::spawn({
        let handle = handle.clone();
        let cancel_token = context.cancel_token.clone();
        async move {
            cancel_token.cancelled().await;
            handle.graceful_shutdown(Some(Duration::from_secs(10)));
        }
    });
    info!("Server running @ https://{}", addr);
    Ok(tokio::spawn(async move {
        if axum_server::bind_rustls(addr, config)
            .handle(handle)
            .serve(router::new(context).into_make_service())
            .await
            .is_err()
        {
            error!("Panicked on join");
        }

        info!("Exited");
    }))
}

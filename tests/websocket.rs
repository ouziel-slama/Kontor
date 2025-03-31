use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use kontor::{
    api::{self, Env},
    config::Config,
    logging,
    reactor::events::EventSubscriber,
    utils::new_test_db,
};
use tokio::sync::mpsc;
use tokio_tungstenite::{Connector, connect_async_tls_with_config, tungstenite::Message};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn test_websocket_server() -> Result<()> {
    logging::setup();
    let cancel_token = CancellationToken::new();
    let config = Config::try_parse()?;
    let (reader, _writer, _temp_dir) = new_test_db().await?;
    let (_, event_rx) = mpsc::channel(10);
    let event_subscriber = EventSubscriber::new();
    let mut handles = vec![];
    handles.push(event_subscriber.run(cancel_token.clone(), event_rx));
    handles.push(
        api::run(Env {
            config: config.clone(),
            cancel_token: cancel_token.clone(),
            reader: reader.clone(),
            event_subscriber,
        })
        .await?,
    );

    let url = format!("wss://localhost:{}/ws", config.api_port);
    let certs = rustls_native_certs::load_native_certs().unwrap();
    let mut root_store = rustls::RootCertStore::empty();
    for cert in certs {
        root_store.add(cert)?;
    }
    let connector = Connector::Rustls(Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth(),
    ));
    let (mut ws_stream, _) =
        connect_async_tls_with_config(url, None, false, Some(connector)).await?;

    let ping_data = vec![1, 2, 3];
    ws_stream
        .send(Message::Ping(ping_data.clone().into()))
        .await?;
    let received = ws_stream.next().await.unwrap()?;
    assert_eq!(received, Message::Pong(ping_data.into()));

    ws_stream.send(Message::Close(None)).await?;
    let close_msg = ws_stream.next().await.unwrap()?;
    assert!(close_msg.is_close());
    cancel_token.cancel();
    for handle in handles {
        handle.await?;
    }
    Ok(())
}

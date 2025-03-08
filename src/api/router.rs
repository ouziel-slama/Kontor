use axum::Router;
use tokio_util::sync::CancellationToken;
use tower_http::trace::TraceLayer;

use crate::{config::Config, database};

#[derive(Clone)]
pub struct State {
    pub config: Config,
    pub cancel_token: CancellationToken,
    pub reader: database::Reader,
}

pub fn new(state: State) -> Router {
    Router::new()
        .nest(
            "/api",
            Router::new()
                .route("/", axum::routing::get(|| async { "Hello, world!" }))
                .route("/health", axum::routing::get(|| async { "OK" })),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

use std::time::Duration;

use axum::{
    Router,
    http::{HeaderName, Request, Response},
    routing::get,
};
use tower::ServiceBuilder;
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, RequestId, SetRequestIdLayer},
    trace::{MakeSpan, OnResponse, TraceLayer},
};
use tracing::Level;

use super::{context::Context, handlers::get_block};

#[derive(Clone)]
struct CustomMakeSpan;
impl<B> MakeSpan<B> for CustomMakeSpan {
    fn make_span(&mut self, req: &Request<B>) -> tracing::Span {
        let id = req
            .extensions()
            .get::<RequestId>()
            .and_then(|id| id.header_value().to_str().ok())
            .unwrap_or("unknown");
        tracing::span!(
            Level::INFO,
            "request",
            id = %id,
            method = %req.method(),
            path = %req.uri().path(), // Just the path, not full URL
            version = ?req.version() // HTTP/2.0, etc.
        )
    }
}

#[derive(Clone)]
struct CustomOnResponse;
impl<B> OnResponse<B> for CustomOnResponse {
    fn on_response(self, res: &Response<B>, latency: Duration, _: &tracing::Span) {
        tracing::info!("{} {}ms", res.status(), latency.as_millis());
    }
}

pub fn new(context: Context) -> Router {
    let x_request_id = HeaderName::from_static("x-request-id");

    Router::new()
        .nest(
            "/api",
            Router::new().route("/block/{height}", get(get_block)),
        )
        .layer(
            ServiceBuilder::new()
                .layer(SetRequestIdLayer::new(
                    x_request_id.clone(),
                    MakeRequestUuid,
                ))
                .layer(
                    TraceLayer::new_for_http()
                        .make_span_with(CustomMakeSpan)
                        .on_response(CustomOnResponse),
                )
                .layer(PropagateRequestIdLayer::new(x_request_id)),
        )
        .with_state(context)
}

mod handler;

use axum::{
    routing::{get, post},
    Router as AxumRouter,
};
use handler::*;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::oneshot;
use tower_http::cors::{Any, CorsLayer};
use crate::state::AppState;

pub async fn start_proxy_server(
    port: u16,
    state: Arc<AppState>,
    shutdown_rx: oneshot::Receiver<()>,
) -> anyhow::Result<()> {
    let app = AxumRouter::new()
        // OpenAI-compatible endpoints
        .route("/v1/chat/completions", post(handle_chat_completions))
        // OpenAI Responses API (new)
        .route("/v1/responses", post(handle_responses))
        .route("/v1/models", get(handle_list_models))
        .route("/v1/embeddings", post(handle_embeddings))
        // Claude-compatible endpoint
        .route("/v1/messages", post(handle_claude_messages))
        // Health check
        .route("/health", get(|| async { "OK" }))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!("Proxy server listening on {}", addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
            tracing::info!("Proxy server shutting down");
        })
        .await?;

    Ok(())
}

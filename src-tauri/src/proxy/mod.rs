mod handler;

use axum::{
    extract::State,
    routing::{get, post},
    Json, Router as AxumRouter,
};
use handler::*;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::oneshot;
use tower_http::cors::{Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::request_id::{MakeRequestId, PropagateRequestIdLayer, RequestId, SetRequestIdLayer};
use tower_http::trace::TraceLayer;
use crate::state::AppState;

#[derive(Clone)]
struct MakeRequestUuid;

impl MakeRequestId for MakeRequestUuid {
    fn make_request_id<B>(&mut self, _req: &axum::http::Request<B>) -> Option<RequestId> {
        let id = uuid::Uuid::new_v4().to_string();
        // SAFETY: UUID v4 is always valid ASCII, hence valid HeaderValue
        Some(RequestId::new(id.parse().expect("UUID parse failed")))
    }
}

/// 健康检查响应
async fn health_check(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let providers = state.get_providers();
    let active_providers = providers.iter().filter(|p| p.is_active).count();
    let total_providers = providers.len();

    let groups = state.get_groups();
    let active_groups = groups.iter().filter(|g| g.is_active).count();

    Json(serde_json::json!({
        "status": "ok",
        "providers": {
            "total": total_providers,
            "active": active_providers,
        },
        "groups": {
            "active": active_groups,
        },
    }))
}

pub async fn start_proxy_server(
    port: u16,
    state: Arc<AppState>,
    shutdown_rx: oneshot::Receiver<()>,
) -> anyhow::Result<()> {
    // 10MB request body limit
    const BODY_LIMIT: usize = 10 * 1024 * 1024;

    let app = AxumRouter::new()
        // OpenAI-compatible endpoints
        .route("/v1/chat/completions", post(handle_chat_completions))
        // OpenAI Responses API (new)
        .route("/v1/responses", post(handle_responses))
        .route("/v1/models", get(handle_list_models))
        .route("/v1/embeddings", post(handle_embeddings))
        // Claude-compatible endpoint
        .route("/v1/messages", post(handle_claude_messages))
        // Health check (structured JSON)
        .route("/health", get(health_check))
        // Request body size limit (10MB)
        .layer(RequestBodyLimitLayer::new(BODY_LIMIT))
        // Generate x-request-id for each request
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(PropagateRequestIdLayer::x_request_id())
        // Request tracing (method, uri, status, latency)
        .layer(TraceLayer::new_for_http())
        // CORS
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state.clone());

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!("Proxy server listening on {}", addr);

    // Auto-takeover CLI tools on proxy start
    let proxy_url = format!("http://127.0.0.1:{}/v1", port);
    let cli_mgr = state.cli_config_manager.clone();
    let settings = cli_mgr.get_global_settings();
    if settings.auto_takeover_on_start {
        match state.db.load_all_cli_tool_configs() {
            Ok(configs) => {
                let results = cli_mgr.takeover_all_enabled(&proxy_url, &configs);
                for r in &results {
                    if r.success {
                        tracing::info!("CLI auto-takeover: {}", r.message);
                    } else {
                        tracing::warn!("CLI auto-takeover failed: {}", r.message);
                    }
                }
            }
            Err(e) => tracing::warn!("Failed to load CLI configs for auto-takeover: {e}"),
        }
    }

    let cli_mgr2 = state.cli_config_manager.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = shutdown_rx.await;
            tracing::info!("Proxy server shutting down");

            // Auto-restore CLI tools on proxy stop
            let settings = cli_mgr2.get_global_settings();
            if settings.auto_restore_on_stop {
                let results = cli_mgr2.restore_all();
                for r in &results {
                    if r.success {
                        tracing::info!("CLI auto-restore: {}", r.message);
                    }
                }
            }
        })
        .await?;

    Ok(())
}

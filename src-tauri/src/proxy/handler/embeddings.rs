use axum::{
    extract::{Request, State},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;

use crate::models::{EmbeddingRequest, RequestLog};
use crate::router::Router;
use crate::state::AppState;

/// Handle embeddings request
pub async fn handle_embeddings(
    State(state): State<Arc<AppState>>,
    request: Request,
) -> Response {
    tracing::info!("收到 Embeddings 请求");
    let start_time = Instant::now();
    let mut log_entry = RequestLog::new("POST".to_string(), "/v1/embeddings".to_string())
        .with_endpoint_type("embeddings".to_string());

    let body = match axum::body::to_bytes(request.into_body(), 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            log_entry = log_entry
                .with_status(400)
                .with_error(format!("Failed to read request body: {}", e));
            state.save_request_log(&log_entry);
            return Json(json!({
                "error": {
                    "message": format!("Failed to read request body: {}", e),
                    "type": "invalid_request_error"
                }
            }))
            .into_response();
        }
    };

    let request_body_str = String::from_utf8_lossy(&body).to_string();
    log_entry = log_entry.with_request(request_body_str.clone());

    let embedding_request: EmbeddingRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            log_entry = log_entry
                .with_status(400)
                .with_error(format!("Invalid request format: {}", e));
            state.save_request_log(&log_entry);
            return Json(json!({
                "error": {
                    "message": format!("Invalid request format: {}", e),
                    "type": "invalid_request_error"
                }
            }))
            .into_response();
        }
    };

    let requested_model = embedding_request.model.clone();
    log_entry = log_entry.with_requested_model(requested_model.clone());

    // 构建 OpenAI 格式的 embedding 请求转发给 provider
    let _provider_request = json!({
        "model": embedding_request.model,
        "input": embedding_request.input,
    });

    let router = Router::new(Arc::clone(&state));
    let route_result = router.route_embedding(embedding_request.clone()).await;

    let info = route_result.info.clone();

    if let Some(model) = &info.actual_model {
        log_entry = log_entry.with_model(model.clone());
    }
    if let Some(name) = &info.provider_name {
        if let Some(prefix) = &info.provider_prefix {
            log_entry = log_entry.with_provider(name.clone(), prefix.clone());
        }
    }
    if let Some(url) = &info.actual_url {
        log_entry = log_entry.with_url(url.clone());
    }
    if let Some(transform) = &info.protocol_transform {
        log_entry = log_entry.with_protocol_transform(transform.clone());
    }

    match (route_result.response, route_result.error) {
        (Some(response), None) => {
            let status = response.status();
            let body_bytes = match response.bytes().await {
                Ok(b) => b,
                Err(e) => {
                    log_entry = log_entry
                        .with_status(500)
                        .with_error(format!("读取响应失败: {}", e));
                    state.save_request_log(&log_entry);
                    return Json(json!({
                        "error": {
                            "message": format!("Failed to read response: {}", e),
                            "type": "internal_error"
                        }
                    }))
                    .into_response();
                }
            };

            let response_json: serde_json::Value = match serde_json::from_slice(&body_bytes) {
                Ok(v) => v,
                Err(_) => {
                    let body_text = String::from_utf8_lossy(&body_bytes);
                    log_entry = log_entry
                        .with_status(200)
                        .with_response(body_text.to_string());
                    state.save_request_log(&log_entry);
                    return Json(json!({ "raw_response": body_text })).into_response();
                }
            };

            let latency = start_time.elapsed().as_millis() as i64;
            log_entry = log_entry
                .with_status(status.as_u16() as i32)
                .with_latency(latency)
                .with_response(serde_json::to_string(&response_json).unwrap_or_default());

            if let Some(usage) = response_json.get("usage") {
                if let Some(prompt) = usage.get("prompt_tokens").and_then(|v| v.as_u64()) {
                    log_entry = log_entry.with_tokens(prompt as i32, 0);
                }
            }

            state.save_request_log(&log_entry);

            Json(response_json).into_response()
        }
        (None, Some(error_msg)) => {
            let latency = start_time.elapsed().as_millis() as i64;
            log_entry = log_entry
                .with_status(500)
                .with_latency(latency)
                .with_error(error_msg.clone());
            state.save_request_log(&log_entry);

            Json(json!({
                "error": {
                    "message": error_msg,
                    "type": "routing_error"
                }
            }))
            .into_response()
        }
        _ => {
            let latency = start_time.elapsed().as_millis() as i64;
            log_entry = log_entry
                .with_status(500)
                .with_latency(latency)
                .with_error("Unknown error".to_string());
            state.save_request_log(&log_entry);

            Json(json!({
                "error": {
                    "message": "Unknown error",
                    "type": "internal_error"
                }
            }))
            .into_response()
        }
    }
}

/// Handle list models request
pub async fn handle_list_models(State(state): State<Arc<AppState>>) -> Response {
    // 获取所有 Provider 中配置的模型
    let providers = state.get_providers();
    let mut seen_models = std::collections::HashSet::new();
    let mut models: Vec<serde_json::Value> = Vec::new();

    for provider in providers.iter().filter(|p| p.is_active) {
        let prefix = &provider.prefix;
        for mc in &provider.models {
            // 构建完整模型名：prefix/model_name
            let full_name = if mc.name == "*" {
                // 通配符模型，跳过
                continue;
            } else if mc.name.contains('/') {
                // 已有前缀，直接使用
                mc.name.clone()
            } else {
                format!("{}/{}", prefix, mc.name)
            };

            // 去重
            if seen_models.insert(full_name.clone()) {
                models.push(json!({
                    "id": full_name,
                    "object": "model",
                    "owned_by": provider.name,
                    "permission": [],
                    "root": provider.name,
                    "provider": provider.name
                }));
            }
        }
    }

    // 按 id 排序
    models.sort_by(|a, b| {
        a.get("id").and_then(|v| v.as_str()).unwrap_or("")
            .cmp(b.get("id").and_then(|v| v.as_str()).unwrap_or(""))
    });

    Json(json!({
        "object": "list",
        "data": models
    }))
    .into_response()
}

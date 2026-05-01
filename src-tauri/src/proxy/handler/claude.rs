use axum::{
    body::Body,
    extract::{Request, State},
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use std::time::Instant;

use crate::models::RequestLog;
use crate::router::Router;
use crate::state::AppState;

use super::common::{is_sse_response, SseUsageCollector, create_logged_passthrough_stream, claude_error};

/// Handle Claude-compatible messages
/// 直连模式：直接转发原始请求到上游的 /v1/messages 端点，不做格式转换
pub async fn handle_claude_messages(
    State(state): State<Arc<AppState>>,
    request: Request,
) -> Response {
    tracing::info!("收到 Claude Messages 请求");
    let start_time = Instant::now();
    let mut log_entry = RequestLog::new("POST".to_string(), "/v1/messages".to_string())
        .with_endpoint_type("claude".to_string());

    let body = match axum::body::to_bytes(request.into_body(), 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            log_entry = log_entry
                .with_status(400)
                .with_error(format!("Failed to read request body: {}", e));
            state.save_request_log(&log_entry);
            return claude_error(400, format!("Failed to read request body: {}", e));
        }
    };

    let request_body_str = String::from_utf8_lossy(&body).to_string();
    // 保存原始请求（客户端发送的）
    log_entry = log_entry.with_original_request(request_body_str.clone());

    // 直接解析为 JSON Value
    let raw_body: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            log_entry = log_entry
                .with_status(400)
                .with_error(format!("Invalid JSON format: {}", e));
            state.save_request_log(&log_entry);
            return claude_error(400, format!("Invalid JSON format: {}", e));
        }
    };

    let requested_model = raw_body.get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    log_entry = log_entry.with_requested_model(requested_model.clone());

    // ===== 直连模式：直接转发原始请求 =====
    // 不做格式转换，直接路由原始 JSON 请求
    let router = Router::new(Arc::clone(&state));
    let route_result = router.route_claude_messages_raw(&raw_body).await;

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

    // 保存实际发送的请求体
    if let Some(ref actual_body) = route_result.actual_request_body {
        if let Ok(body_str) = serde_json::to_string(actual_body) {
            log_entry = log_entry.with_request(body_str);
        }
    }

    match (route_result.response, route_result.error) {
        (Some(response), None) => {
            let status = response.status();

            if is_sse_response(&response) {
                // 流式响应：透传 SSE 流并收集 usage
                tracing::info!(
                    "Claude Messages 流式响应: requested='{}', actual='{}', provider='{}'",
                    info.requested_model,
                    info.actual_model.as_deref().unwrap_or("-"),
                    info.provider_name.as_deref().unwrap_or("-")
                );

                let mut builder = Response::builder().status(status.as_u16());

                // 复制响应头
                for (key, value) in response.headers() {
                    let key_str = key.as_str();
                    if !matches!(key_str, "content-encoding" | "content-length" | "transfer-encoding") {
                        builder = builder.header(key, value);
                    }
                }

                // 创建使用量收集器（支持 OpenAI 和 Claude 格式）
                let state_clone = Arc::clone(&state);
                let log_entry_base = log_entry.clone();
                let model_for_cost = info.actual_model.clone();
                let provider_for_cost = info.provider_prefix.as_ref()
                    .and_then(|prefix| state.get_provider_by_prefix(prefix));
                let provider_prefix_for_cost = provider_for_cost.as_ref().map(|p| p.prefix.clone());
                let pricing_manager = Arc::clone(&state.pricing_manager);
                let collector = Arc::new(SseUsageCollector::new(start_time, move |events, latency_ms, first_token_ms, prompt_tokens, completion_tokens, _full_content| {
                    let mut final_log = log_entry_base.clone();
                    final_log = final_log
                        .with_status(200)
                        .with_latency(latency_ms);

                    if let Some(ft) = first_token_ms {
                        final_log = final_log.with_first_token(ft);
                    }

                    if prompt_tokens > 0 || completion_tokens > 0 {
                        final_log = final_log.with_tokens(prompt_tokens, completion_tokens);

                        if let (Some(model), Some(ref provider)) = (&model_for_cost, &provider_for_cost) {
                            let prefix_ref = provider_prefix_for_cost.as_deref();
                            if let Some(cost) = super::common::calculate_request_cost(
                                provider, &pricing_manager, model, prefix_ref, prompt_tokens, completion_tokens,
                            ) {
                                final_log = final_log.with_cost(cost);
                            }
                        }
                    }

                    // 保存最后的 SSE 事件作为响应记录
                    if let Some(last_event) = events.last() {
                        if let Ok(response_str) = serde_json::to_string(last_event) {
                            final_log = final_log.with_response(response_str);
                        }
                    }

                    state_clone.save_request_log(&final_log);
                }));

                // 创建带日志的流
                let stream = response.bytes_stream();
                let logged_stream = create_logged_passthrough_stream(stream, collector);
                let body = Body::from_stream(logged_stream);

                match builder.body(body) {
                    Ok(resp) => resp,
                    Err(e) => {
                        log_entry = log_entry
                            .with_status(500)
                            .with_error(format!("构建流式响应失败: {}", e));
                        state.save_request_log(&log_entry);
                        claude_error(500, format!("Failed to build streaming response: {}", e))
                    }
                }
            } else {
                // 非流式响应：直接返回
                let body_bytes = match response.bytes().await {
                    Ok(b) => b,
                    Err(e) => {
                        log_entry = log_entry
                            .with_status(500)
                            .with_error(format!("读取响应失败: {}", e));
                        state.save_request_log(&log_entry);
                        return claude_error(500, format!("Failed to read response: {}", e));
                    }
                };

                let latency = start_time.elapsed().as_millis() as i64;
                log_entry = log_entry
                    .with_status(status.as_u16() as i32)
                    .with_latency(latency)
                    .with_response(String::from_utf8_lossy(&body_bytes).to_string());

                // 尝试从响应中提取 tokens
                if let Ok(response_json) = serde_json::from_slice::<serde_json::Value>(&body_bytes) {
                    if let Some(usage) = response_json.get("usage") {
                        // OpenAI 格式
                        if let Some(input) = usage.get("prompt_tokens").and_then(|v| v.as_i64()) {
                            if let Some(output) = usage.get("completion_tokens").and_then(|v| v.as_i64()) {
                                log_entry = log_entry.with_tokens(input as i32, output as i32);
                            }
                        }
                        // Claude 格式
                        if let Some(input) = usage.get("input_tokens").and_then(|v| v.as_i64()) {
                            if let Some(output) = usage.get("output_tokens").and_then(|v| v.as_i64()) {
                                log_entry = log_entry.with_tokens(input as i32, output as i32);
                            }
                        }
                    }
                }

                state.save_request_log(&log_entry);

                Response::builder()
                    .status(status.as_u16())
                    .header("content-type", "application/json")
                    .body(Body::from(body_bytes))
                    .unwrap_or_else(|_| Response::builder().status(500).body(Body::empty()).unwrap())
                    .into_response()
            }
        }
        (None, Some(error_msg)) => {
            let latency = start_time.elapsed().as_millis() as i64;
            log_entry = log_entry
                .with_status(500)
                .with_latency(latency)
                .with_error(error_msg.clone());
            state.save_request_log(&log_entry);
            claude_error(500, error_msg)
        }
        _ => {
            let latency = start_time.elapsed().as_millis() as i64;
            log_entry = log_entry
                .with_status(500)
                .with_latency(latency)
                .with_error("Unknown error".to_string());
            state.save_request_log(&log_entry);
            claude_error(500, "Unknown error")
        }
    }
}

use axum::{
    body::Body,
    extract::{Request, State},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;

use crate::models::{ChatRequest, RequestLog};
use crate::router::Router;
use crate::state::AppState;

use super::common::{is_sse_response, SseUsageCollector, create_logged_passthrough_stream};

/// Handle OpenAI-compatible chat completions
pub async fn handle_chat_completions(
    State(state): State<Arc<AppState>>,
    request: Request,
) -> Response {
    tracing::info!("收到聊天请求");
    let start_time = Instant::now();
    let mut log_entry = RequestLog::new("POST".to_string(), "/v1/chat/completions".to_string())
        .with_endpoint_type("chat".to_string());

    // 解析请求体
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

    // 保存原始请求（客户端发送的）
    let request_body_str = String::from_utf8_lossy(&body).to_string();
    log_entry = log_entry.with_original_request(request_body_str.clone());

    let chat_request: ChatRequest = match serde_json::from_slice(&body) {
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

    let requested_model = chat_request.model.clone();
    log_entry = log_entry.with_requested_model(requested_model.clone());

    // 创建路由器并路由请求
    let router = Router::new(Arc::clone(&state));
    let route_result = router.route_chat(chat_request).await;

    // 提取路由信息
    let info = route_result.info.clone();

    // 打印路由结果
    tracing::info!(
        "路由结果: provider={:?}, prefix={:?}, actual_model={:?}, error={:?}",
        info.provider_name, info.provider_prefix, info.actual_model, route_result.error
    );

    // 保存转换后的请求（发送给上游的）
    if let Some(ref actual_body) = route_result.actual_request_body {
        if let Ok(body_str) = serde_json::to_string(actual_body) {
            log_entry = log_entry.with_request(body_str);
        }
    }

    // 记录日志元数据
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

    // 获取 Provider 信息用于成本计算
    let provider_for_cost = info.provider_prefix.as_ref()
        .and_then(|prefix| state.get_provider_by_prefix(prefix));

    // 处理结果
    match (route_result.response, route_result.error) {
        (Some(response), None) => {
            let status = response.status();

            if is_sse_response(&response) {
                // 流式响应：透传 SSE 流并收集 usage
                tracing::info!(
                    "流式响应: requested='{}', actual='{}', provider='{}'",
                    info.requested_model,
                    info.actual_model.as_deref().unwrap_or("-"),
                    info.provider_name.as_deref().unwrap_or("-")
                );

                let mut builder = Response::builder().status(status.as_u16());

                // 复制响应头（排除可能导致问题的头）
                for (key, value) in response.headers() {
                    let key_str = key.as_str();
                    if !matches!(key_str, "content-encoding" | "content-length" | "transfer-encoding") {
                        builder = builder.header(key, value);
                    }
                }

                // 创建使用量收集器
                let state_clone = Arc::clone(&state);
                let log_entry_base = log_entry.clone();
                let model_for_cost = info.actual_model.clone();
                let model_for_response = info.requested_model.clone();
                let provider_for_cost_clone = provider_for_cost.clone();
                let provider_prefix_for_cost = provider_for_cost.as_ref().map(|p| p.prefix.clone());
                let pricing_manager = Arc::clone(&state.pricing_manager);
                let collector = Arc::new(SseUsageCollector::new(start_time, move |events, latency_ms, first_token_ms, prompt_tokens, completion_tokens, full_content| {
                    let mut final_log = log_entry_base.clone();
                    final_log = final_log
                        .with_status(200)
                        .with_latency(latency_ms);

                    if let Some(ft) = first_token_ms {
                        final_log = final_log.with_first_token(ft);
                    }

                    if prompt_tokens > 0 || completion_tokens > 0 {
                        final_log = final_log.with_tokens(prompt_tokens, completion_tokens);

                        if let (Some(model), Some(ref provider)) = (&model_for_cost, &provider_for_cost_clone) {
                            let prefix_ref = provider_prefix_for_cost.as_deref();
                            if let Some(cost) = super::common::calculate_request_cost(
                                provider, &pricing_manager, model, prefix_ref, prompt_tokens, completion_tokens,
                            ) {
                                final_log = final_log.with_cost(cost);
                            }
                        }
                    }

                    // 构建完整响应（包含累积的内容）
                    let response_json = serde_json::json!({
                        "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
                        "object": "chat.completion",
                        "created": chrono::Utc::now().timestamp(),
                        "model": model_for_response,
                        "choices": [{
                            "index": 0,
                            "message": {
                                "role": "assistant",
                                "content": full_content
                            },
                            "finish_reason": "stop"
                        }],
                        "usage": {
                            "prompt_tokens": prompt_tokens,
                            "completion_tokens": completion_tokens,
                            "total_tokens": prompt_tokens + completion_tokens
                        }
                    });

                    if let Ok(response_str) = serde_json::to_string(&response_json) {
                        final_log = final_log.with_response(response_str);
                    }

                    // 保存响应内容（可选：保存最后几个事件）
                    if let Some(last_event) = events.last() {
                        if let Ok(response_str) = serde_json::to_string(last_event) {
                            final_log = final_log.with_response(response_str);
                        }
                    }

                    state_clone.save_request_log(&final_log);

                    tracing::info!(
                        "流式请求完成: latency={}ms, first_token={:?}ms, tokens={}/{}, cost=${:.6}",
                        latency_ms,
                        first_token_ms,
                        prompt_tokens,
                        completion_tokens,
                        final_log.cost.unwrap_or(0.0)
                    );
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
                        Json(json!({
                            "error": {
                                "message": format!("Failed to build streaming response: {}", e),
                                "type": "internal_error"
                            }
                        }))
                        .into_response()
                    }
                }
            } else {
                // 非流式响应：读取并解析 JSON
                let latency = start_time.elapsed().as_millis() as i64;
                let body_bytes = match response.bytes().await {
                    Ok(b) => b,
                    Err(e) => {
                        log_entry = log_entry
                            .with_status(500)
                            .with_latency(latency)
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
                        // 如果不是 JSON，返回原始文本
                        let body_text = String::from_utf8_lossy(&body_bytes);
                        log_entry = log_entry
                            .with_status(200)
                            .with_latency(latency)
                            .with_response(body_text.to_string());
                        state.save_request_log(&log_entry);
                        return Json(json!({
                            "raw_response": body_text
                        }))
                        .into_response();
                    }
                };

                // 首先检查上游是否返回了错误
                if let Some(error) = response_json.get("error") {
                    tracing::warn!("上游返回错误: {:?}", error);
                    // 直接返回错误响应，保持原始错误信息
                    log_entry = log_entry
                        .with_status(status.as_u16() as i32)
                        .with_latency(latency)
                        .with_original_response(serde_json::to_string(&response_json).unwrap_or_default())
                        .with_response(serde_json::to_string(&response_json).unwrap_or_default())
                        .with_error(error.to_string());
                    state.save_request_log(&log_entry);

                    return Response::builder()
                        .status(status.as_u16())
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_string(&response_json).unwrap_or_default()))
                        .unwrap_or_else(|_| Response::builder().status(500).body(Body::empty()).unwrap())
                        .into_response();
                }

                // 检查上游是否返回了 Responses API 格式（需要转换为 Chat 格式）
                let (final_response, needs_conversion) = if response_json.get("output").is_some() && response_json.get("choices").is_none() {
                    tracing::info!("上游返回 Responses API 格式，转换为 Chat Completions 格式");
                    (crate::router::responses_to_chat_response(&response_json, &info.requested_model), true)
                } else {
                    // 确保响应中的模型名是用户请求的模型名
                    let mut result = response_json.clone();
                    if let Some(obj) = result.as_object_mut() {
                        obj.insert("model".to_string(), json!(info.requested_model));
                    }
                    (result, false)
                };

                // 记录日志：如果有协议转换，同时记录原始响应和转换后的响应
                log_entry = log_entry
                    .with_status(status.as_u16() as i32)
                    .with_latency(latency)
                    .with_response(serde_json::to_string(&final_response).unwrap_or_default());

                if needs_conversion {
                    log_entry = log_entry.with_original_response(serde_json::to_string(&response_json).unwrap_or_default());
                }

                // 尝试从响应中提取 token 使用量
                if let Some(usage) = final_response.get("usage") {
                    if let Some(prompt) = usage.get("prompt_tokens").and_then(|v| v.as_i64()) {
                        if let Some(completion) = usage.get("completion_tokens").and_then(|v| v.as_i64()) {
                            log_entry = log_entry.with_tokens(prompt as i32, completion as i32);

                            if let (Some(model), Some(ref provider)) = (&info.actual_model, &provider_for_cost) {
                                let prefix_ref = info.provider_prefix.as_deref();
                                if let Some(cost) = super::common::calculate_request_cost(
                                    provider, &state.pricing_manager, model, prefix_ref, prompt as i32, completion as i32,
                                ) {
                                    log_entry = log_entry.with_cost(cost);
                                }
                            }
                        } else {
                            tracing::warn!("响应中未找到 completion_tokens");
                        }
                    } else {
                        tracing::warn!("响应中未找到 prompt_tokens");
                    }
                } else {
                    tracing::warn!("响应中未找到 usage 字段");
                }

                state.save_request_log(&log_entry);
                tracing::info!(
                    "请求成功: requested='{}', actual='{}', provider='{}', cost=${:.6}",
                    info.requested_model,
                    info.actual_model.as_deref().unwrap_or("-"),
                    info.provider_name.as_deref().unwrap_or("-"),
                    log_entry.cost.unwrap_or(0.0)
                );

                Json(final_response).into_response()
            }
        }
        (None, Some(error_msg)) => {
            // 失败
            let latency = start_time.elapsed().as_millis() as i64;
            tracing::error!(
                "请求失败: requested='{}', actual='{}', provider='{}', error='{}'",
                info.requested_model,
                info.actual_model.as_deref().unwrap_or("-"),
                info.provider_name.as_deref().unwrap_or("-"),
                error_msg
            );

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
            // 未知状态
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

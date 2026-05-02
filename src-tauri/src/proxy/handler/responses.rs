use axum::{
    body::Body,
    extract::{Request, State},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures::StreamExt;
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;
use tokio::time::Duration;

use crate::models::RequestLog;
use crate::router::{Router, chat_to_responses_response};
use crate::state::AppState;

use super::common::{build_response, is_sse_response, strip_sse_field, SseUsageCollector, create_logged_passthrough_stream, responses_error};

/// Handle OpenAI Responses API requests
pub async fn handle_responses(
    State(state): State<Arc<AppState>>,
    request: Request,
) -> Response {
    tracing::info!("收到 Responses API 请求");
    let start_time = Instant::now();
    let mut log_entry = RequestLog::new("POST".to_string(), "/v1/responses".to_string())
        .with_endpoint_type("responses".to_string());

    let body = match axum::body::to_bytes(request.into_body(), 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            log_entry = log_entry
                .with_status(400)
                .with_error(format!("Failed to read request body: {}", e));
            state.save_request_log(&log_entry);
            return responses_error(400, format!("Failed to read request body: {}", e));
        }
    };

    let request_body_str = String::from_utf8_lossy(&body).to_string();
    // 保存原始请求（客户端发送的）
    log_entry = log_entry.with_original_request(request_body_str.clone());

    // 解析原始 JSON（保留所有字段）
    let raw_body: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            log_entry = log_entry
                .with_status(400)
                .with_error(format!("Invalid JSON format: {}", e));
            state.save_request_log(&log_entry);
            return responses_error(400, format!("Invalid JSON format: {}", e));
        }
    };

    let requested_model = raw_body.get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    log_entry = log_entry.with_requested_model(requested_model.clone());

    // 使用 Router 的 route_responses_raw 方法（保留原始请求体进行直连转发）
    let router = Router::new(Arc::clone(&state));
    let route_result = router.route_responses_raw(&raw_body).await;

    let info = route_result.info.clone();

    // 保存转换后的请求（发送给上游的）
    if let Some(ref actual_body) = route_result.actual_request_body {
        if let Ok(body_str) = serde_json::to_string(actual_body) {
            log_entry = log_entry.with_request(body_str);
        }
    }

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
    // 协议转换标签
    if let Some(transform) = &info.protocol_transform {
        if !transform.is_empty() {
            log_entry = log_entry.with_protocol_transform(transform.clone());
        }
    }
    if let Some(endpoint_type) = &info.endpoint_type {
        log_entry = log_entry.with_protocol_transform(format!("responses->{}", endpoint_type));
    }

    match (route_result.response, route_result.error) {
        (Some(response), None) => {
            let status = response.status();
            let is_stream = is_sse_response(&response);

            tracing::info!(
                "上游响应: status={}, is_stream={}, endpoint_type={:?}",
                status, is_stream, info.endpoint_type
            );

            // 直连 Responses API：直接透传响应
            if info.endpoint_type.as_deref() == Some("responses") {
                tracing::info!("直连 Responses API，直接透传响应");

                let mut builder = Response::builder().status(status.as_u16());

                // 复制响应头
                for (key, value) in response.headers() {
                    let key_str = key.as_str();
                    if !matches!(key_str, "content-encoding" | "content-length" | "transfer-encoding") {
                        builder = builder.header(key, value);
                    }
                }

                // 设置协议转换标签
                log_entry = log_entry.with_protocol_transform("direct".to_string());

                if is_stream {
                    // 流式响应：透传字节流，同时解析 SSE 事件提取 tokens
                    let state_clone = Arc::clone(&state);
                    let log_entry_base = log_entry.clone();
                    let start = start_time;

                    let byte_stream = response.bytes_stream();
                    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::convert::Infallible>>(100);

                    tokio::spawn(async move {
                        use futures::StreamExt;

                        let mut stream = byte_stream;
                        let mut prompt_tokens = 0i32;
                        let mut completion_tokens = 0i32;
                        let mut buffer = String::new();

                        while let Some(item) = stream.next().await {
                            match item {
                                Ok(bytes) => {
                                    // 解析 SSE 事件提取 tokens
                                    let chunk = String::from_utf8_lossy(&bytes);
                                    buffer.push_str(&chunk);

                                    // 解析 usage 信息
                                    for line in buffer.lines() {
                                        if let Some(data) = line.strip_prefix("data: ") {
                                            if data != "[DONE]" {
                                                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                                                    // 打印事件类型用于调试
                                                    if let Some(event_type) = json.get("type").and_then(|v| v.as_str()) {
                                                        tracing::debug!("SSE 事件类型: {}", event_type);
                                                    }

                                                    // Responses API 格式: response.completed 事件中的 response.usage
                                                    if let Some(response_obj) = json.get("response") {
                                                        if let Some(usage) = response_obj.get("usage") {
                                                            tracing::info!("找到 response.usage: {:?}", usage);
                                                            if let Some(input) = usage.get("input_tokens").and_then(|v| v.as_u64()) {
                                                                prompt_tokens = input as i32;
                                                            }
                                                            if let Some(output) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                                                                completion_tokens = output as i32;
                                                            }
                                                        }
                                                    }
                                                    // 直接在顶层的 usage（某些供应商可能使用）
                                                    if let Some(usage) = json.get("usage") {
                                                        tracing::info!("找到 usage: {:?}", usage);
                                                        // Responses API 格式
                                                        if let Some(input) = usage.get("input_tokens").and_then(|v| v.as_u64()) {
                                                            prompt_tokens = input as i32;
                                                        }
                                                        if let Some(output) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                                                            completion_tokens = output as i32;
                                                        }
                                                        // OpenAI Chat 格式
                                                        if let Some(input) = usage.get("prompt_tokens").and_then(|v| v.as_u64()) {
                                                            prompt_tokens = input as i32;
                                                        }
                                                        if let Some(output) = usage.get("completion_tokens").and_then(|v| v.as_u64()) {
                                                            completion_tokens = output as i32;
                                                        }
                                                    }
                                                    // delta 中的 usage
                                                    if let Some(delta) = json.get("delta") {
                                                        if let Some(usage) = delta.get("usage") {
                                                            tracing::info!("找到 delta.usage: {:?}", usage);
                                                            if let Some(input) = usage.get("input_tokens").and_then(|v| v.as_u64()) {
                                                                prompt_tokens = input as i32;
                                                            }
                                                            if let Some(output) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                                                                completion_tokens = output as i32;
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    // 清理已处理的 buffer（保留最后不完整的行）
                                    if let Some(pos) = buffer.rfind('\n') {
                                        buffer.drain(..pos + 1);
                                    }

                                    if tx.send(Ok(bytes)).await.is_err() {
                                        break;
                                    }
                                }
                                Err(e) => {
                                    tracing::error!("流式响应错误: {}", e);
                                    break;
                                }
                            }
                        }

                        let latency = start.elapsed().as_millis() as i64;
                        let mut final_log = log_entry_base.clone();
                        final_log = final_log.with_status(200).with_latency(latency);
                        if prompt_tokens > 0 || completion_tokens > 0 {
                            final_log = final_log.with_tokens(prompt_tokens, completion_tokens);
                        }
                        state_clone.save_request_log(&final_log);
                    });

                    use tokio_stream::wrappers::ReceiverStream;
                    return build_response(
                        builder
                            .header("content-type", "text/event-stream")
                            .header("cache-control", "no-cache")
                            .header("connection", "keep-alive"),
                        Body::from_stream(ReceiverStream::new(rx)),
                    )
                    .into_response();
                } else {
                    // 非流式响应：读取并解析 JSON 提取 tokens
                    let body_bytes = response.bytes().await.unwrap_or_default();
                    let latency = start_time.elapsed().as_millis() as i64;

                    // 解析响应提取 tokens
                    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&body_bytes) {
                        tracing::info!("直连 Responses 非流式响应: {}", serde_json::to_string(&json).unwrap_or_default().chars().take(500).collect::<String>());
                        if let Some(usage) = json.get("usage") {
                            // Responses API 格式
                            let mut input_tokens = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as i32;
                            let mut output_tokens = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as i32;
                            // OpenAI 格式（OpenRouter 可能使用）
                            if input_tokens == 0 {
                                input_tokens = usage.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as i32;
                            }
                            if output_tokens == 0 {
                                output_tokens = usage.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as i32;
                            }
                            tracing::info!("直连 Responses tokens: input={}, output={}", input_tokens, output_tokens);
                            log_entry = log_entry.with_tokens(input_tokens, output_tokens);
                        }
                    }

                    log_entry = log_entry.with_status(status.as_u16() as i32).with_latency(latency);
                    state.save_request_log(&log_entry);

                    return build_response(builder, Body::from(body_bytes))
                        .into_response();
                }
            }

            // 非直连（转换为 Chat 格式）：需要处理响应转换
            // 检查是否是流式响应
            if is_stream {
                tracing::info!("Responses API 流式响应被识别: provider='{}'", info.provider_name.as_deref().unwrap_or("-"));

                let mut builder = Response::builder().status(status.as_u16());

                // 复制响应头（排除可能导致问题的头）
                for (key, value) in response.headers() {
                    let key_str = key.as_str();
                    if !matches!(key_str, "content-encoding" | "content-length" | "transfer-encoding") {
                        builder = builder.header(key, value);
                    }
                }

                // 创建日志收集器（流结束后保存日志）
                // 注意：Responses API 流式请求会将 Chat 格式转换为 Responses 格式
                let state_clone = Arc::clone(&state);
                let log_entry_base = log_entry.clone();
                let model_for_response = requested_model.clone();
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
                    }

                    // 保存原始响应（Chat 格式）- 包含累积的内容
                    if !events.is_empty() {
                        // 构建原始响应（Chat 格式，包含完整内容）
                        let original_response = serde_json::json!({
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
                                "completion_tokens": completion_tokens
                            }
                        });
                        if let Ok(response_str) = serde_json::to_string(&original_response) {
                            final_log = final_log.with_original_response(response_str);
                        }

                        // 构建转换后的响应（Responses API 格式）
                        let converted_response = serde_json::json!({
                            "id": format!("resp_{}", uuid::Uuid::new_v4()),
                            "object": "response",
                            "status": "completed",
                            "model": model_for_response,
                            "output": [{
                                "type": "message",
                                "id": format!("msg_{}", uuid::Uuid::new_v4()),
                                "status": "completed",
                                "role": "assistant",
                                "content": [{
                                    "type": "output_text",
                                    "text": full_content
                                }]
                            }],
                            "usage": {
                                "input_tokens": prompt_tokens,
                                "output_tokens": completion_tokens
                            }
                        });
                        if let Ok(response_str) = serde_json::to_string(&converted_response) {
                            final_log = final_log.with_response(response_str);
                        }
                    }

                    state_clone.save_request_log(&final_log);

                    tracing::info!(
                        "Responses API 流式请求完成: latency={}ms, first_token={:?}ms, tokens={}/{}",
                        latency_ms,
                        first_token_ms,
                        prompt_tokens,
                        completion_tokens
                    );
                }));

                // 创建带日志收集的流
                let requested_model_for_stream = requested_model.clone();
                let stream = response.bytes_stream();
                let logged_stream = create_logged_passthrough_stream(stream, collector);
                let converted_stream = create_responses_sse_stream_from_logged(logged_stream, requested_model_for_stream);

                let body = Body::from_stream(converted_stream);

                match builder.body(body) {
                    Ok(resp) => resp,
                    Err(e) => {
                        tracing::error!("构建流式响应失败: {}", e);
                        responses_error(500, format!("Failed to build streaming response: {}", e))
                    }
                }
            } else {
                tracing::info!("Responses API 非流式响应: content-type={:?}", response.headers().get("content-type"));
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
                        return responses_error(500, format!("Failed to read response: {}", e));
                    }
                };

                let response_json: serde_json::Value = match serde_json::from_slice(&body_bytes) {
                    Ok(v) => v,
                    Err(_) => {
                        let body_text = String::from_utf8_lossy(&body_bytes);
                        log_entry = log_entry
                            .with_status(200)
                            .with_latency(latency)
                            .with_response(body_text.to_string());
                        state.save_request_log(&log_entry);
                        return responses_error(500, "Provider returned non-JSON response");
                    }
                };

                tracing::info!(
                    "Responses API 上游响应: model={}, choices={:?}, full_response={}",
                    response_json.get("model").and_then(|v| v.as_str()).unwrap_or("unknown"),
                    response_json.get("choices"),
                    serde_json::to_string(&response_json).unwrap_or_else(|_| "serialize error".to_string())
                );

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

                    return build_response(
                        Response::builder()
                            .status(status.as_u16())
                            .header("content-type", "application/json"),
                        Body::from(serde_json::to_string(&response_json).unwrap_or_default()),
                    )
                    .into_response();
                }

                // 检查上游是否已经返回 Responses 格式
                let (responses_response, needs_conversion) = if response_json.get("output").is_some() && response_json.get("choices").is_none() {
                    // 上游已经是 Responses 格式，检查是否有效
                    let has_valid_content = response_json.get("output")
                        .and_then(|o| o.as_array())
                        .map(|items| {
                            items.iter().any(|item| {
                                // 检查 message 类型是否有非空内容
                                if item.get("type").and_then(|t| t.as_str()) == Some("message") {
                                    item.get("content")
                                        .and_then(|c| c.as_array())
                                        .map(|parts| {
                                            parts.iter().any(|p| {
                                                let ptype = p.get("type").and_then(|t| t.as_str());
                                                if ptype == Some("output_text") || ptype == Some("input_text") {
                                                    p.get("text").and_then(|t| t.as_str()).map(|s| !s.is_empty()).unwrap_or(false)
                                                } else {
                                                    false
                                                }
                                            })
                                        })
                                        .unwrap_or(false)
                                } else if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
                                    // function_call 也是有效内容
                                    true
                                } else {
                                    false
                                }
                            })
                        })
                        .unwrap_or(false);

                    if has_valid_content {
                        tracing::info!("上游已返回有效的 Responses API 格式，直接透传");
                        // 保持用户请求的模型名，而不是上游返回的模型名
                        let mut result = response_json.clone();
                        if let Some(obj) = result.as_object_mut() {
                            obj.insert("model".to_string(), json!(requested_model));
                        }
                        (result, false)
                    } else {
                        // 上游返回空内容，记录警告但仍返回（让客户端知道上游没返回内容）
                        tracing::warn!("上游返回 Responses API 格式但内容为空，可能是免费模型限制");
                        // 保持用户请求的模型名
                        let mut result = response_json.clone();
                        if let Some(obj) = result.as_object_mut() {
                            obj.insert("model".to_string(), json!(requested_model));
                        }
                        (result, false)
                    }
                } else {
                    // 将 OpenAI Chat 格式响应转换为 Responses 格式
                    // 注意：此路径仅在 Router 已启用协议转换时可达（enable_protocol_transform=true）
                    // Router 的 route_responses_raw 会在未启用时直接返回错误，不会到达此处
                    tracing::info!("上游返回 Chat 格式，转换为 Responses API 格式");
                    (chat_to_responses_response(&response_json, &requested_model), true)
                };

                let latency = start_time.elapsed().as_millis() as i64;
                log_entry = log_entry
                    .with_status(status.as_u16() as i32)
                    .with_latency(latency)
                    .with_response(serde_json::to_string(&responses_response).unwrap_or_default());

                // 如果有协议转换，记录原始响应
                if needs_conversion {
                    log_entry = log_entry.with_original_response(serde_json::to_string(&response_json).unwrap_or_default());
                }

                if let Some(usage) = responses_response.get("usage") {
                    let usage_obj: &serde_json::Value = usage;
                    if let Some(prompt) = usage_obj.get("input_tokens").and_then(|v: &serde_json::Value| v.as_u64()) {
                        if let Some(completion) = usage_obj.get("output_tokens").and_then(|v: &serde_json::Value| v.as_u64()) {
                            log_entry = log_entry.with_tokens(prompt as i32, completion as i32);
                        }
                    }
                }

                state.save_request_log(&log_entry);

                build_response(
                    Response::builder()
                        .status(status.as_u16())
                        .header("content-type", "application/json"),
                    Body::from(serde_json::to_string(&responses_response).unwrap_or_default()),
                )
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
            responses_error(500, error_msg)
        }
        _ => {
            let latency = start_time.elapsed().as_millis() as i64;
            log_entry = log_entry
                .with_status(500)
                .with_latency(latency)
                .with_error("Unknown error".to_string());
            state.save_request_log(&log_entry);
            responses_error(500, "Unknown error")
        }
    }
}

/// SSE buffer 最大容量 (1MB)
const SSE_MAX_BUFFER_SIZE: usize = 1024 * 1024;
/// 单个 SSE 事件最大大小 (64KB)
const SSE_MAX_EVENT_SIZE: usize = 64 * 1024;
/// SSE chunk 接收超时时间
const SSE_CHUNK_TIMEOUT: Duration = Duration::from_secs(60);

/// 创建 Responses API 流式响应转换器（从已收集日志的流）
/// 支持: 文本内容、推理内容、工具调用
fn create_responses_sse_stream_from_logged(
    stream: impl futures::Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    requested_model: String,
) -> impl futures::Stream<Item = Result<Bytes, std::io::Error>> + Send {
    let response_id = format!("resp_{}", &uuid::Uuid::new_v4().to_string().replace("-", "")[..24]);
    let _model = requested_model;

    async_stream::stream! {
        let mut buffer = String::new();
        let mut seq: i64 = 0;
        let mut next_seq = || { seq += 1; seq };

        // 状态跟踪
        let mut started = false;
        let mut msg_item_added = false;
        let mut msg_content_added = false;
        let mut msg_closed = false;
        let mut reasoning_opened = false;
        let mut reasoning_closed = false;
        let mut reasoning_id = String::new();
        let mut reasoning_buf = String::new();

        // 工具调用状态 (index -> state)
        let mut tool_call_ids: std::collections::HashMap<usize, String> = std::collections::HashMap::new();
        let mut tool_call_names: std::collections::HashMap<usize, String> = std::collections::HashMap::new();
        let mut tool_call_args: std::collections::HashMap<usize, String> = std::collections::HashMap::new();
        let mut tool_call_done: std::collections::HashMap<usize, bool> = std::collections::HashMap::new();
        let mut next_tool_index: usize = 0;

        let item_id = format!("msg_{}", &uuid::Uuid::new_v4().to_string().replace("-", "")[..24]);
        let mut accumulated_content = String::new();
        let mut usage: Option<serde_json::Value> = None;
        let mut finish_reason: Option<String> = None;

        // 辅助函数：发送事件
        let emit_event = |event_type: &str, data: serde_json::Value, seq_num: i64| -> Result<Bytes, std::io::Error> {
            let mut data = data;
            if let Some(obj) = data.as_object_mut() {
                obj.insert("sequence_number".to_string(), serde_json::json!(seq_num));
            }
            Ok(Bytes::from(format!("event: {}\ndata: {}\n\n", event_type, serde_json::to_string(&data).unwrap_or_default())))
        };

        tokio::pin!(stream);

        loop {
            // 使用超时防止连接挂起
            let chunk_result = match tokio::time::timeout(SSE_CHUNK_TIMEOUT, stream.next()).await {
                Ok(result) => result,
                Err(_) => {
                    tracing::warn!("SSE chunk 接收超时 ({:?})，关闭流", SSE_CHUNK_TIMEOUT);
                    break;
                }
            };

            match chunk_result {
                Some(Ok(bytes)) => {
                    let text = String::from_utf8_lossy(&bytes);

                    // 检查 buffer 容量限制
                    if buffer.len() + text.len() > SSE_MAX_BUFFER_SIZE {
                        tracing::warn!(
                            "SSE buffer 超过 {}KB 限制 (当前 {} + 新增 {} 字节)，清空 buffer",
                            SSE_MAX_BUFFER_SIZE / 1024,
                            buffer.len(),
                            text.len()
                        );
                        buffer.clear();
                    }

                    buffer.push_str(&text);

                    // 解析 SSE 事件
                    while let Some(pos) = buffer.find("\n\n") {
                        let event_text = buffer[..pos].to_string();
                        buffer.drain(..pos + 2);

                        // 检查单个事件大小限制
                        if event_text.len() > SSE_MAX_EVENT_SIZE {
                            tracing::warn!(
                                "SSE 事件超过 {}KB 限制 ({} 字节)，跳过",
                                SSE_MAX_EVENT_SIZE / 1024,
                                event_text.len()
                            );
                            continue;
                        }

                        if !event_text.trim().is_empty() {
                            for line in event_text.lines() {
                                if let Some(data) = strip_sse_field(line, "data") {
                                    if data.trim() == "[DONE]" {
                                        continue;
                                    }

                                    if let Ok(chunk) = serde_json::from_str::<serde_json::Value>(data) {
                                        // 初始化响应
                                        if !started {
                                            started = true;

                                            let created = chrono::Utc::now().timestamp();
                                            yield emit_event("response.created", serde_json::json!({
                                                "type": "response.created",
                                                "response": {
                                                    "id": response_id.clone(),
                                                    "object": "response",
                                                    "created_at": created,
                                                    "status": "in_progress",
                                                    "background": false,
                                                    "error": null,
                                                    "output": []
                                                }
                                            }), next_seq());

                                            yield emit_event("response.in_progress", serde_json::json!({
                                                "type": "response.in_progress",
                                                "response": {
                                                    "id": response_id.clone(),
                                                    "object": "response",
                                                    "created_at": created,
                                                    "status": "in_progress"
                                                }
                                            }), next_seq());
                                        }

                                        // 提取 choices
                                        if let Some(choices) = chunk.get("choices").and_then(|c| c.as_array()) {
                                            for choice in choices {
                                                if let Some(delta) = choice.get("delta") {
                                                    // 1. 处理 reasoning_content（深度思考）
                                                    if let Some(reasoning) = delta.get("reasoning_content").and_then(|r| r.as_str()) {
                                                        if !reasoning.is_empty() {
                                                            // 初始化 reasoning 项
                                                            if !reasoning_opened {
                                                                reasoning_opened = true;
                                                                reasoning_id = format!("rs_{}_0", response_id);

                                                                yield emit_event("response.output_item.added", serde_json::json!({
                                                                    "type": "response.output_item.added",
                                                                    "output_index": 0,
                                                                    "item": {
                                                                        "id": reasoning_id.clone(),
                                                                        "type": "reasoning",
                                                                        "status": "in_progress",
                                                                        "summary": []
                                                                    }
                                                                }), next_seq());

                                                                yield emit_event("response.reasoning_summary_part.added", serde_json::json!({
                                                                    "type": "response.reasoning_summary_part.added",
                                                                    "item_id": reasoning_id.clone(),
                                                                    "output_index": 0,
                                                                    "summary_index": 0,
                                                                    "part": {
                                                                        "type": "summary_text",
                                                                        "text": ""
                                                                    }
                                                                }), next_seq());
                                                            }

                                                            reasoning_buf.push_str(reasoning);
                                                            yield emit_event("response.reasoning_summary_text.delta", serde_json::json!({
                                                                "type": "response.reasoning_summary_text.delta",
                                                                "item_id": reasoning_id.clone(),
                                                                "output_index": 0,
                                                                "summary_index": 0,
                                                                "delta": reasoning
                                                            }), next_seq());
                                                        }
                                                    }

                                                    // 2. 处理普通文本内容
                                                    if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                                                        if !content.is_empty() {
                                                            // 在输出文本前，先关闭 reasoning（如果有的话）
                                                            if reasoning_opened && !reasoning_closed {
                                                                reasoning_closed = true;
                                                                yield emit_event("response.reasoning_summary_text.done", serde_json::json!({
                                                                    "type": "response.reasoning_summary_text.done",
                                                                    "item_id": reasoning_id.clone(),
                                                                    "output_index": 0,
                                                                    "summary_index": 0,
                                                                    "text": reasoning_buf.clone()
                                                                }), next_seq());

                                                                yield emit_event("response.reasoning_summary_part.done", serde_json::json!({
                                                                    "type": "response.reasoning_summary_part.done",
                                                                    "item_id": reasoning_id.clone(),
                                                                    "output_index": 0,
                                                                    "summary_index": 0,
                                                                    "part": {
                                                                        "type": "summary_text",
                                                                        "text": reasoning_buf.clone()
                                                                    }
                                                                }), next_seq());

                                                                yield emit_event("response.output_item.done", serde_json::json!({
                                                                    "type": "response.output_item.done",
                                                                    "output_index": 0,
                                                                    "item": {
                                                                        "id": reasoning_id.clone(),
                                                                        "type": "reasoning",
                                                                        "summary": [{
                                                                            "type": "summary_text",
                                                                            "text": reasoning_buf.clone()
                                                                        }]
                                                                    }
                                                                }), next_seq());
                                                            }

                                                            // 初始化消息项
                                                            let msg_index = if reasoning_opened { 1 } else { 0 };
                                                            if !msg_item_added {
                                                                msg_item_added = true;

                                                                yield emit_event("response.output_item.added", serde_json::json!({
                                                                    "type": "response.output_item.added",
                                                                    "output_index": msg_index,
                                                                    "item": {
                                                                        "id": item_id.clone(),
                                                                        "type": "message",
                                                                        "status": "in_progress",
                                                                        "content": [],
                                                                        "role": "assistant"
                                                                    }
                                                                }), next_seq());
                                                            }

                                                            if !msg_content_added {
                                                                msg_content_added = true;
                                                                yield emit_event("response.content_part.added", serde_json::json!({
                                                                    "type": "response.content_part.added",
                                                                    "item_id": item_id.clone(),
                                                                    "output_index": msg_index,
                                                                    "content_index": 0,
                                                                    "part": {
                                                                        "type": "output_text",
                                                                        "annotations": [],
                                                                        "logprobs": [],
                                                                        "text": ""
                                                                    }
                                                                }), next_seq());
                                                            }

                                                            accumulated_content.push_str(content);
                                                            yield emit_event("response.output_text.delta", serde_json::json!({
                                                                "type": "response.output_text.delta",
                                                                "item_id": item_id.clone(),
                                                                "output_index": msg_index,
                                                                "content_index": 0,
                                                                "delta": content,
                                                                "logprobs": []
                                                            }), next_seq());
                                                        }
                                                    }

                                                    // 3. 处理 tool_calls
                                                    if let Some(tool_calls_delta) = delta.get("tool_calls").and_then(|tc| tc.as_array()) {
                                                        // 先关闭消息项（如果有内容）
                                                        if msg_item_added && !msg_closed {
                                                            msg_closed = true;
                                                            let msg_index = if reasoning_opened { 1 } else { 0 };

                                                            yield emit_event("response.output_text.done", serde_json::json!({
                                                                "type": "response.output_text.done",
                                                                "item_id": item_id.clone(),
                                                                "output_index": msg_index,
                                                                "content_index": 0,
                                                                "text": accumulated_content.clone(),
                                                                "logprobs": []
                                                            }), next_seq());

                                                            yield emit_event("response.content_part.done", serde_json::json!({
                                                                "type": "response.content_part.done",
                                                                "item_id": item_id.clone(),
                                                                "output_index": msg_index,
                                                                "content_index": 0,
                                                                "part": {
                                                                    "type": "output_text",
                                                                    "annotations": [],
                                                                    "logprobs": [],
                                                                    "text": accumulated_content.clone()
                                                                }
                                                            }), next_seq());

                                                            yield emit_event("response.output_item.done", serde_json::json!({
                                                                "type": "response.output_item.done",
                                                                "output_index": msg_index,
                                                                "item": {
                                                                    "id": item_id.clone(),
                                                                    "type": "message",
                                                                    "status": "completed",
                                                                    "content": [{
                                                                        "type": "output_text",
                                                                        "annotations": [],
                                                                        "logprobs": [],
                                                                        "text": accumulated_content.clone()
                                                                    }],
                                                                    "role": "assistant"
                                                                }
                                                            }), next_seq());
                                                        }

                                                        for tc in tool_calls_delta {
                                                            let tc_index = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(next_tool_index as u64) as usize;
                                                            let tc_id = tc.get("id").and_then(|i| i.as_str());
                                                            let tc_function = tc.get("function");
                                                            let tc_name = tc_function.and_then(|f| f.get("name")).and_then(|n| n.as_str());
                                                            let tc_args = tc_function.and_then(|f| f.get("arguments")).and_then(|a| a.as_str());

                                                            // 新的工具调用
                                                            if let Some(id) = tc_id {
                                                                if id.is_empty() { continue; }

                                                                // 检查是否是新的工具调用
                                                                let is_new = !tool_call_ids.contains_key(&tc_index);
                                                                if is_new {
                                                                    let call_id = id.to_string();
                                                                    let fc_id = format!("fc_{}", call_id);
                                                                    tool_call_ids.insert(tc_index, call_id.clone());
                                                                    next_tool_index = (tc_index + 1).max(next_tool_index);

                                                                    if let Some(name) = tc_name {
                                                                        if !name.is_empty() {
                                                                            tool_call_names.insert(tc_index, name.to_string());
                                                                        }
                                                                    }

                                                                    // 计算输出索引
                                                                    let output_index = tc_index + if reasoning_opened { 1 } else { 0 } + if msg_item_added { 1 } else { 0 };

                                                                    yield emit_event("response.output_item.added", serde_json::json!({
                                                                        "type": "response.output_item.added",
                                                                        "output_index": output_index,
                                                                        "item": {
                                                                            "id": fc_id,
                                                                            "type": "function_call",
                                                                            "status": "in_progress",
                                                                            "arguments": "",
                                                                            "call_id": call_id,
                                                                            "name": tool_call_names.get(&tc_index).cloned().unwrap_or_default()
                                                                        }
                                                                    }), next_seq());
                                                                }
                                                            }

                                                            // 参数增量
                                                            if let Some(args) = tc_args {
                                                                if !args.is_empty() {
                                                                    let current_args = tool_call_args.entry(tc_index).or_default();
                                                                    current_args.push_str(args);

                                                                    if let Some(call_id) = tool_call_ids.get(&tc_index) {
                                                                        let output_index = tc_index + if reasoning_opened { 1 } else { 0 } + if msg_item_added { 1 } else { 0 };
                                                                        yield emit_event("response.function_call_arguments.delta", serde_json::json!({
                                                                            "type": "response.function_call_arguments.delta",
                                                                            "item_id": format!("fc_{}", call_id),
                                                                            "output_index": output_index,
                                                                            "delta": args
                                                                        }), next_seq());
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }

                                                // 提取 finish_reason
                                                if let Some(fr) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                                                    finish_reason = Some(fr.to_string());
                                                }
                                            }
                                        }

                                        // 提取 usage
                                        if let Some(u) = chunk.get("usage") {
                                            usage = Some(u.clone());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Some(Err(e)) => {
                    tracing::error!("流错误: {}", e);
                    yield Err(e);
                    return;
                }
                None => {
                    break;
                }
            }
        }

        // ===== 最终化 =====

        // 关闭消息项
        if msg_item_added && !msg_closed {
            let msg_index = if reasoning_opened { 1 } else { 0 };
            yield emit_event("response.output_text.done", serde_json::json!({
                "type": "response.output_text.done",
                "item_id": item_id.clone(),
                "output_index": msg_index,
                "content_index": 0,
                "text": accumulated_content.clone(),
                "logprobs": []
            }), next_seq());

            yield emit_event("response.content_part.done", serde_json::json!({
                "type": "response.content_part.done",
                "item_id": item_id.clone(),
                "output_index": msg_index,
                "content_index": 0,
                "part": {
                    "type": "output_text",
                    "annotations": [],
                    "logprobs": [],
                    "text": accumulated_content.clone()
                }
            }), next_seq());

            yield emit_event("response.output_item.done", serde_json::json!({
                "type": "response.output_item.done",
                "output_index": msg_index,
                "item": {
                    "id": item_id,
                    "type": "message",
                    "status": "completed",
                    "content": [{
                        "type": "output_text",
                        "annotations": [],
                        "logprobs": [],
                        "text": accumulated_content.clone()
                    }],
                    "role": "assistant"
                }
            }), next_seq());
        }

        // 关闭工具调用
        for (tc_index, call_id) in &tool_call_ids {
            if tool_call_done.get(tc_index).copied().unwrap_or(false) {
                continue;
            }
            tool_call_done.insert(*tc_index, true);

            let args = tool_call_args.get(tc_index).cloned().unwrap_or_else(|| "{}".to_string());
            let name = tool_call_names.get(tc_index).cloned().unwrap_or_default();
            let output_index = tc_index + if reasoning_opened { 1 } else { 0 } + if msg_item_added { 1 } else { 0 };
            let fc_id = format!("fc_{}", call_id);

            yield emit_event("response.function_call_arguments.done", serde_json::json!({
                "type": "response.function_call_arguments.done",
                "item_id": fc_id.clone(),
                "output_index": output_index,
                "arguments": args
            }), next_seq());

            yield emit_event("response.output_item.done", serde_json::json!({
                "type": "response.output_item.done",
                "output_index": output_index,
                "item": {
                    "id": fc_id,
                    "type": "function_call",
                    "status": "completed",
                    "arguments": args,
                    "call_id": call_id,
                    "name": name
                }
            }), next_seq());
        }

        // 构建 output 数组
        let mut output: Vec<serde_json::Value> = Vec::new();

        // 添加 reasoning
        if reasoning_opened {
            output.push(serde_json::json!({
                "id": reasoning_id.clone(),
                "type": "reasoning",
                "summary": [{
                    "type": "summary_text",
                    "text": reasoning_buf
                }]
            }));
        }

        // 添加 message
        if msg_item_added {
            output.push(serde_json::json!({
                "id": item_id.clone(),
                "type": "message",
                "status": "completed",
                "content": [{
                    "type": "output_text",
                    "annotations": [],
                    "logprobs": [],
                    "text": accumulated_content
                }],
                "role": "assistant"
            }));
        }

        // 添加 function_calls
        for (tc_index, call_id) in &tool_call_ids {
            let args = tool_call_args.get(tc_index).cloned().unwrap_or_else(|| "{}".to_string());
            let name = tool_call_names.get(tc_index).cloned().unwrap_or_default();
            output.push(serde_json::json!({
                "id": format!("fc_{}", call_id),
                "type": "function_call",
                "status": "completed",
                "arguments": args,
                "call_id": call_id,
                "name": name
            }));
        }

        // 构建 usage
        let final_usage = usage.map(|u| {
            serde_json::json!({
                "input_tokens": u.get("prompt_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
                "output_tokens": u.get("completion_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
                "total_tokens": u.get("total_tokens").and_then(|v| v.as_i64()).unwrap_or(0)
            })
        });

        // 确定 status
        let status = match finish_reason.as_deref() {
            Some("stop") => "completed",
            Some("length") => "incomplete",
            Some("tool_calls") => "completed",
            _ => "completed",
        };

        // 发送 response.completed
        yield emit_event("response.completed", serde_json::json!({
            "type": "response.completed",
            "response": {
                "id": response_id,
                "object": "response",
                "created_at": chrono::Utc::now().timestamp(),
                "status": status,
                "background": false,
                "error": null,
                "output": output,
                "usage": final_usage
            }
        }), next_seq());
    }
}

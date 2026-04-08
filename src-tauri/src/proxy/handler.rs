use axum::{
    body::Body,
    extract::{Request, State},
    response::{IntoResponse, Response},
    Json,
};
use bytes::Bytes;
use futures::StreamExt;
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use crate::models::{ChatRequest, EmbeddingRequest};
use crate::router::{Router, chat_to_responses_response};
use crate::state::AppState;
use crate::models::RequestLog;
use crate::pricing::{calculate_cost, normalize_model_name_with_prefix};

/// 判断响应是否是 SSE 流式响应
fn is_sse_response(response: &reqwest::Response) -> bool {
    // 检查 content-type
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // 标准的 SSE content-type
    if content_type.contains("text/event-stream") {
        return true;
    }

    // 某些供应商可能使用 application/x-ndjson 或其他类型
    if content_type.contains("application/x-ndjson") {
        return true;
    }

    // 检查是否是流式响应（某些供应商可能没有正确的 content-type）
    // 如果 transfer-encoding 是 chunked，也可能是流式
    if let Some(transfer_encoding) = response.headers().get("transfer-encoding") {
        if let Ok(te) = transfer_encoding.to_str() {
            if te.contains("chunked") && !content_type.contains("application/json") {
                return true;
            }
        }
    }

    false
}

/// 从 SSE 行中提取指定字段的值
#[inline]
fn strip_sse_field<'a>(line: &'a str, field: &str) -> Option<&'a str> {
    line.strip_prefix(&format!("{field}: "))
        .or_else(|| line.strip_prefix(&format!("{field}:")))
}

/// SSE 使用量收集器
struct SseUsageCollector {
    events: Arc<Mutex<Vec<serde_json::Value>>>,
    first_event_time: Arc<Mutex<Option<Instant>>>,
    start_time: Instant,
    on_complete: Box<dyn Fn(Vec<serde_json::Value>, i64, Option<i64>, i32, i32, String) + Send + Sync>,
}

impl SseUsageCollector {
    fn new(
        start_time: Instant,
        on_complete: impl Fn(Vec<serde_json::Value>, i64, Option<i64>, i32, i32, String) + Send + Sync + 'static,
    ) -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
            first_event_time: Arc::new(Mutex::new(None)),
            start_time,
            on_complete: Box::new(on_complete),
        }
    }

    async fn push(&self, event: serde_json::Value) {
        // 记录首个事件时间
        {
            let mut first_time = self.first_event_time.lock().await;
            if first_time.is_none() {
                *first_time = Some(Instant::now());
            }
        }
        self.events.lock().await.push(event);
    }

    async fn finish(&self) {
        let events = self.events.lock().await.clone();
        let latency_ms = self.start_time.elapsed().as_millis() as i64;

        // 计算首 token 延迟
        let first_token_ms = {
            let first_time = self.first_event_time.lock().await;
            first_time.map(|t| (t - self.start_time).as_millis() as i64)
        };

        // 从事件中提取 usage
        let mut prompt_tokens = 0i32;
        let mut completion_tokens = 0i32;

        // 累积内容
        let mut accumulated_content = String::new();
        let mut accumulated_reasoning = String::new();

        for event in &events {
            // OpenAI 格式的 usage
            if let Some(usage) = event.get("usage") {
                if let Some(p) = usage.get("prompt_tokens").and_then(|v| v.as_i64()) {
                    prompt_tokens = p as i32;
                }
                if let Some(c) = usage.get("completion_tokens").and_then(|v| v.as_i64()) {
                    completion_tokens = c as i32;
                }
                // Claude 格式的 usage (input_tokens, output_tokens)
                if let Some(p) = usage.get("input_tokens").and_then(|v| v.as_i64()) {
                    prompt_tokens = p as i32;
                }
                if let Some(c) = usage.get("output_tokens").and_then(|v| v.as_i64()) {
                    completion_tokens = c as i32;
                }
            }

            // 某些供应商在最后一个 chunk 中附带 usage
            if let Some(x_gpt_usage) = event.get("x_gpt_usage") {
                if let Some(p) = x_gpt_usage.get("prompt_tokens").and_then(|v| v.as_i64()) {
                    prompt_tokens = p as i32;
                }
                if let Some(c) = x_gpt_usage.get("completion_tokens").and_then(|v| v.as_i64()) {
                    completion_tokens = c as i32;
                }
            }

            // 提取内容（OpenAI 格式：从 delta.content）
            if let Some(choices) = event.get("choices").and_then(|c| c.as_array()) {
                for choice in choices {
                    if let Some(delta) = choice.get("delta") {
                        // 普通内容
                        if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                            accumulated_content.push_str(content);
                        }
                        // reasoning 内容
                        if let Some(reasoning) = delta.get("reasoning_content").and_then(|r| r.as_str()) {
                            accumulated_reasoning.push_str(reasoning);
                        }
                    }
                }
            }

            // 提取内容（Claude 格式：从 delta.text）
            if let Some(delta) = event.get("delta") {
                // 普通文本内容
                if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                    accumulated_content.push_str(text);
                }
                // thinking 内容
                if let Some(thinking) = delta.get("thinking").and_then(|t| t.as_str()) {
                    accumulated_reasoning.push_str(thinking);
                }
            }
        }

        // 构建完整内容（优先 reasoning + content）
        let full_content = if !accumulated_reasoning.is_empty() {
            format!("[思考]\n{}\n\n[回答]\n{}", accumulated_reasoning, accumulated_content)
        } else {
            accumulated_content.clone()
        };

        (self.on_complete)(events, latency_ms, first_token_ms, prompt_tokens, completion_tokens, full_content);
    }
}

/// 创建带日志记录的透传流
fn create_logged_passthrough_stream(
    stream: impl futures::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
    collector: Arc<SseUsageCollector>,
) -> impl futures::Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();

        tokio::pin!(stream);

        loop {
            let chunk_result = stream.next().await;

            match chunk_result {
                Some(Ok(bytes)) => {
                    let text = String::from_utf8_lossy(&bytes);
                    buffer.push_str(&text);

                    // 尝试解析完整的 SSE 事件
                    while let Some(pos) = buffer.find("\n\n") {
                        let event_text = buffer[..pos].to_string();
                        buffer = buffer[pos + 2..].to_string();

                        if !event_text.trim().is_empty() {
                            // 提取 data 部分并尝试解析
                            for line in event_text.lines() {
                                if let Some(data) = strip_sse_field(line, "data") {
                                    if data.trim() != "[DONE]" {
                                        if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(data) {
                                            collector.push(json_value).await;
                                        }
                                    }
                                }
                            }
                        }
                    }

                    yield Ok(bytes);
                }
                Some(Err(e)) => {
                    tracing::error!("流错误: {}", e);
                    yield Err(std::io::Error::other(e.to_string()));
                    break;
                }
                None => {
                    // 流正常结束，处理 buffer 中剩余的内容
                    if !buffer.trim().is_empty() {
                        tracing::debug!("处理 buffer 中剩余的内容: {}", buffer);
                        // 尝试解析剩余的 buffer
                        for line in buffer.lines() {
                            if let Some(data) = strip_sse_field(line, "data") {
                                if data.trim() != "[DONE]" && !data.trim().is_empty() {
                                    if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(data) {
                                        collector.push(json_value).await;
                                    }
                                }
                            }
                        }
                    }
                    break;
                }
            }
        }

        // 流结束，记录日志
        collector.finish().await;
    }
}

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
                        tracing::info!("流式响应提取到 tokens: prompt={}, completion={}", prompt_tokens, completion_tokens);

                        // 计算成本
                        if let Some(model) = &model_for_cost {
                            // 使用带 prefix 的标准化，正确处理模型名本身包含 / 的情况
                            let prefix_ref = provider_prefix_for_cost.as_deref();
                            let normalized_model = normalize_model_name_with_prefix(model, prefix_ref);
                            tracing::info!("流式成本计算: model={}, prefix={:?}, normalized={}", model, prefix_ref, normalized_model);

                            if let Some(ref provider) = provider_for_cost_clone {
                                tracing::info!("流式找到 Provider: name={}, enable_cost={}", provider.name, provider.enable_cost);
                                
                                if provider.enable_cost {
                                    // 优先使用 Provider 的模型定价
                                    let pricing_opt = provider.get_model_pricing(normalized_model)
                                        .map(|p| crate::pricing::PricingEntry::new(p.input, p.output));
                                    
                                    tracing::info!("流式 Provider 定价查找: model={}, found={}", normalized_model, pricing_opt.is_some());

                                    // 如果没有，使用全局定价
                                    let pricing = pricing_opt.or_else(|| {
                                        let pm = pricing_manager.read();
                                        let global_pricing = pm.get_pricing(&provider.prefix, normalized_model);
                                        tracing::info!("流式全局定价查找: prefix={}, model={}, found={}", provider.prefix, normalized_model, global_pricing.is_some());
                                        global_pricing
                                    });

                                    if let Some(pricing) = pricing {
                                        let cost = calculate_cost(prompt_tokens, completion_tokens, None, None, &pricing);
                                        final_log = final_log.with_cost(cost);
                                        tracing::info!("流式成本计算完成: cost=${:.6}", cost);
                                    } else {
                                        tracing::warn!("流式未找到模型定价: model={}", normalized_model);
                                    }
                                } else {
                                    tracing::info!("流式 Provider 未启用成本统计");
                                }
                            } else {
                                tracing::warn!("流式未找到 Provider");
                            }
                        } else {
                            tracing::warn!("流式未设置 actual_model");
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

                    let _ = state_clone.save_request_log(&final_log);

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
                        return Json(json!({
                            "error": {
                                "message": format!("Failed to build streaming response: {}", e),
                                "type": "internal_error"
                            }
                        }))
                        .into_response();
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
                        .unwrap()
                        .into_response();
                }

                // 检查上游是否返回了 Responses API 格式（需要转换为 Chat 格式）
                let (final_response, needs_conversion) = if response_json.get("output").is_some() && response_json.get("choices").is_none() {
                    tracing::info!("上游返回 Responses API 格式，转换为 Chat Completions 格式");
                    (responses_to_chat_response(&response_json, &info.requested_model), true)
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
                            tracing::info!("提取到 tokens: prompt={}, completion={}", prompt, completion);

                            // 计算成本
                            if let Some(model) = &info.actual_model {
                                // 使用带 prefix 的标准化，正确处理模型名本身包含 / 的情况
                                let prefix_ref = info.provider_prefix.as_deref();
                                let normalized_model = normalize_model_name_with_prefix(model, prefix_ref);
                                tracing::info!("成本计算: model={}, prefix={:?}, normalized={}", model, prefix_ref, normalized_model);

                                // 检查 provider_for_cost
                                if let Some(ref provider) = provider_for_cost {
                                    tracing::info!("找到 Provider: name={}, enable_cost={}", provider.name, provider.enable_cost);
                                    
                                    if provider.enable_cost {
                                        // 优先使用 Provider 的模型定价
                                        let pricing_opt = provider.get_model_pricing(normalized_model)
                                            .map(|p| crate::pricing::PricingEntry::new(p.input, p.output));
                                        
                                        tracing::info!("Provider 定价查找: model={}, found={}", normalized_model, pricing_opt.is_some());

                                        // 如果没有，使用全局定价
                                        let pricing = pricing_opt.or_else(|| {
                                            let pm = state.pricing_manager.read();
                                            let global_pricing = pm.get_pricing(&provider.prefix, normalized_model);
                                            tracing::info!("全局定价查找: prefix={}, model={}, found={}", provider.prefix, normalized_model, global_pricing.is_some());
                                            global_pricing
                                        });

                                        if let Some(pricing) = pricing {
                                            let cost = calculate_cost(prompt as i32, completion as i32, None, None, &pricing);
                                            log_entry = log_entry.with_cost(cost);
                                            tracing::info!("成本计算完成: cost=${:.6}", cost);
                                        } else {
                                            tracing::warn!("未找到模型定价: model={}", normalized_model);
                                        }
                                    } else {
                                        tracing::info!("Provider 未启用成本统计");
                                    }
                                } else {
                                    tracing::warn!("未找到 Provider: prefix={:?}", info.provider_prefix);
                                }
                            } else {
                                tracing::warn!("未设置 actual_model");
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
                    let start = start_time.clone();

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
                                        if line.starts_with("data: ") {
                                            let data = &line[6..];
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
                                        buffer = buffer[pos + 1..].to_string();
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
                    return builder
                        .header("content-type", "text/event-stream")
                        .header("cache-control", "no-cache")
                        .header("connection", "keep-alive")
                        .body(Body::from_stream(ReceiverStream::new(rx)))
                        .unwrap()
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

                    return builder.body(Body::from(body_bytes)).unwrap().into_response();
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

                    let _ = state_clone.save_request_log(&final_log);

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
                        return responses_error(500, format!("Failed to build streaming response: {}", e));
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

                    return Response::builder()
                        .status(status.as_u16())
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_string(&response_json).unwrap_or_default()))
                        .unwrap()
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

                Response::builder()
                    .status(status.as_u16())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&responses_response).unwrap_or_default()))
                    .unwrap()
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

/// 创建 Responses API 流式响应转换器（从已收集日志的流）
/// 支持: 文本内容、推理内容、工具调用
fn create_responses_sse_stream_from_logged(
    stream: impl futures::Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    requested_model: String,
) -> impl futures::Stream<Item = Result<Bytes, std::io::Error>> + Send {
    let response_id = format!("resp_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..24].to_string());
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

        let item_id = format!("msg_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..24].to_string());
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
            let chunk_result = stream.next().await;

            match chunk_result {
                Some(Ok(bytes)) => {
                    let text = String::from_utf8_lossy(&bytes);
                    buffer.push_str(&text);

                    // 解析 SSE 事件
                    while let Some(pos) = buffer.find("\n\n") {
                        let event_text = buffer[..pos].to_string();
                        buffer = buffer[pos + 2..].to_string();

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
                                                                    let current_args = tool_call_args.entry(tc_index).or_insert_with(String::new);
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

/// 创建 Responses API 格式错误响应
fn responses_error(status: u16, message: impl Into<String>) -> Response {
    let msg = message.into();
    let body = json!({
        "error": {
            "message": msg,
            "type": "api_error",
            "code": null,
            "param": null
        }
    });
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap_or_default()))
        .unwrap()
        .into_response()
}

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
                        tracing::info!("Claude 流式响应提取到 tokens: prompt={}, completion={}", prompt_tokens, completion_tokens);

                        // 计算成本
                        if let Some(model) = &model_for_cost {
                            let prefix_ref = provider_prefix_for_cost.as_deref();
                            let normalized_model = normalize_model_name_with_prefix(model, prefix_ref);

                            if let Some(ref provider) = provider_for_cost {
                                if provider.enable_cost {
                                    let pricing_opt = provider.get_model_pricing(normalized_model)
                                        .map(|p| crate::pricing::PricingEntry::new(p.input, p.output));

                                    let pricing = pricing_opt.or_else(|| {
                                        let pm = pricing_manager.read();
                                        pm.get_pricing(&provider.prefix, normalized_model)
                                    });

                                    if let Some(pricing) = pricing {
                                        let cost = calculate_cost(prompt_tokens, completion_tokens, None, None, &pricing);
                                        final_log = final_log.with_cost(cost);
                                    }
                                }
                            }
                        }
                    }

                    // 保存最后的 SSE 事件作为响应记录
                    if let Some(last_event) = events.last() {
                        if let Ok(response_str) = serde_json::to_string(last_event) {
                            final_log = final_log.with_response(response_str);
                        }
                    }

                    let _ = state_clone.save_request_log(&final_log);
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
                        return claude_error(500, format!("Failed to build streaming response: {}", e));
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
                    .unwrap()
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

/// 创建 Claude 格式错误响应
fn claude_error(status: u16, message: impl Into<String>) -> Response {
    let msg = message.into();
    let resp_type = if status >= 500 { "error" } else { "error_response" };
    let body = json!({
        "type": resp_type,
        "error": {
            "type": "api_error",
            "message": msg
        }
    });
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap_or_default()))
        .unwrap()
        .into_response()
}

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

/// 将 Responses API 格式响应转换为 Chat Completions 格式
fn responses_to_chat_response(responses_resp: &serde_json::Value, requested_model: &str) -> serde_json::Value {
    let id = responses_resp.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
    // 使用用户请求的模型名，而不是上游返回的模型名
    let model = requested_model;

    // 从 output 数组提取内容
    let output = responses_resp.get("output").and_then(|v| v.as_array());
    let mut content = String::new();
    let mut tool_calls: Vec<serde_json::Value> = Vec::new();
    let mut has_tool_use = false;

    if let Some(items) = output {
        for item in items {
            let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");

            match item_type {
                "message" => {
                    if let Some(msg_content) = item.get("content").and_then(|c| c.as_array()) {
                        for block in msg_content {
                            let block_type = block.get("type").and_then(|t| t.as_str());
                            if block_type == Some("output_text") || block_type == Some("input_text") {
                                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                    content.push_str(text);
                                }
                            }
                        }
                    }
                }
                "function_call" => {
                    let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let args = item.get("arguments").and_then(|v| v.as_str()).unwrap_or("{}");

                    tool_calls.push(json!({
                        "id": call_id,
                        "type": "function",
                        "function": {"name": name, "arguments": args}
                    }));
                    has_tool_use = true;
                }
                _ => {}
            }
        }
    }

    // 处理 status → finish_reason
    let status = responses_resp.get("status").and_then(|v| v.as_str()).unwrap_or("completed");
    let finish_reason = match status {
        "completed" => if has_tool_use { "tool_calls" } else { "stop" },
        "incomplete" => "length",
        _ => "stop",
    };

    // 处理 usage
    let usage = responses_resp.get("usage").cloned().unwrap_or(json!({}));
    let prompt_tokens = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let completion_tokens = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);

    let message = if !tool_calls.is_empty() {
        json!({
            "role": "assistant",
            "content": if content.is_empty() { serde_json::Value::Null } else { json!(content) },
            "tool_calls": tool_calls
        })
    } else {
        json!({"role": "assistant", "content": content})
    };

    json!({
        "id": id,
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": finish_reason
        }],
        "usage": {
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "total_tokens": prompt_tokens + completion_tokens
        }
    })
}

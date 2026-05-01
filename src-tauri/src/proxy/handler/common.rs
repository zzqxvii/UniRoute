use axum::{
    body::Body,
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures::StreamExt;
use serde_json::json;
use tokio::time::Duration;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

/// 判断响应是否是 SSE 流式响应
pub fn is_sse_response(response: &reqwest::Response) -> bool {
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
pub fn strip_sse_field<'a>(line: &'a str, field: &str) -> Option<&'a str> {
    line.strip_prefix(&format!("{field}: "))
        .or_else(|| line.strip_prefix(&format!("{field}:")))
}

/// SSE 使用量收集完成回调类型
type OnCompleteCallback = Box<dyn Fn(Vec<serde_json::Value>, i64, Option<i64>, i32, i32, String) + Send + Sync>;

/// SSE 使用量收集器
pub struct SseUsageCollector {
    events: Arc<Mutex<Vec<serde_json::Value>>>,
    first_event_time: Arc<Mutex<Option<Instant>>>,
    start_time: Instant,
    on_complete: OnCompleteCallback,
}

impl SseUsageCollector {
    pub fn new(
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

    pub async fn push(&self, event: serde_json::Value) {
        // 记录首个事件时间
        {
            let mut first_time = self.first_event_time.lock().await;
            if first_time.is_none() {
                *first_time = Some(Instant::now());
            }
        }
        self.events.lock().await.push(event);
    }

    pub async fn finish(&self) {
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

/// SSE buffer 最大容量 (1MB)
const SSE_MAX_BUFFER_SIZE: usize = 1024 * 1024;
/// 单个 SSE 事件最大大小 (64KB)
const SSE_MAX_EVENT_SIZE: usize = 64 * 1024;
/// SSE chunk 接收超时时间
const SSE_CHUNK_TIMEOUT: Duration = Duration::from_secs(60);

/// 创建带日志记录的透传流
pub fn create_logged_passthrough_stream(
    stream: impl futures::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
    collector: Arc<SseUsageCollector>,
) -> impl futures::Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();

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

                    // 尝试解析完整的 SSE 事件
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

/// 创建 Responses API 格式错误响应
pub fn responses_error(status: u16, message: impl Into<String>) -> Response {
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
        .unwrap_or_else(|_| Response::builder().status(500).body(Body::empty()).unwrap())
        .into_response()
}

/// 创建 Claude 格式错误响应
pub fn claude_error(status: u16, message: impl Into<String>) -> Response {
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
        .unwrap_or_else(|_| Response::builder().status(500).body(Body::empty()).unwrap())
        .into_response()
}

/// 计算请求成本（公共逻辑）
///
/// 优先使用 Provider 的模型定价，其次使用全局定价。
/// 返回 Some(cost) 或 None（未找到定价或未启用成本统计）。
pub fn calculate_request_cost(
    provider: &crate::models::Provider,
    pricing_manager: &parking_lot::RwLock<crate::pricing::PricingManager>,
    model: &str,
    prefix: Option<&str>,
    prompt_tokens: i32,
    completion_tokens: i32,
) -> Option<f64> {
    if !provider.enable_cost {
        return None;
    }

    let normalized_model = crate::pricing::normalize_model_name_with_prefix(model, prefix);

    // 优先使用 Provider 的模型定价
    let pricing_opt = provider
        .get_model_pricing(normalized_model)
        .map(|p| crate::pricing::PricingEntry::new(p.input, p.output));

    // 如果没有，使用全局定价
    let pricing = pricing_opt.or_else(|| {
        let pm = pricing_manager.read();
        pm.get_pricing(&provider.prefix, normalized_model)
    });

    pricing.map(|p| crate::pricing::calculate_cost(prompt_tokens, completion_tokens, None, None, &p))
}

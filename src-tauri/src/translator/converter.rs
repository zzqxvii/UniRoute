use crate::models::{ApiFormat, ChatRequest, Message, MessageContent};
use crate::translator::{StreamState, Translator, TranslatorError};
use async_trait::async_trait;
use serde_json::{json, Value};

/// 默认协议转换器
pub struct DefaultTranslator {}

impl DefaultTranslator {
    pub fn new() -> Self {
        Self {}
    }

    // ========== 辅助函数 ==========

    /// OpenAI finish_reason → Responses API status
    fn finish_reason_to_status(finish_reason: &str) -> &'static str {
        match finish_reason {
            "stop" | "tool_calls" => "completed",
            "length" => "incomplete",
            _ => "completed",
        }
    }

    /// Claude stop_reason → Responses API status
    fn stop_reason_to_status(stop_reason: &str) -> &'static str {
        match stop_reason {
            "end_turn" | "tool_use" => "completed",
            "max_tokens" => "incomplete",
            _ => "completed",
        }
    }

    /// 构建 Responses API 格式的 usage 对象
    fn build_responses_usage(input_tokens: u64, output_tokens: u64) -> Value {
        json!({
            "input_tokens": input_tokens,
            "input_tokens_details": { "cached_tokens": 0 },
            "output_tokens": output_tokens,
            "output_tokens_details": { "reasoning_tokens": 0 },
            "total_tokens": input_tokens + output_tokens
        })
    }

    /// 构建 Responses API 的 output_text 项
    fn build_output_text(text: &str) -> Value {
        json!({
            "type": "output_text",
            "text": text,
            "annotations": []
        })
    }

    /// 构建 Responses API 的 message 项
    fn build_responses_message(id: &str, status: &str, content: Vec<Value>) -> Value {
        json!({
            "type": "message",
            "id": format!("msg_{}", id),
            "status": status,
            "role": "assistant",
            "content": content
        })
    }

    /// 构建 Responses API 的 function_call 项
    fn build_responses_function_call(call_id: &str, name: &str, args: &str) -> Value {
        json!({
            "type": "function_call",
            "id": format!("fc_{}", call_id),
            "call_id": call_id,
            "name": name,
            "arguments": args
        })
    }
}

#[async_trait]
impl Translator for DefaultTranslator {
    fn translate_request(
        &self,
        source: ApiFormat,
        target: ApiFormat,
        request: &ChatRequest,
    ) -> Result<Value, TranslatorError> {
        match (source, target) {
            (ApiFormat::OpenAI, ApiFormat::OpenAI) => {
                Ok(serde_json::to_value(request)?)
            }
            (ApiFormat::OpenAI, ApiFormat::Claude) => {
                self.openai_to_claude_request(request)
            }
            (ApiFormat::OpenAI, ApiFormat::Gemini) => {
                self.openai_to_gemini_request(request)
            }
            (ApiFormat::OpenAI, ApiFormat::Responses) => {
                self.openai_to_responses_request(request)
            }
            (ApiFormat::Claude, ApiFormat::OpenAI) => {
                self.claude_to_openai_request(request)
            }
            (ApiFormat::Claude, ApiFormat::Claude) => {
                Ok(serde_json::to_value(request)?)
            }
            (ApiFormat::Claude, ApiFormat::Gemini) => {
                let openai = self.claude_to_openai_request(request)?;
                let chat_request: ChatRequest = serde_json::from_value(openai)?;
                self.openai_to_gemini_request(&chat_request)
            }
            (ApiFormat::Claude, ApiFormat::Responses) => {
                let openai = self.claude_to_openai_request(request)?;
                let chat_request: ChatRequest = serde_json::from_value(openai)?;
                self.openai_to_responses_request(&chat_request)
            }
            (ApiFormat::Gemini, ApiFormat::OpenAI) => {
                self.gemini_to_openai_request(request)
            }
            (ApiFormat::Gemini, ApiFormat::Claude) => {
                let openai = self.gemini_to_openai_request(request)?;
                let chat_request: ChatRequest = serde_json::from_value(openai)?;
                self.openai_to_claude_request(&chat_request)
            }
            (ApiFormat::Gemini, ApiFormat::Gemini) => {
                Ok(serde_json::to_value(request)?)
            }
            (ApiFormat::Gemini, ApiFormat::Responses) => {
                let openai = self.gemini_to_openai_request(request)?;
                let chat_request: ChatRequest = serde_json::from_value(openai)?;
                self.openai_to_responses_request(&chat_request)
            }
            (ApiFormat::Responses, ApiFormat::OpenAI) => {
                self.responses_to_openai_request(request)
            }
            (ApiFormat::Responses, ApiFormat::Claude) => {
                let openai = self.responses_to_openai_request(request)?;
                let chat_request: ChatRequest = serde_json::from_value(openai)?;
                self.openai_to_claude_request(&chat_request)
            }
            (ApiFormat::Responses, ApiFormat::Gemini) => {
                let openai = self.responses_to_openai_request(request)?;
                let chat_request: ChatRequest = serde_json::from_value(openai)?;
                self.openai_to_gemini_request(&chat_request)
            }
            (ApiFormat::Responses, ApiFormat::Responses) => {
                Ok(serde_json::to_value(request)?)
            }
        }
    }

    fn translate_response(
        &self,
        source: ApiFormat,
        target: ApiFormat,
        response: &Value,
    ) -> Result<Value, TranslatorError> {
        match (source, target) {
            (ApiFormat::OpenAI, ApiFormat::OpenAI) => Ok(response.clone()),
            (ApiFormat::OpenAI, ApiFormat::Claude) => {
                self.openai_to_claude_response(response)
            }
            (ApiFormat::Claude, ApiFormat::OpenAI) => {
                self.claude_to_openai_response(response)
            }
            (ApiFormat::Claude, ApiFormat::Claude) => Ok(response.clone()),
            (ApiFormat::Gemini, ApiFormat::OpenAI) => {
                self.gemini_to_openai_response(response)
            }
            (ApiFormat::OpenAI, ApiFormat::Gemini) => {
                Ok(response.clone())
            }
            (ApiFormat::Responses, ApiFormat::OpenAI) => {
                self.responses_to_openai_response(response)
            }
            (ApiFormat::Responses, ApiFormat::Claude) => {
                self.responses_to_claude_response(response)
            }
            (ApiFormat::OpenAI, ApiFormat::Responses) => {
                self.openai_to_responses_response(response)
            }
            (ApiFormat::Claude, ApiFormat::Responses) => {
                self.claude_to_responses_response(response)
            }
            _ => Ok(response.clone()),
        }
    }

    fn translate_stream_chunk(
        &self,
        source: ApiFormat,
        target: ApiFormat,
        chunk: &Value,
        state: &mut StreamState,
    ) -> Result<Option<Vec<Value>>, TranslatorError> {
        match (source, target) {
            (ApiFormat::Claude, ApiFormat::OpenAI) => {
                self.claude_to_openai_stream_chunk(chunk, state)
            }
            (ApiFormat::Gemini, ApiFormat::OpenAI) => {
                self.gemini_to_openai_stream_chunk(chunk, state)
            }
            (ApiFormat::Responses, ApiFormat::OpenAI) => {
                self.responses_to_openai_stream_chunk(chunk, state)
            }
            (ApiFormat::Responses, ApiFormat::Claude) => {
                self.responses_to_claude_stream_chunk(chunk, state)
            }
            _ => Ok(Some(vec![chunk.clone()])),
        }
    }
}

impl DefaultTranslator {
    // ============ OpenAI → Claude ============

    fn openai_to_claude_request(&self, request: &ChatRequest) -> Result<Value, TranslatorError> {
        let system: Vec<Value> = request
            .messages
            .iter()
            .filter(|m| m.role == "system")
            .filter_map(|m| match &m.content {
                MessageContent::Text(text) => Some(json!({"type": "text", "text": text})),
                MessageContent::Parts(parts) => {
                    let text: String = parts.iter().filter_map(|p| p.text.clone()).collect();
                    if text.is_empty() { None } else { Some(json!({"type": "text", "text": text})) }
                }
            })
            .collect();

        let messages: Vec<Value> = request
            .messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| self.convert_message_to_claude(m))
            .collect();

        let mut result = json!({
            "model": request.model,
            "max_tokens": request.max_tokens.unwrap_or(4096),
            "messages": messages,
        });

        if !system.is_empty() {
            result["system"] = json!(system);
        }
        if request.stream {
            result["stream"] = json!(true);
        }
        if let Some(temp) = request.temperature {
            result["temperature"] = json!(temp);
        }
        if !request.tools.is_empty() {
            let tools: Vec<Value> = request.tools.iter()
                .filter(|t| t.tool_type == "function")
                .filter_map(|t| {
                    t.function.as_ref().map(|f| json!({
                        "name": f.name,
                        "description": f.description,
                        "input_schema": f.parameters
                    }))
                })
                .collect();
            result["tools"] = json!(tools);
        }

        Ok(result)
    }

    fn convert_message_to_claude(&self, msg: &Message) -> Value {
        let role = if msg.role == "assistant" { "assistant" } else { "user" };

        let content = match &msg.content {
            MessageContent::Text(text) => json!([{"type": "text", "text": text}]),
            MessageContent::Parts(parts) => {
                let converted: Vec<Value> = parts.iter().map(|p| {
                    if p.content_type == "text" {
                        json!({"type": "text", "text": p.text})
                    } else if p.content_type == "image_url" {
                        if let Some(ref img) = p.image_url {
                            self.parse_image_url(&img.url)
                        } else {
                            json!({"type": "text", "text": ""})
                        }
                    } else {
                        json!({"type": &p.content_type})
                    }
                }).collect();
                json!(converted)
            }
        };

        if !msg.tool_calls.is_empty() {
            let mut content_arr = content.as_array().cloned().unwrap_or_default();
            for tc in &msg.tool_calls {
                let input: Value = serde_json::from_str(&tc.function.arguments).unwrap_or(json!({}));
                content_arr.push(json!({
                    "type": "tool_use",
                    "id": tc.id,
                    "name": tc.function.name,
                    "input": input
                }));
            }
            return json!({"role": role, "content": content_arr});
        }

        json!({"role": role, "content": content})
    }

    fn parse_image_url(&self, url: &str) -> Value {
        if url.starts_with("data:") && url.contains(";base64,") {
            if let Some(idx) = url.find(";base64,") {
                let media_type = &url[5..idx];
                let data = &url[idx + 8..];
                return json!({
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": media_type,
                        "data": data
                    }
                });
            }
        }
        json!({"type": "text", "text": ""})
    }

    fn openai_to_claude_response(&self, response: &Value) -> Result<Value, TranslatorError> {
        let id = response.get("id").and_then(|v| v.as_str()).unwrap_or("msg_unknown");
        let model = response.get("model").and_then(|v| v.as_str()).unwrap_or("unknown");
        let content_text = response.get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("");

        let usage = response.get("usage").cloned().unwrap_or(json!({}));

        Ok(json!({
            "id": id,
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": content_text}],
            "model": model,
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": usage.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                "output_tokens": usage.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
            }
        }))
    }

    // ============ Claude → OpenAI ============

    fn claude_to_openai_request(&self, request: &ChatRequest) -> Result<Value, TranslatorError> {
        Ok(serde_json::to_value(request)?)
    }

    fn claude_to_openai_response(&self, response: &Value) -> Result<Value, TranslatorError> {
        let id = response.get("id").and_then(|v| v.as_str()).unwrap_or("chatcmpl-unknown");
        let model = response.get("model").and_then(|v| v.as_str()).unwrap_or("unknown");

        let content = response.get("content")
            .and_then(|c| c.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|block| {
                        if block.get("type")?.as_str()? == "text" {
                            block.get("text")?.as_str()
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();

        let input_tokens = response.get("usage")
            .and_then(|u| u.get("input_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        let output_tokens = response.get("usage")
            .and_then(|u| u.get("output_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        let stop_reason = response.get("stop_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("end_turn");

        let finish_reason = match stop_reason {
            "end_turn" => "stop",
            "max_tokens" => "length",
            "tool_use" => "tool_calls",
            _ => "stop",
        };

        Ok(json!({
            "id": id,
            "object": "chat.completion",
            "created": chrono::Utc::now().timestamp(),
            "model": model,
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": content},
                "finish_reason": finish_reason
            }],
            "usage": {
                "prompt_tokens": input_tokens,
                "completion_tokens": output_tokens,
                "total_tokens": input_tokens + output_tokens
            }
        }))
    }

    fn claude_to_openai_stream_chunk(
        &self,
        chunk: &Value,
        state: &mut StreamState,
    ) -> Result<Option<Vec<Value>>, TranslatorError> {
        let event = chunk.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let mut results = Vec::new();

        match event {
            "message_start" => {
                state.message_id = chunk.get("message").and_then(|m| m.get("id"))
                    .and_then(|v| v.as_str()).map(|s| s.to_string());
                state.model = chunk.get("message").and_then(|m| m.get("model"))
                    .and_then(|v| v.as_str()).map(|s| s.to_string());
                state.tool_call_index = 0;
                results.push(self.create_chunk(state, json!({"role": "assistant"}), None));
            }
            "content_block_start" => {
                if let Some(block) = chunk.get("content_block") {
                    if block.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                        let idx = state.tool_call_index;
                        state.tool_call_index += 1;
                        results.push(self.create_chunk(state, json!({
                            "tool_calls": [{
                                "index": idx,
                                "id": block.get("id"),
                                "type": "function",
                                "function": {"name": block.get("name"), "arguments": ""}
                            }]
                        }), None));
                    }
                }
            }
            "content_block_delta" => {
                if let Some(delta) = chunk.get("delta") {
                    match delta.get("type").and_then(|v| v.as_str()) {
                        Some("text_delta") => {
                            if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                                results.push(self.create_chunk(state, json!({"content": text}), None));
                            }
                        }
                        Some("input_json_delta") => {
                            if let Some(partial) = delta.get("partial_json").and_then(|v| v.as_str()) {
                                results.push(self.create_chunk(state, json!({
                                    "tool_calls": [{"index": 0, "function": {"arguments": partial}}]
                                }), None));
                            }
                        }
                        _ => {}
                    }
                }
            }
            "message_delta" => {
                if let Some(usage) = chunk.get("usage") {
                    state.usage = Some(usage.clone());
                }
                if let Some(stop) = chunk.get("delta").and_then(|d| d.get("stop_reason")).and_then(|v| v.as_str()) {
                    let finish_reason = match stop {
                        "end_turn" => "stop",
                        "tool_use" => "tool_calls",
                        _ => "stop",
                    };
                    state.finish_reason = Some(finish_reason.to_string());
                    let mut final_chunk = self.create_chunk(state, json!({}), Some(finish_reason));
                    if let Some(ref usage) = state.usage {
                        final_chunk["usage"] = json!({
                            "prompt_tokens": usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                            "completion_tokens": usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
                        });
                    }
                    results.push(final_chunk);
                    state.finish_reason_sent = true;
                }
            }
            "message_stop" => {
                if !state.finish_reason_sent {
                    results.push(self.create_chunk(state, json!({}), Some("stop")));
                }
            }
            _ => {}
        }

        if results.is_empty() { Ok(None) } else { Ok(Some(results)) }
    }

    fn create_chunk(&self, state: &StreamState, delta: Value, finish_reason: Option<&str>) -> Value {
        json!({
            "id": format!("chatcmpl-{}", state.message_id.as_deref().unwrap_or("unknown")),
            "object": "chat.completion.chunk",
            "created": chrono::Utc::now().timestamp(),
            "model": state.model.as_deref().unwrap_or("unknown"),
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish_reason}]
        })
    }

    // ============ OpenAI ↔ Gemini ============

    fn openai_to_gemini_request(&self, request: &ChatRequest) -> Result<Value, TranslatorError> {
        let contents: Vec<Value> = request.messages.iter().map(|m| {
            let role = match m.role.as_str() {
                "assistant" => "model",
                _ => "user",
            };
            let text = match &m.content {
                MessageContent::Text(t) => t.clone(),
                MessageContent::Parts(parts) => parts.iter().filter_map(|p| p.text.clone()).collect(),
            };
            json!({"role": role, "parts": [{"text": text}]})
        }).collect();

        let mut result = json!({"contents": contents});

        if let Some(temp) = request.temperature {
            result["generationConfig"] = json!({"temperature": temp, "maxOutputTokens": request.max_tokens});
        }

        Ok(result)
    }

    fn gemini_to_openai_request(&self, request: &ChatRequest) -> Result<Value, TranslatorError> {
        Ok(serde_json::to_value(request)?)
    }

    fn gemini_to_openai_response(&self, response: &Value) -> Result<Value, TranslatorError> {
        let content = response.get("candidates")
            .and_then(|c| c.as_array()).and_then(|arr| arr.first())
            .and_then(|c| c.get("content")).and_then(|c| c.get("parts"))
            .and_then(|p| p.as_array()).and_then(|parts| parts.first())
            .and_then(|p| p.get("text")).and_then(|t| t.as_str())
            .unwrap_or("");

        Ok(json!({
            "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
            "object": "chat.completion",
            "created": chrono::Utc::now().timestamp(),
            "model": "gemini",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": content}, "finish_reason": "stop"}],
            "usage": {
                "prompt_tokens": response.get("usageMetadata").and_then(|u| u.get("promptTokenCount")).and_then(|v| v.as_u64()).unwrap_or(0),
                "completion_tokens": response.get("usageMetadata").and_then(|u| u.get("candidatesTokenCount")).and_then(|v| v.as_u64()).unwrap_or(0)
            }
        }))
    }

    fn gemini_to_openai_stream_chunk(
        &self,
        chunk: &Value,
        state: &mut StreamState,
    ) -> Result<Option<Vec<Value>>, TranslatorError> {
        if let Some(candidates) = chunk.get("candidates").and_then(|v| v.as_array()) {
            let mut results = Vec::new();
            for candidate in candidates {
                if let Some(parts) = candidate.get("content")
                    .and_then(|c| c.get("parts")).and_then(|p| p.as_array())
                {
                    for part in parts {
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            results.push(self.create_chunk(state, json!({"content": text}), None));
                        }
                    }
                }
            }
            if !results.is_empty() { return Ok(Some(results)); }
        }
        Ok(None)
    }

    // ============ OpenAI ↔ Responses API ============

    /// OpenAI Chat Completions → Responses API 请求
    fn openai_to_responses_request(&self, request: &ChatRequest) -> Result<Value, TranslatorError> {
        let mut input: Vec<Value> = Vec::new();
        let mut instructions: Option<String> = None;

        for msg in &request.messages {
            match msg.role.as_str() {
                "system" => {
                    // 第一个 system 消息作为 instructions
                    if instructions.is_none() {
                        instructions = Some(match &msg.content {
                            MessageContent::Text(t) => t.clone(),
                            MessageContent::Parts(parts) => {
                                parts.iter().filter_map(|p| p.text.clone()).collect()
                            }
                        });
                    }
                }
                "user" => {
                    let content = self.convert_content_to_responses(&msg.content, "input_text");
                    input.push(json!({
                        "type": "message",
                        "role": "user",
                        "content": content
                    }));
                }
                "assistant" => {
                    // 处理 assistant 消息内容
                    let content = self.convert_content_to_responses(&msg.content, "output_text");
                    if !content.is_empty() {
                        input.push(json!({
                            "type": "message",
                            "role": "assistant",
                            "content": content
                        }));
                    }
                    // 处理 tool_calls
                    for tc in &msg.tool_calls {
                        input.push(json!({
                            "type": "function_call",
                            "call_id": tc.id,
                            "name": tc.function.name,
                            "arguments": tc.function.arguments
                        }));
                    }
                }
                "tool" => {
                    let output = match &msg.content {
                        MessageContent::Text(t) => t.clone(),
                        MessageContent::Parts(parts) => {
                            parts.iter().filter_map(|p| p.text.clone()).collect()
                        }
                    };
                    input.push(json!({
                        "type": "function_call_output",
                        "call_id": msg.tool_call_id,
                        "output": output
                    }));
                }
                _ => {}
            }
        }

        let mut result = json!({
            "model": request.model,
            "input": input,
            "stream": request.stream,
        });

        if let Some(instr) = instructions {
            result["instructions"] = json!(instr);
        }

        if let Some(max_tokens) = request.max_tokens {
            result["max_output_tokens"] = json!(max_tokens);
        }

        if let Some(temp) = request.temperature {
            result["temperature"] = json!(temp);
        }

        // 转换 tools
        if !request.tools.is_empty() {
            let tools: Vec<Value> = request.tools.iter()
                .filter(|t| t.tool_type == "function")
                .filter_map(|t| {
                    t.function.as_ref().map(|f| json!({
                        "type": "function",
                        "name": f.name,
                        "description": f.description,
                        "parameters": f.parameters
                    }))
                })
                .collect();
            result["tools"] = json!(tools);
        }

        Ok(result)
    }

    fn convert_content_to_responses(&self, content: &MessageContent, text_type: &str) -> Vec<Value> {
        match content {
            MessageContent::Text(t) if !t.is_empty() => {
                vec![json!({"type": text_type, "text": t})]
            }
            MessageContent::Parts(parts) => {
                parts.iter().filter_map(|p| {
                    if p.content_type == "text" {
                        p.text.as_ref().map(|t| json!({"type": text_type, "text": t}))
                    } else if p.content_type == "image_url" {
                        p.image_url.as_ref().map(|img| json!({
                            "type": "input_image",
                            "image_url": img.url
                        }))
                    } else {
                        None
                    }
                }).collect()
            }
            _ => vec![]
        }
    }

    /// Responses API → OpenAI Chat Completions 请求
    fn responses_to_openai_request(&self, request: &ChatRequest) -> Result<Value, TranslatorError> {
        // 从 extra 字段解析 Responses API 格式
        let input = request.extra.get("input");
        let instructions = request.extra.get("instructions");

        let mut messages: Vec<Value> = Vec::new();

        // 处理 instructions 作为 system 消息
        if let Some(instr) = instructions.and_then(|v| v.as_str()) {
            if !instr.is_empty() {
                messages.push(json!({"role": "system", "content": instr}));
            }
        }

        // 处理 input 数组
        if let Some(input_arr) = input.and_then(|v| v.as_array()) {
            for item in input_arr {
                let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("message");

                match item_type {
                    "message" => {
                        let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("user");
                        let content = self.convert_responses_content_to_openai(item.get("content"));
                        messages.push(json!({"role": role, "content": content}));
                    }
                    "function_call" => {
                        // 添加到上一个 assistant 消息或创建新的
                        let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let args = item.get("arguments").and_then(|v| v.as_str()).unwrap_or("{}");

                        // 检查最后一个消息是否是 assistant
                        let should_append = messages.last()
                            .map(|last| last.get("role").and_then(|v| v.as_str()) == Some("assistant"))
                            .unwrap_or(false);

                        if should_append {
                            if let Some(last) = messages.last_mut() {
                                if let Some(obj) = last.as_object_mut() {
                                    let tool_calls = obj.entry("tool_calls".to_string())
                                        .or_insert_with(|| json!([]));
                                    if let Some(arr) = tool_calls.as_array_mut() {
                                        arr.push(json!({
                                            "id": call_id,
                                            "type": "function",
                                            "function": {"name": name, "arguments": args}
                                        }));
                                    }
                                }
                            }
                        } else {
                            messages.push(json!({
                                "role": "assistant",
                                "content": null,
                                "tool_calls": [{
                                    "id": call_id,
                                    "type": "function",
                                    "function": {"name": name, "arguments": args}
                                }]
                            }));
                        }
                    }
                    "function_call_output" => {
                        let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                        let output = item.get("output").and_then(|v| v.as_str()).unwrap_or("");
                        messages.push(json!({
                            "role": "tool",
                            "tool_call_id": call_id,
                            "content": output
                        }));
                    }
                    _ => {}
                }
            }
        }

        let mut result = json!({
            "model": request.model,
            "messages": messages,
            "stream": request.stream,
        });

        if let Some(max_tokens) = request.max_tokens {
            result["max_tokens"] = json!(max_tokens);
        }

        if let Some(temp) = request.temperature {
            result["temperature"] = json!(temp);
        }

        Ok(result)
    }

    fn convert_responses_content_to_openai(&self, content: Option<&Value>) -> Value {
        match content {
            Some(Value::String(s)) => json!(s),
            Some(Value::Array(parts)) => {
                let text: String = parts.iter()
                    .filter_map(|p| {
                        let ptype = p.get("type").and_then(|v| v.as_str());
                        if ptype == Some("input_text") || ptype == Some("output_text") {
                            p.get("text").and_then(|t| t.as_str())
                        } else {
                            None
                        }
                    })
                    .collect();
                json!(text)
            }
            _ => json!("")
        }
    }

    /// Responses API 响应 → OpenAI Chat Completions 响应
    fn responses_to_openai_response(&self, response: &Value) -> Result<Value, TranslatorError> {
        let id = response.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
        let model = response.get("model").and_then(|v| v.as_str()).unwrap_or("unknown");

        // 从 output 数组提取内容
        let output = response.get("output").and_then(|v| v.as_array());
        let mut content = String::new();
        let mut tool_calls: Vec<Value> = Vec::new();
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
        let status = response.get("status").and_then(|v| v.as_str()).unwrap_or("completed");
        let finish_reason = match status {
            "completed" => if has_tool_use { "tool_calls" } else { "stop" },
            "incomplete" => "length",
            _ => "stop",
        };

        // 处理 usage
        let usage = response.get("usage").cloned().unwrap_or(json!({}));
        let prompt_tokens = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as i32;
        let completion_tokens = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as i32;

        let message = if !tool_calls.is_empty() {
            json!({
                "role": "assistant",
                "content": if content.is_empty() { Value::Null } else { json!(content) },
                "tool_calls": tool_calls
            })
        } else {
            json!({"role": "assistant", "content": content})
        };

        Ok(json!({
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
        }))
    }

    /// Responses API 响应 → Claude 响应
    fn responses_to_claude_response(&self, response: &Value) -> Result<Value, TranslatorError> {
        let id = response.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
        let model = response.get("model").and_then(|v| v.as_str()).unwrap_or("unknown");

        let output = response.get("output").and_then(|v| v.as_array());
        let mut content: Vec<Value> = Vec::new();
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
                                        if !text.is_empty() {
                                            content.push(json!({"type": "text", "text": text}));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    "function_call" => {
                        let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let args = item.get("arguments").and_then(|v| v.as_str()).unwrap_or("{}");
                        let input: Value = serde_json::from_str(args).unwrap_or(json!({}));

                        content.push(json!({
                            "type": "tool_use",
                            "id": call_id,
                            "name": name,
                            "input": input
                        }));
                        has_tool_use = true;
                    }
                    "reasoning" => {
                        // 处理 reasoning summary → thinking
                        if let Some(summary) = item.get("summary").and_then(|s| s.as_array()) {
                            let thinking_text: String = summary.iter()
                                .filter_map(|s| {
                                    if s.get("type").and_then(|t| t.as_str()) == Some("summary_text") {
                                        s.get("text").and_then(|t| t.as_str())
                                    } else {
                                        None
                                    }
                                })
                                .collect();

                            if !thinking_text.is_empty() {
                                content.push(json!({
                                    "type": "thinking",
                                    "thinking": thinking_text
                                }));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        let status = response.get("status").and_then(|v| v.as_str()).unwrap_or("completed");
        let stop_reason = match status {
            "completed" => if has_tool_use { "tool_use" } else { "end_turn" },
            "incomplete" => "max_tokens",
            _ => "end_turn",
        };

        let usage = response.get("usage").cloned().unwrap_or(json!({}));

        Ok(json!({
            "id": id,
            "type": "message",
            "role": "assistant",
            "content": content,
            "model": model,
            "stop_reason": stop_reason,
            "stop_sequence": Value::Null,
            "usage": {
                "input_tokens": usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                "output_tokens": usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
            }
        }))
    }

    /// OpenAI 响应 → Responses API 响应
    fn openai_to_responses_response(&self, response: &Value) -> Result<Value, TranslatorError> {
        let id = response.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
        let model = response.get("model").and_then(|v| v.as_str()).unwrap_or("unknown");

        let choices = response.get("choices").and_then(|c| c.as_array());
        let mut output: Vec<Value> = Vec::new();

        let finish_reason = choices
            .and_then(|arr| arr.first())
            .and_then(|c| c.get("finish_reason"))
            .and_then(|v| v.as_str())
            .unwrap_or("stop");

        let status = Self::finish_reason_to_status(finish_reason);

        if let Some(choice) = choices.and_then(|arr| arr.first()) {
            if let Some(message) = choice.get("message") {
                let mut msg_content: Vec<Value> = Vec::new();

                if let Some(text) = message.get("content").and_then(|c| c.as_str()) {
                    if !text.is_empty() {
                        msg_content.push(Self::build_output_text(text));
                    }
                }

                if let Some(tool_calls) = message.get("tool_calls").and_then(|t| t.as_array()) {
                    for tc in tool_calls {
                        let call_id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        let name = tc.get("function").and_then(|f| f.get("name")).and_then(|v| v.as_str()).unwrap_or("");
                        let args = tc.get("function").and_then(|f| f.get("arguments")).and_then(|v| v.as_str()).unwrap_or("{}");
                        output.push(Self::build_responses_function_call(call_id, name, args));
                    }
                }

                if !msg_content.is_empty() {
                    output.push(Self::build_responses_message(id, status, msg_content));
                }
            }
        }

        let usage = response.get("usage").cloned().unwrap_or(json!({}));
        let created_at = chrono::Utc::now().timestamp();
        let input_tokens = usage.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let output_tokens = usage.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let completed_at = if status == "completed" { Some(created_at + 1) } else { None };

        Ok(json!({
            "id": format!("resp_{}", id),
            "object": "response",
            "created_at": created_at,
            "status": status,
            "completed_at": completed_at,
            "model": model,
            "output": output,
            "usage": Self::build_responses_usage(input_tokens, output_tokens)
        }))
    }

    /// Claude 响应 → Responses API 响应
    fn claude_to_responses_response(&self, response: &Value) -> Result<Value, TranslatorError> {
        let id = response.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
        let model = response.get("model").and_then(|v| v.as_str()).unwrap_or("unknown");

        let content = response.get("content").and_then(|c| c.as_array());
        let mut output: Vec<Value> = Vec::new();
        let mut msg_content: Vec<Value> = Vec::new();

        if let Some(blocks) = content {
            for block in blocks {
                let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");

                match block_type {
                    "text" => {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                msg_content.push(Self::build_output_text(text));
                            }
                        }
                    }
                    "tool_use" => {
                        let call_id = block.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let input = block.get("input").cloned().unwrap_or(json!({}));
                        let args = serde_json::to_string(&input).unwrap_or_default();
                        output.push(Self::build_responses_function_call(call_id, name, &args));
                    }
                    "thinking" => {
                        if let Some(thinking) = block.get("thinking").and_then(|t| t.as_str()) {
                            output.push(json!({
                                "type": "reasoning",
                                "id": format!("rs_{}", uuid::Uuid::new_v4()),
                                "summary": [{"type": "summary_text", "text": thinking}]
                            }));
                        }
                    }
                    _ => {}
                }
            }
        }

        let stop_reason = response.get("stop_reason").and_then(|v| v.as_str()).unwrap_or("end_turn");
        let status = Self::stop_reason_to_status(stop_reason);

        if !msg_content.is_empty() {
            output.push(Self::build_responses_message(id, status, msg_content));
        }

        let usage = response.get("usage").cloned().unwrap_or(json!({}));
        let created_at = chrono::Utc::now().timestamp();
        let input_tokens = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let output_tokens = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let completed_at = if status == "completed" { Some(created_at + 1) } else { None };

        Ok(json!({
            "id": format!("resp_{}", id),
            "object": "response",
            "created_at": created_at,
            "status": status,
            "completed_at": completed_at,
            "model": model,
            "output": output,
            "usage": Self::build_responses_usage(input_tokens, output_tokens)
        }))
    }

    /// Responses API 流式 → OpenAI 流式
    fn responses_to_openai_stream_chunk(
        &self,
        chunk: &Value,
        state: &mut StreamState,
    ) -> Result<Option<Vec<Value>>, TranslatorError> {
        let event_type = chunk.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let mut results = Vec::new();

        match event_type {
            "response.created" | "response.in_progress" => {
                // 初始化响应
                if let Some(resp) = chunk.get("response") {
                    state.message_id = resp.get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
                    state.model = resp.get("model").and_then(|v| v.as_str()).map(|s| s.to_string());
                }
                if event_type == "response.created" {
                    results.push(self.create_chunk(state, json!({"role": "assistant"}), None));
                }
            }
            "response.output_text.delta" => {
                // 文本增量
                if let Some(delta) = chunk.get("delta").and_then(|v| v.as_str()) {
                    results.push(self.create_chunk(state, json!({"content": delta}), None));
                }
            }
            "response.output_item.added" => {
                // 工具调用开始
                if let Some(item) = chunk.get("item") {
                    if item.get("type").and_then(|v| v.as_str()) == Some("function_call") {
                        let idx = state.tool_call_index;
                        state.tool_call_index += 1;
                        let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");

                        results.push(self.create_chunk(state, json!({
                            "tool_calls": [{
                                "index": idx,
                                "id": call_id,
                                "type": "function",
                                "function": {"name": name, "arguments": ""}
                            }]
                        }), None));
                    }
                }
            }
            "response.function_call_arguments.delta" => {
                // 工具参数增量
                if let Some(delta) = chunk.get("delta").and_then(|v| v.as_str()) {
                    let idx = state.tool_call_index.saturating_sub(1);
                    results.push(self.create_chunk(state, json!({
                        "tool_calls": [{
                            "index": idx,
                            "function": {"arguments": delta}
                        }]
                    }), None));
                }
            }
            "response.completed" => {
                // 响应完成
                let had_tool_calls = state.tool_call_index > 0;
                let finish_reason = if had_tool_calls { "tool_calls" } else { "stop" };

                // 处理 usage
                if let Some(resp) = chunk.get("response").and_then(|r| r.get("usage")) {
                    state.usage = Some(resp.clone());
                }

                let mut final_chunk = self.create_chunk(state, json!({}), Some(finish_reason));
                if let Some(ref usage) = state.usage {
                    final_chunk["usage"] = json!({
                        "prompt_tokens": usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                        "completion_tokens": usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
                    });
                }
                results.push(final_chunk);
                state.finish_reason_sent = true;
            }
            "error" => {
                // 错误处理
                if let Some(err) = chunk.get("error") {
                    let msg = err.get("message").and_then(|v| v.as_str()).unwrap_or("Unknown error");
                    return Err(TranslatorError::InvalidResponse(msg.to_string()));
                }
            }
            _ => {}
        }

        if results.is_empty() { Ok(None) } else { Ok(Some(results)) }
    }

    /// Responses API 流式 → Claude 流式
    fn responses_to_claude_stream_chunk(
        &self,
        chunk: &Value,
        state: &mut StreamState,
    ) -> Result<Option<Vec<Value>>, TranslatorError> {
        let event_type = chunk.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let mut results = Vec::new();

        match event_type {
            "response.created" => {
                if let Some(resp) = chunk.get("response") {
                    state.message_id = resp.get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
                    state.model = resp.get("model").and_then(|v| v.as_str()).map(|s| s.to_string());
                }
                results.push(json!({
                    "type": "message_start",
                    "message": {
                        "id": state.message_id,
                        "type": "message",
                        "role": "assistant",
                        "model": state.model,
                        "content": []
                    }
                }));
            }
            "response.output_text.delta" => {
                if let Some(delta) = chunk.get("delta").and_then(|v| v.as_str()) {
                    let idx = state.current_block_index.unwrap_or(0);
                    results.push(json!({
                        "type": "content_block_delta",
                        "index": idx,
                        "delta": {"type": "text_delta", "text": delta}
                    }));
                }
            }
            "response.output_item.added" => {
                if let Some(item) = chunk.get("item") {
                    match item.get("type").and_then(|v| v.as_str()) {
                        Some("message") => {
                            let idx = state.current_block_index.unwrap_or(0);
                            state.current_block_index = Some(idx + 1);
                            results.push(json!({
                                "type": "content_block_start",
                                "index": idx,
                                "content_block": {"type": "text", "text": ""}
                            }));
                        }
                        Some("function_call") => {
                            let idx = state.current_block_index.unwrap_or(0);
                            state.current_block_index = Some(idx + 1);
                            let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                            let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
                            results.push(json!({
                                "type": "content_block_start",
                                "index": idx,
                                "content_block": {
                                    "type": "tool_use",
                                    "id": call_id,
                                    "name": name,
                                    "input": {}
                                }
                            }));
                        }
                        _ => {}
                    }
                }
            }
            "response.function_call_arguments.delta" => {
                if let Some(delta) = chunk.get("delta").and_then(|v| v.as_str()) {
                    let idx = state.current_block_index.unwrap_or(0).saturating_sub(1);
                    results.push(json!({
                        "type": "content_block_delta",
                        "index": idx,
                        "delta": {"type": "input_json_delta", "partial_json": delta}
                    }));
                }
            }
            "response.completed" => {
                let had_tool_calls = state.tool_call_index > 0;
                let stop_reason = if had_tool_calls { "tool_use" } else { "end_turn" };

                if let Some(resp) = chunk.get("response").and_then(|r| r.get("usage")) {
                    state.usage = Some(resp.clone());
                }

                results.push(json!({
                    "type": "message_delta",
                    "delta": {"stop_reason": stop_reason},
                    "usage": {
                        "input_tokens": state.usage.as_ref().and_then(|u| u.get("input_tokens").and_then(|v| v.as_u64())).unwrap_or(0),
                        "output_tokens": state.usage.as_ref().and_then(|u| u.get("output_tokens").and_then(|v| v.as_u64())).unwrap_or(0)
                    }
                }));

                results.push(json!({"type": "message_stop"}));
                state.finish_reason_sent = true;
            }
            _ => {}
        }

        if results.is_empty() { Ok(None) } else { Ok(Some(results)) }
    }
}

impl Default for DefaultTranslator {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Message, MessageContent};

    fn create_translator() -> DefaultTranslator {
        DefaultTranslator::new()
    }

    fn create_chat_request(model: &str, messages: Vec<Message>) -> ChatRequest {
        ChatRequest {
            model: model.to_string(),
            messages,
            stream: false,
            temperature: None,
            max_tokens: Some(1024),
            tools: vec![],
            top_p: None,
            n: None,
            stop: None,
            presence_penalty: None,
            frequency_penalty: None,
            logit_bias: None,
            user: None,
            tool_choice: None,
            parallel_tool_calls: None,
            response_format: None,
            seed: None,
            logprobs: None,
            top_logprobs: None,
            extra: serde_json::Map::new(),
        }
    }

    #[test]
    fn test_responses_to_openai_response_simple() {
        let translator = create_translator();
        let response = json!({
            "id": "resp_123",
            "object": "response",
            "status": "completed",
            "model": "gpt-4o",
            "output": [{
                "type": "message",
                "id": "msg_123",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "Hello!"}]
            }],
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });

        let result = translator.translate_response(
            ApiFormat::Responses,
            ApiFormat::OpenAI,
            &response,
        ).unwrap();

        assert_eq!(result["id"], "resp_123");
        assert_eq!(result["object"], "chat.completion");
        assert_eq!(result["choices"][0]["message"]["role"], "assistant");
        assert_eq!(result["choices"][0]["message"]["content"], "Hello!");
        assert_eq!(result["choices"][0]["finish_reason"], "stop");
        assert_eq!(result["usage"]["prompt_tokens"], 10);
        assert_eq!(result["usage"]["completion_tokens"], 5);
    }

    #[test]
    fn test_responses_to_openai_response_with_function_call() {
        let translator = create_translator();
        let response = json!({
            "id": "resp_123",
            "object": "response",
            "status": "completed",
            "model": "gpt-4o",
            "output": [{
                "type": "function_call",
                "id": "fc_123",
                "call_id": "call_123",
                "name": "get_weather",
                "arguments": "{\"location\": \"Tokyo\"}"
            }],
            "usage": {"input_tokens": 10, "output_tokens": 15}
        });

        let result = translator.translate_response(
            ApiFormat::Responses,
            ApiFormat::OpenAI,
            &response,
        ).unwrap();

        assert_eq!(result["choices"][0]["message"]["tool_calls"][0]["id"], "call_123");
        assert_eq!(result["choices"][0]["message"]["tool_calls"][0]["function"]["name"], "get_weather");
        assert_eq!(result["choices"][0]["finish_reason"], "tool_calls");
    }

    #[test]
    fn test_responses_to_claude_response_simple() {
        let translator = create_translator();
        let response = json!({
            "id": "resp_123",
            "object": "response",
            "status": "completed",
            "model": "gpt-4o",
            "output": [{
                "type": "message",
                "content": [{"type": "output_text", "text": "Hello!"}]
            }],
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });

        let result = translator.translate_response(
            ApiFormat::Responses,
            ApiFormat::Claude,
            &response,
        ).unwrap();

        assert_eq!(result["id"], "resp_123");
        assert_eq!(result["type"], "message");
        assert_eq!(result["role"], "assistant");
        assert_eq!(result["content"][0]["type"], "text");
        assert_eq!(result["content"][0]["text"], "Hello!");
        assert_eq!(result["stop_reason"], "end_turn");
    }

    #[test]
    fn test_responses_to_claude_response_with_function_call() {
        let translator = create_translator();
        let response = json!({
            "id": "resp_123",
            "status": "completed",
            "model": "gpt-4o",
            "output": [{
                "type": "function_call",
                "call_id": "call_123",
                "name": "get_weather",
                "arguments": "{\"location\": \"Tokyo\"}"
            }],
            "usage": {"input_tokens": 10, "output_tokens": 15}
        });

        let result = translator.translate_response(
            ApiFormat::Responses,
            ApiFormat::Claude,
            &response,
        ).unwrap();

        assert_eq!(result["content"][0]["type"], "tool_use");
        assert_eq!(result["content"][0]["id"], "call_123");
        assert_eq!(result["content"][0]["name"], "get_weather");
        assert_eq!(result["content"][0]["input"]["location"], "Tokyo");
        assert_eq!(result["stop_reason"], "tool_use");
    }

    #[test]
    fn test_openai_to_responses_request_simple() {
        let translator = create_translator();
        let request = create_chat_request("gpt-4o", vec![
            Message {
                role: "system".to_string(),
                content: MessageContent::Text("You are helpful.".to_string()),
                name: None,
                tool_calls: vec![],
                tool_call_id: None,
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Text("Hello".to_string()),
                name: None,
                tool_calls: vec![],
                tool_call_id: None,
            },
        ]);

        let result = translator.translate_request(
            ApiFormat::OpenAI,
            ApiFormat::Responses,
            &request,
        ).unwrap();

        assert_eq!(result["model"], "gpt-4o");
        assert_eq!(result["instructions"], "You are helpful.");
        assert_eq!(result["input"][0]["type"], "message");
        assert_eq!(result["input"][0]["role"], "user");
        assert_eq!(result["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(result["input"][0]["content"][0]["text"], "Hello");
    }

    #[test]
    fn test_openai_to_responses_request_with_tool_calls() {
        let translator = create_translator();
        let request = create_chat_request("gpt-4o", vec![
            Message {
                role: "assistant".to_string(),
                content: MessageContent::Text("Let me check".to_string()),
                name: None,
                tool_calls: vec![crate::models::ToolCall {
                    id: "call_123".to_string(),
                    call_type: "function".to_string(),
                    function: crate::models::FunctionCall {
                        name: "get_weather".to_string(),
                        arguments: "{\"location\": \"Tokyo\"}".to_string(),
                    },
                }],
                tool_call_id: None,
            },
        ]);

        let result = translator.translate_request(
            ApiFormat::OpenAI,
            ApiFormat::Responses,
            &request,
        ).unwrap();

        // Should have message + function_call
        assert_eq!(result["input"].as_array().unwrap().len(), 2);
        assert_eq!(result["input"][0]["type"], "message");
        assert_eq!(result["input"][0]["role"], "assistant");
        assert_eq!(result["input"][1]["type"], "function_call");
        assert_eq!(result["input"][1]["call_id"], "call_123");
        assert_eq!(result["input"][1]["name"], "get_weather");
    }

    #[test]
    fn test_responses_incomplete_status() {
        let translator = create_translator();
        let response = json!({
            "id": "resp_123",
            "status": "incomplete",
            "model": "gpt-4o",
            "output": [{
                "type": "message",
                "content": [{"type": "output_text", "text": "Partial..."}]
            }],
            "usage": {"input_tokens": 10, "output_tokens": 4096}
        });

        let result = translator.translate_response(
            ApiFormat::Responses,
            ApiFormat::OpenAI,
            &response,
        ).unwrap();

        assert_eq!(result["choices"][0]["finish_reason"], "length");
    }

    #[test]
    fn test_responses_with_reasoning() {
        let translator = create_translator();
        let response = json!({
            "id": "resp_123",
            "status": "completed",
            "model": "gpt-4o",
            "output": [
                {
                    "type": "reasoning",
                    "id": "rs_123",
                    "summary": [{"type": "summary_text", "text": "Thinking..."}]
                },
                {
                    "type": "message",
                    "content": [{"type": "output_text", "text": "The answer is 42"}]
                }
            ],
            "usage": {"input_tokens": 10, "output_tokens": 20}
        });

        let result = translator.translate_response(
            ApiFormat::Responses,
            ApiFormat::Claude,
            &response,
        ).unwrap();

        assert_eq!(result["content"][0]["type"], "thinking");
        assert_eq!(result["content"][0]["thinking"], "Thinking...");
        assert_eq!(result["content"][1]["type"], "text");
        assert_eq!(result["content"][1]["text"], "The answer is 42");
    }
}

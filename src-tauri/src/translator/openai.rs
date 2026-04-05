use super::{Translator, TranslatorError};
use crate::models::{ApiFormat, ChatRequest, MessageContent};
use async_trait::async_trait;
use serde_json::{json, Value};

/// OpenAI format translator
pub struct OpenAITranslator {}

impl OpenAITranslator {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl Translator for OpenAITranslator {
    fn translate_request(
        &self,
        source: &ApiFormat,
        target: &ApiFormat,
        request: &ChatRequest,
    ) -> Result<Value, TranslatorError> {
        match (source, target) {
            (ApiFormat::OpenAI, ApiFormat::OpenAI) => {
                // No translation needed
                Ok(serde_json::to_value(request)?)
            }
            (ApiFormat::OpenAI, ApiFormat::Claude) => {
                // OpenAI -> Claude
                self.openai_to_claude_request(request)
            }
            (ApiFormat::Claude, ApiFormat::OpenAI) => {
                // Claude -> OpenAI
                self.claude_to_openai_request(request)
            }
            _ => Err(TranslatorError::UnsupportedConversion(
                format!("{:?}", source),
                format!("{:?}", target),
            )),
        }
    }

    fn translate_response(
        &self,
        source: &ApiFormat,
        target: &ApiFormat,
        response: &Value,
    ) -> Result<Value, TranslatorError> {
        match (source, target) {
            (ApiFormat::OpenAI, ApiFormat::OpenAI) => Ok(response.clone()),
            (ApiFormat::Claude, ApiFormat::OpenAI) => {
                self.claude_to_openai_response(response)
            }
            _ => Err(TranslatorError::UnsupportedConversion(
                format!("{:?}", source),
                format!("{:?}", target),
            )),
        }
    }
}

impl OpenAITranslator {
    fn openai_to_claude_request(&self, request: &ChatRequest) -> Result<Value, TranslatorError> {
        // Extract system message
        let system = request
            .messages
            .iter()
            .filter(|m| m.role == "system")
            .map(|m| match &m.content {
                MessageContent::Text(text) => text.clone(),
                MessageContent::Parts(parts) => parts
                    .iter()
                    .filter_map(|p| p.text.clone())
                    .collect::<Vec<_>>()
                    .join("\n"),
            })
            .collect::<Vec<_>>()
            .join("\n");

        // Convert messages
        let messages: Vec<Value> = request
            .messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| {
                let content = match &m.content {
                    MessageContent::Text(text) => {
                        json!([{ "type": "text", "text": text }])
                    }
                    MessageContent::Parts(parts) => {
                        let converted: Vec<Value> = parts
                            .iter()
                            .map(|p| {
                                if p.content_type == "text" {
                                    json!({ "type": "text", "text": p.text })
                                } else if p.content_type == "image_url" {
                                    json!({
                                        "type": "image",
                                        "source": {
                                            "type": "url",
                                            "url": p.image_url.as_ref().map(|i| &i.url).unwrap_or(&"".to_string())
                                        }
                                    })
                                } else {
                                    json!({ "type": &p.content_type })
                                }
                            })
                            .collect();
                        json!(converted)
                    }
                };

                json!({
                    "role": if m.role == "assistant" { "assistant" } else { "user" },
                    "content": content
                })
            })
            .collect();

        Ok(json!({
            "model": request.model,
            "max_tokens": request.max_tokens.unwrap_or(4096),
            "messages": messages,
            "system": if system.is_empty() { Value::Null } else { json!(system) },
            "stream": request.stream
        }))
    }

    fn claude_to_openai_request(&self, _request: &ChatRequest) -> Result<Value, TranslatorError> {
        // TODO: Implement Claude to OpenAI request conversion
        Err(TranslatorError::UnsupportedConversion(
            "claude".to_string(),
            "openai".to_string(),
        ))
    }

    fn claude_to_openai_response(&self, response: &Value) -> Result<Value, TranslatorError> {
        let id = response.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
        let model = response.get("model").and_then(|v| v.as_str()).unwrap_or("unknown");

        let content = response
            .get("content")
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

        let input_tokens = response
            .get("usage")
            .and_then(|u| u.get("input_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        let output_tokens = response
            .get("usage")
            .and_then(|u| u.get("output_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        let stop_reason = response
            .get("stop_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("stop");

        Ok(json!({
            "id": id,
            "object": "chat.completion",
            "created": chrono::Utc::now().timestamp(),
            "model": model,
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": content
                },
                "finish_reason": if stop_reason == "end_turn" { "stop" } else { stop_reason }
            }],
            "usage": {
                "prompt_tokens": input_tokens,
                "completion_tokens": output_tokens,
                "total_tokens": input_tokens + output_tokens
            }
        }))
    }
}

impl Default for OpenAITranslator {
    fn default() -> Self {
        Self::new()
    }
}

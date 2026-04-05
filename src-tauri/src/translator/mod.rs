pub mod converter;

pub use converter::*;

use crate::models::{ApiFormat, ChatRequest};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

/// 协议转换错误
#[derive(Debug, thiserror::Error)]
pub enum TranslatorError {
    #[error("Unsupported format conversion: {0} -> {1}")]
    UnsupportedConversion(String, String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Missing required field: {0}")]
    MissingField(String),
}

/// 协议转换器 trait
#[async_trait]
pub trait Translator: Send + Sync {
    /// 转换请求
    fn translate_request(
        &self,
        source: ApiFormat,
        target: ApiFormat,
        request: &ChatRequest,
    ) -> Result<Value, TranslatorError>;

    /// 转换响应
    fn translate_response(
        &self,
        source: ApiFormat,
        target: ApiFormat,
        response: &Value,
    ) -> Result<Value, TranslatorError>;

    /// 转换流式响应块
    fn translate_stream_chunk(
        &self,
        source: ApiFormat,
        target: ApiFormat,
        chunk: &Value,
        state: &mut StreamState,
    ) -> Result<Option<Vec<Value>>, TranslatorError>;
}

/// 流式响应状态
#[derive(Debug, Default)]
pub struct StreamState {
    pub message_id: Option<String>,
    pub model: Option<String>,
    pub tool_call_index: usize,
    pub tool_calls: HashMap<usize, Value>,
    pub finish_reason: Option<String>,
    pub finish_reason_sent: bool,
    pub in_thinking_block: bool,
    pub current_block_index: Option<usize>,
    pub usage: Option<Value>,
    pub accumulated_content: String,
    // Responses API 特有
    pub response_id: Option<String>,
    pub current_tool_call_id: Option<String>,
    pub tool_call_arguments_buffer: HashMap<usize, String>,
}

/// 获取协议转换器
pub fn get_translator() -> Box<dyn Translator> {
    Box::new(converter::DefaultTranslator::new())
}

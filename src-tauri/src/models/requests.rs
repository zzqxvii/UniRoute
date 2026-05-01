//! 请求模型：ChatRequest、ResponsesRequest、ClaudeMessagesRequest 等

use serde::{Deserialize, Serialize};

use super::entities::{
    Tool, ToolChoice, ResponseFormat,
};

// ============ ImageUrl ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrl {
    pub url: String,
    #[serde(default)] pub detail: Option<String>,
}

// ============ Chat Request ============

/// Chat Completions API 请求
/// 支持 OpenAI Chat Completions API 的完整参数
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(default)] pub stream: bool,
    #[serde(default)] pub temperature: Option<f32>,
    #[serde(default)] pub top_p: Option<f32>,
    #[serde(default)] pub n: Option<u32>,
    #[serde(default)] pub stop: Option<Vec<String>>,
    #[serde(default)] pub max_tokens: Option<u32>,
    #[serde(default)] pub presence_penalty: Option<f32>,
    #[serde(default)] pub frequency_penalty: Option<f32>,
    #[serde(default)] pub logit_bias: Option<std::collections::HashMap<String, f32>>,
    #[serde(default)] pub user: Option<String>,
    #[serde(default)] pub tools: Vec<Tool>,
    #[serde(default)] pub tool_choice: Option<ToolChoice>,
    #[serde(default)] pub parallel_tool_calls: Option<bool>,
    #[serde(default)] pub response_format: Option<ResponseFormat>,
    #[serde(default)] pub seed: Option<i64>,
    #[serde(default)] pub logprobs: Option<bool>,
    #[serde(default)] pub top_logprobs: Option<u32>,
    /// 捕获其他未知字段
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(default)] pub content: Option<serde_json::Value>,
    #[serde(default)] pub name: Option<String>,
    #[serde(default)] pub tool_calls: Option<Vec<serde_json::Value>>,
    #[serde(default)] pub tool_call_id: Option<String>,
}

// ============ Message（通用消息类型） ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: MessageContent,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")] pub tool_calls: Vec<super::entities::ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentPart {
    #[serde(rename = "type")] pub content_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub image_url: Option<ImageUrl>,
    #[serde(flatten, skip_serializing_if = "serde_json::Map::is_empty")] pub extra: serde_json::Map<String, serde_json::Value>,
}

// ============ Responses API ============

/// OpenAI Responses API 请求
/// https://platform.openai.com/docs/api-reference/responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesRequest {
    pub model: String,
    /// 输入内容：文本或消息项数组
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<ResponsesInput>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stream: bool,
    /// 系统指令（转换为 system message）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<Tool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
    /// 思考配置（无法映射到 Chat API）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningConfig>,
    /// 截断策略（无法映射到 Chat API）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncation: Option<String>,
    /// 会话 ID（无法映射到 Chat API）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    /// 用户标识
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// 元数据（无法映射到 Chat API）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    /// 服务层级（无法映射到 Chat API）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    /// 是否存储（无法映射到 Chat API）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,
    /// 捕获其他未知字段
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// 思考配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generate_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponsesInput {
    Text(String),
    Items(Vec<ResponsesItem>),
    /// 回退：捕获任何其他格式
    Raw(serde_json::Value),
}

/// Responses API 的输入项
/// 可以是 message 类型或其他类型，content 可以是字符串或数组
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesItem {
    #[serde(rename = "type", default = "default_message_type")]
    pub item_type: String,
    pub role: Option<String>,
    #[serde(default)]
    pub content: ResponsesContent,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

fn default_message_type() -> String { "message".to_string() }

/// Responses API 的内容，可以是字符串或内容数组
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponsesContent {
    Text(String),
    Parts(Vec<ResponsesContentPart>),
}

impl Default for ResponsesContent {
    fn default() -> Self {
        ResponsesContent::Text(String::new())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesContentPart {
    #[serde(rename = "type")]
    pub content_type: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub image_url: Option<ImageUrl>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ============ Claude Messages API ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeMessagesRequest {
    pub model: String,
    pub messages: Vec<ClaudeMessage>,
    #[serde(default)] pub max_tokens: i32,
    #[serde(default)] pub stream: bool,
    #[serde(default)] pub system: Option<Vec<ClaudeBlock>>,
    #[serde(default)] pub tools: Option<Vec<ClaudeTool>>,
    #[serde(default)] pub temperature: Option<f64>,
    #[serde(flatten)] pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeMessage {
    pub role: String,
    pub content: Vec<ClaudeContent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClaudeContent {
    Text { text: String },
    Image { source: ClaudeImageSource },
    ToolUse { id: String, name: String, input: serde_json::Value },
    ToolResult { tool_use_id: String, content: String },
    Blocks(Vec<ClaudeBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeBlock {
    #[serde(rename = "type")] pub block_type: String,
    #[serde(default)] pub text: Option<String>,
    #[serde(default)] pub source: Option<ClaudeImageSource>,
    #[serde(default)] pub name: Option<String>,
    #[serde(default)] pub input: Option<serde_json::Value>,
    #[serde(default)] pub input_schema: Option<serde_json::Value>,
    #[serde(default)] pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeImageSource {
    #[serde(rename = "type")] pub source_type: String,
    pub media_type: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeTool {
    pub name: String,
    #[serde(default)] pub description: Option<String>,
    pub input_schema: serde_json::Value,
}

// ============ Embeddings API ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingRequest {
    pub model: String,
    pub input: EmbeddingInput,
    #[serde(default)] pub encoding_format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EmbeddingInput {
    Single(String),
    Multiple(Vec<String>),
}

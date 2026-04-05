//! UniRoute 核心数据模型
//!
//! 架构：Provider（供应商）只管认证，ProviderEndpoint（端点）管协议和模型
//! 请求模型名 → Group → 端点列表 → 选择端点 → 通过 Provider 认证 → 发送请求

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============ 认证类型 ============

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthType { ApiKey, OAuth }

impl Default for AuthType {
    fn default() -> Self { Self::ApiKey }
}

// ============ OAuth ============

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub token_url: Option<String>,
    pub refresh_url: Option<String>,
    pub auth_url: Option<String>,
    pub initiate_url: Option<String>,
    pub poll_url_base: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub email: Option<String>,
}

impl OAuthTokens {
    pub fn is_expired(&self) -> bool {
        self.expires_at.map(|e| e < Utc::now()).unwrap_or(false)
    }
    pub fn needs_refresh(&self) -> bool {
        self.expires_at.map(|e| e < Utc::now() + chrono::Duration::minutes(5)).unwrap_or(false)
    }
}

// ============ 端点类型 ============

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EndpointType {
    Chat, Responses, Messages, Gemini, Embeddings, Audio, Images,
}

impl Default for EndpointType { fn default() -> Self { Self::Chat } }

impl EndpointType {
    pub fn default_path(&self) -> &'static str {
        match self {
            Self::Chat => "/v1/chat/completions",
            Self::Responses => "/v1/responses",
            Self::Messages => "/v1/messages",
            Self::Gemini => "/v1beta/models",
            Self::Embeddings => "/v1/embeddings",
            Self::Audio => "/v1/audio/speech",
            Self::Images => "/v1/images/generations",
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Self::Chat => "Chat", Self::Responses => "Responses", Self::Messages => "Messages",
            Self::Gemini => "Gemini", Self::Embeddings => "Embeddings", Self::Audio => "Audio",
            Self::Images => "Images",
        }
    }
    pub fn supports_streaming(&self) -> bool {
        matches!(self, Self::Chat | Self::Responses | Self::Messages | Self::Gemini)
    }
    pub fn can_convert_to(&self, target: &EndpointType) -> bool {
        if self == target { return true; }
        matches!(self, Self::Chat | Self::Responses | Self::Messages | Self::Gemini)
            && matches!(target, Self::Chat | Self::Responses | Self::Messages | Self::Gemini)
    }
}

// ============ 模型与定价 ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPricing { pub input: f64, pub output: f64 }
impl Default for ModelPricing { fn default() -> Self { Self { input: 0.0, output: 0.0 } } }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub name: String,
    #[serde(default)]
    pub pricing: Option<ModelPricing>,
}
impl From<&str> for ModelConfig { fn from(name: &str) -> Self { Self { name: name.to_string(), pricing: None } } }
impl From<String> for ModelConfig { fn from(name: String) -> Self { Self { name, pricing: None } } }

// ============ 配额管理 ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaLimit {
    #[serde(default)] pub daily_limit: Option<f64>,
    #[serde(default)] pub monthly_limit: Option<f64>,
    #[serde(default = "default_warning_threshold")] pub warning_threshold: f64,
}
fn default_warning_threshold() -> f64 { 0.8 }
impl Default for QuotaLimit { fn default() -> Self { Self { daily_limit: None, monthly_limit: None, warning_threshold: 0.8 } } }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QuotaStatus {
    pub daily_used: f64, pub monthly_used: f64,
    pub daily_remaining: Option<f64>, pub monthly_remaining: Option<f64>,
    pub daily_percent: Option<f64>, pub monthly_percent: Option<f64>,
    pub is_exceeded: bool, pub is_warning: bool,
}

impl QuotaStatus {
    pub fn compute(daily_used: f64, monthly_used: f64, limit: &QuotaLimit) -> Self {
        let daily_remaining = limit.daily_limit.map(|l| (l - daily_used).max(0.0));
        let monthly_remaining = limit.monthly_limit.map(|l| (l - monthly_used).max(0.0));
        let daily_percent = limit.daily_limit.map(|l| (daily_used / l * 100.0).min(100.0));
        let monthly_percent = limit.monthly_limit.map(|l| (monthly_used / l * 100.0).min(100.0));
        let is_exceeded = limit.daily_limit.map(|l| daily_used >= l).unwrap_or(false)
            || limit.monthly_limit.map(|l| monthly_used >= l).unwrap_or(false);
        let is_warning = !is_exceeded && (
            limit.daily_limit.map(|l| daily_used / l >= limit.warning_threshold).unwrap_or(false)
            || limit.monthly_limit.map(|l| monthly_used / l >= limit.warning_threshold).unwrap_or(false)
        );
        Self { daily_used, monthly_used, daily_remaining, monthly_remaining, daily_percent, monthly_percent, is_exceeded, is_warning }
    }
    pub fn allow_request(&self) -> Result<(), String> {
        if self.is_exceeded { Err(format!("配额已用完（已使用 ${:.4}）", self.daily_used)) } else { Ok(()) }
    }
}

// ============ Provider（供应商） ============

/// Provider 只管认证，但保留兼容字段供过渡期使用
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String, pub name: String, pub prefix: String,
    pub api_key: Option<String>,
    #[serde(default)] pub auth_type: AuthType,
    #[serde(default)] pub oauth: Option<OAuthConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub oauth_tokens: Option<OAuthTokens>,
    #[serde(default)] pub headers: HashMap<String, String>,
    #[serde(default = "default_auth_header")] pub auth_header: String,
    #[serde(default)] pub auth_prefix: Option<String>,
    // 兼容字段（过渡期保留）
    #[serde(default)] pub base_url: String,
    #[serde(default)] pub api_format: ApiFormat,
    #[serde(default)] pub models: Vec<ModelConfig>,
    #[serde(default = "default_enable_cost")] pub enable_cost: bool,
    pub is_active: bool, pub is_builtin: bool,
    pub created_at: DateTime<Utc>, pub updated_at: DateTime<Utc>,
}

fn default_auth_header() -> String { "Authorization".to_string() }
fn default_enable_cost() -> bool { true }

impl Provider {
    pub fn new(name: String, prefix: String) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(), name, prefix,
            api_key: None, auth_type: AuthType::ApiKey, oauth: None, oauth_tokens: None,
            headers: HashMap::new(), auth_header: "Authorization".to_string(), auth_prefix: None,
            base_url: String::new(), api_format: ApiFormat::default(),
            models: Vec::new(), enable_cost: true,
            is_active: true, is_builtin: false, created_at: now, updated_at: now,
        }
    }
    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = base_url;
        self
    }
    pub fn needs_oauth(&self) -> bool { self.auth_type == AuthType::OAuth && self.oauth_tokens.is_none() }
    pub fn needs_token_refresh(&self) -> bool {
        if self.auth_type != AuthType::OAuth { return false; }
        self.oauth_tokens.as_ref().map(|t| t.needs_refresh()).unwrap_or(true)
    }
    pub fn get_auth_value(&self) -> Option<String> {
        match self.auth_type {
            AuthType::ApiKey => self.api_key.as_ref().map(|key| {
                if let Some(prefix) = &self.auth_prefix {
                    if prefix.is_empty() { key.clone() } else { format!("{} {}", prefix, key) }
                } else if self.auth_header.to_lowercase() == "authorization" {
                    format!("Bearer {}", key)
                } else { key.clone() }
            }),
            AuthType::OAuth => self.oauth_tokens.as_ref().map(|t| format!("Bearer {}", t.access_token)),
        }
    }
    pub fn model_names(&self) -> Vec<String> {
        self.models.iter().map(|m| m.name.clone()).collect()
    }
    pub fn get_model_pricing(&self, model_name: &str) -> Option<&ModelPricing> {
        self.models.iter()
            .find(|m| m.name == model_name || m.name == "*")
            .and_then(|m| m.pricing.as_ref())
    }
    pub fn builtin_templates() -> Vec<ProviderTemplate> {
        ProviderTemplate::builtin_templates()
    }
}

// ============ Provider Endpoint（供应商端点） ============

/// 端点 = base_url + 端点类型 + 模型列表
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderEndpoint {
    pub id: String, pub provider_id: String, pub endpoint_type: EndpointType,
    pub base_url: String, pub models: Vec<ModelConfig>,
    #[serde(default)] pub enable_cost: bool,
    pub is_active: bool,
    pub created_at: DateTime<Utc>, pub updated_at: DateTime<Utc>,
}

impl ProviderEndpoint {
    pub fn new(provider_id: String, endpoint_type: EndpointType, base_url: String) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            provider_id,
            endpoint_type,
            base_url,
            models: Vec::new(),
            enable_cost: true,
            is_active: true,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn model_names(&self) -> Vec<String> {
        self.models.iter().map(|m| m.name.clone()).collect()
    }

    pub fn get_model_pricing(&self, model_name: &str) -> Option<&ModelPricing> {
        self.models.iter()
            .find(|m| m.name == model_name || m.name == "*")
            .and_then(|m| m.pricing.as_ref())
    }
}

// ============ Group（模型组） ============

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GroupStrategy { Priority, RoundRobin, Random, Weighted, LeastUsed, CostOptimized }
impl Default for GroupStrategy { fn default() -> Self { Self::Priority } }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GroupConfig {
    pub retry_delay_ms: i32,
    pub max_retries: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupModel {
    pub model: String,
    #[serde(default)] pub priority: i32,
    #[serde(default = "default_weight")] pub weight: u32,
}
fn default_weight() -> u32 { 1 }

impl GroupModel {
    pub fn new(model: String) -> Self {
        Self { model, priority: 0, weight: 1 }
    }
    pub fn with_priority(mut self, priority: u32) -> Self {
        self.priority = priority as i32;
        self
    }
    pub fn with_weight(mut self, weight: u32) -> Self {
        self.weight = weight;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: String, pub name: String,
    #[serde(default)] pub description: Option<String>,
    pub models: Vec<GroupModel>,
    #[serde(default)] pub strategy: GroupStrategy,
    #[serde(default)] pub config: GroupConfig,
    pub is_active: bool,
    pub created_at: DateTime<Utc>, pub updated_at: DateTime<Utc>,
}

impl Group {
    pub fn new(name: String) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            description: None,
            models: Vec::new(),
            strategy: GroupStrategy::default(),
            config: GroupConfig::default(),
            is_active: true,
            created_at: now,
            updated_at: now,
        }
    }
    pub fn add_model(&mut self, model: GroupModel) {
        self.models.push(model);
    }
    pub fn get_ordered_models(&self) -> Vec<GroupModel> {
        let mut models = self.models.clone();
        models.sort_by_key(|m| m.priority);
        models
    }
}

// ============ 模型映射 ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMapping {
    pub id: String,
    pub pattern: String,
    pub group_id: String,
    #[serde(default)] pub priority: u32,
}

impl ModelMapping {
    pub fn new(pattern: String, group_id: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            pattern,
            group_id,
            priority: 0,
        }
    }
    pub fn matches(&self, model_name: &str) -> bool {
        if self.pattern.starts_with('^') && self.pattern.ends_with('$') {
            regex::Regex::new(&self.pattern).map(|r| r.is_match(model_name)).unwrap_or(false)
        } else if self.pattern.contains('*') {
            let parts: Vec<&str> = self.pattern.split('*').collect();
            if parts.len() == 2 {
                model_name.starts_with(parts[0]) && model_name.ends_with(parts[1])
            } else { model_name == self.pattern }
        } else { model_name == self.pattern }
    }
}

// ============ 请求日志 ============

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RequestLog {
    pub id: i64,
    pub timestamp: DateTime<Utc>,
    pub method: String,
    pub path: String,
    pub requested_model: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub provider_prefix: Option<String>,
    pub url: Option<String>,
    pub protocol_transform: Option<String>,
    pub status_code: i32,
    pub latency_ms: i64,
    pub first_token_ms: Option<i64>,
    pub prompt_tokens: Option<i32>,
    pub completion_tokens: Option<i32>,
    pub cost: Option<f64>,
    pub error: Option<String>,
    pub request_body: Option<String>,
    pub response_body: Option<String>,
    /// 原始响应（协议转换前的上游响应）
    pub original_response_body: Option<String>,
}

impl RequestLog {
    pub fn new(method: String, path: String) -> Self {
        Self {
            id: 0,
            timestamp: Utc::now(),
            method, path,
            requested_model: None,
            model: None,
            provider: None,
            provider_prefix: None,
            url: None,
            protocol_transform: None,
            status_code: 0,
            latency_ms: 0,
            first_token_ms: None,
            prompt_tokens: None,
            completion_tokens: None,
            cost: None,
            error: None,
            request_body: None,
            response_body: None,
            original_response_body: None,
        }
    }

    pub fn with_status(mut self, status: i32) -> Self { self.status_code = status; self }
    pub fn with_latency(mut self, latency_ms: i64) -> Self { self.latency_ms = latency_ms; self }
    pub fn with_first_token(mut self, first_token_ms: i64) -> Self { self.first_token_ms = Some(first_token_ms); self }
    pub fn with_requested_model(mut self, model: String) -> Self { self.requested_model = Some(model); self }
    pub fn with_model(mut self, model: String) -> Self { self.model = Some(model); self }
    pub fn with_provider(mut self, name: String, prefix: String) -> Self { self.provider = Some(name); self.provider_prefix = Some(prefix); self }
    pub fn with_url(mut self, url: String) -> Self { self.url = Some(url); self }
    pub fn with_request(mut self, request: String) -> Self { self.request_body = Some(request); self }
    pub fn with_response(mut self, response: String) -> Self { self.response_body = Some(response); self }
    pub fn with_original_response(mut self, response: String) -> Self { self.original_response_body = Some(response); self }
    pub fn with_error(mut self, error: String) -> Self { self.error = Some(error); self }
    pub fn with_tokens(mut self, prompt: i32, completion: i32) -> Self { self.prompt_tokens = Some(prompt); self.completion_tokens = Some(completion); self }
    pub fn with_cost(mut self, cost: f64) -> Self { self.cost = Some(cost); self }
    pub fn with_protocol_transform(mut self, transform: String) -> Self { self.protocol_transform = Some(transform); self }
}

// ============ 设置 ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default = "default_proxy_port")]
    pub proxy_port: u16,
    #[serde(default)] pub auto_start_proxy: bool,
    #[serde(default = "default_log_level")] pub log_level: String,
}
fn default_proxy_port() -> u16 { 8080 }
fn default_log_level() -> String { "info".to_string() }

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            proxy_port: 8080,
            auto_start_proxy: false,
            log_level: "info".to_string(),
        }
    }
}

// ============ ApiFormat（兼容旧代码） ============

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ApiFormat {
    #[default]
    OpenAI,
    Claude,
    Gemini,
    Responses,
}

impl ApiFormat {
    pub fn to_endpoint_type(&self) -> EndpointType {
        match self {
            Self::OpenAI => EndpointType::Chat,
            Self::Claude => EndpointType::Messages,
            Self::Gemini => EndpointType::Gemini,
            Self::Responses => EndpointType::Responses,
        }
    }
    pub fn default_endpoint(&self) -> &'static str {
        match self {
            Self::OpenAI => "/v1/chat/completions",
            Self::Claude => "/v1/messages",
            Self::Gemini => "/v1beta/models",
            Self::Responses => "/v1/responses",
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Self::OpenAI => "OpenAI",
            Self::Claude => "Claude",
            Self::Gemini => "Gemini",
            Self::Responses => "Responses",
        }
    }
    pub fn endpoint_path(&self) -> &'static str {
        match self {
            Self::OpenAI => "/v1/chat/completions",
            Self::Claude => "/v1/messages",
            Self::Gemini => "/v1beta/models",
            Self::Responses => "/v1/responses",
        }
    }
}

// ============ Chat Request ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(default)] pub stream: bool,
    #[serde(default)] pub temperature: Option<f32>,
    #[serde(default)] pub max_tokens: Option<u32>,
    #[serde(default)] pub tools: Vec<Tool>,
    #[serde(flatten)] pub extra: serde_json::Map<String, serde_json::Value>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")] pub tool_calls: Vec<ToolCall>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrl {
    pub url: String,
    #[serde(default)] pub detail: Option<String>,
}

// ============ Responses API ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesRequest {
    pub model: String,
    pub input: ResponsesInput,
    #[serde(default)] pub stream: bool,
    #[serde(default)] pub instructions: Option<String>,
    #[serde(default)] pub tools: Vec<Tool>,
    #[serde(default)] pub temperature: Option<f64>,
    #[serde(default)] pub max_output_tokens: Option<i32>,
    #[serde(flatten)] pub extra: serde_json::Map<String, serde_json::Value>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesResponse {
    pub id: String,
    pub object: String,
    pub created_at: i64,
    pub status: String,
    pub model: String,
    pub output: Option<ResponsesOutput>,
    #[serde(default)] pub usage: Option<UsageInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesOutput {
    #[serde(default)] pub content: Option<Vec<ContentPart>>,
    #[serde(default)] pub text: Option<String>,
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
    Text(String),
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

// ============ Tool ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    #[serde(rename = "type")] pub tool_type: String,
    #[serde(default)]
    pub function: Option<ToolFunction>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    #[serde(default)] pub description: Option<String>,
    #[serde(default)] pub parameters: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")] pub call_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

// ============ API 响应格式 ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<ChatChoice>,
    #[serde(default)] pub usage: Option<UsageInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatChoice {
    pub index: i32,
    pub message: ChatMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageInfo {
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
}

// ============ 模板 ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderTemplate {
    pub id: String,
    pub name: String,
    pub prefix: String,
    pub default_base_url: String,
    pub endpoint_types: Vec<EndpointType>,
    // 兼容字段：默认使用第一个 endpoint_type 对应的 api_format
    #[serde(default = "default_api_format")]
    pub api_format: ApiFormat,
    pub auth_type: AuthType,
    #[serde(default)] pub oauth: Option<OAuthConfig>,
    #[serde(default)] pub headers: HashMap<String, String>,
    pub auth_header: String,
    pub auth_prefix: Option<String>,
    pub models: Vec<ModelConfig>,
}

fn default_api_format() -> ApiFormat { ApiFormat::OpenAI }

impl ProviderTemplate {
    pub fn builtin_templates() -> Vec<ProviderTemplate> {
        vec![
            // OpenAI
            ProviderTemplate {
                id: "openai".into(),
                name: "OpenAI".into(),
                prefix: "oai".into(),
                default_base_url: "https://api.openai.com".into(),
                endpoint_types: vec![EndpointType::Chat, EndpointType::Responses, EndpointType::Embeddings, EndpointType::Audio, EndpointType::Images],
                api_format: ApiFormat::OpenAI,
                auth_type: AuthType::ApiKey,
                oauth: None,
                headers: HashMap::new(),
                auth_header: "Authorization".into(),
                auth_prefix: Some("Bearer".into()),
                models: vec!["gpt-4o".into(), "gpt-4o-mini".into(), "gpt-4-turbo".into()],
            },
            // Claude
            ProviderTemplate {
                id: "claude".into(),
                name: "Claude".into(),
                prefix: "ant".into(),
                default_base_url: "https://api.anthropic.com".into(),
                endpoint_types: vec![EndpointType::Messages],
                api_format: ApiFormat::Claude,
                auth_type: AuthType::ApiKey,
                oauth: None,
                headers: vec![("anthropic-version".into(), "2023-06-01".into())].into_iter().collect(),
                auth_header: "x-api-key".into(),
                auth_prefix: None,
                models: vec!["claude-opus-4-6".into(), "claude-sonnet-4-6".into(), "claude-haiku-4-5-20251001".into()],
            },
            // Claude Code OAuth
            ProviderTemplate {
                id: "claude-code".into(),
                name: "Claude Code (OAuth)".into(),
                prefix: "cc".into(),
                default_base_url: "https://api.anthropic.com".into(),
                endpoint_types: vec![EndpointType::Messages],
                api_format: ApiFormat::Claude,
                auth_type: AuthType::OAuth,
                oauth: Some(OAuthConfig {
                    client_id: "9d1c250a-e61b-44d9-88ed-5944d1962f5e".into(),
                    token_url: Some("https://console.anthropic.com/v1/oauth/token".into()),
                    ..Default::default()
                }),
                headers: vec![
                    ("anthropic-version".into(), "2023-06-01".into()),
                    ("anthropic-beta".into(), "claude-code-20250219,oauth-2025-04-20".into()),
                ].into_iter().collect(),
                auth_header: "x-api-key".into(),
                auth_prefix: None,
                models: vec!["claude-opus-4-6".into(), "claude-sonnet-4-6".into()],
            },
            // Gemini
            ProviderTemplate {
                id: "gemini".into(),
                name: "Gemini".into(),
                prefix: "gc".into(),
                default_base_url: "https://generativelanguage.googleapis.com".into(),
                endpoint_types: vec![EndpointType::Gemini],
                api_format: ApiFormat::Gemini,
                auth_type: AuthType::ApiKey,
                oauth: None,
                headers: HashMap::new(),
                auth_header: "x-goog-api-key".into(),
                auth_prefix: None,
                models: vec!["gemini-2.5-pro".into(), "gemini-2.5-flash".into()],
            },
            // DeepSeek
            ProviderTemplate {
                id: "deepseek".into(),
                name: "DeepSeek".into(),
                prefix: "ds".into(),
                default_base_url: "https://api.deepseek.com".into(),
                endpoint_types: vec![EndpointType::Chat],
                api_format: ApiFormat::OpenAI,
                auth_type: AuthType::ApiKey,
                oauth: None,
                headers: HashMap::new(),
                auth_header: "Authorization".into(),
                auth_prefix: Some("Bearer".into()),
                models: vec!["deepseek-chat".into(), "deepseek-reasoner".into()],
            },
            // Moonshot (Kimi)
            ProviderTemplate {
                id: "moonshot".into(),
                name: "Moonshot (Kimi)".into(),
                prefix: "ms".into(),
                default_base_url: "https://api.moonshot.cn".into(),
                endpoint_types: vec![EndpointType::Chat],
                api_format: ApiFormat::OpenAI,
                auth_type: AuthType::ApiKey,
                oauth: None,
                headers: HashMap::new(),
                auth_header: "Authorization".into(),
                auth_prefix: Some("Bearer".into()),
                models: vec!["moonshot-v1-8k".into(), "moonshot-v1-32k".into(), "moonshot-v1-128k".into()],
            },
            // 智谱AI
            ProviderTemplate {
                id: "zhipu".into(),
                name: "智谱AI".into(),
                prefix: "zp".into(),
                default_base_url: "https://open.bigmodel.cn/api/paas/v4".into(),
                endpoint_types: vec![EndpointType::Chat],
                api_format: ApiFormat::OpenAI,
                auth_type: AuthType::ApiKey,
                oauth: None,
                headers: HashMap::new(),
                auth_header: "Authorization".into(),
                auth_prefix: Some("Bearer".into()),
                models: vec!["glm-4-plus".into(), "glm-4-flash".into()],
            },
        ]
    }
}

// ============ Tests ============

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_endpoint_type_can_convert() {
        assert!(EndpointType::Chat.can_convert_to(&EndpointType::Responses));
        assert!(EndpointType::Messages.can_convert_to(&EndpointType::Chat));
        assert!(!EndpointType::Chat.can_convert_to(&EndpointType::Embeddings));
    }

    #[test]
    fn test_provider_auth_value() {
        let mut p = Provider::new("Test".into(), "test".into());
        p.api_key = Some("sk-123".into());
        assert_eq!(p.get_auth_value(), Some("Bearer sk-123".to_string()));

        p.auth_header = "x-api-key".into();
        p.auth_prefix = None;
        assert_eq!(p.get_auth_value(), Some("sk-123".to_string()));
    }

    #[test]
    fn test_quota_status() {
        let limit = QuotaLimit { daily_limit: Some(10.0), monthly_limit: None, warning_threshold: 0.8 };
        let status = QuotaStatus::compute(8.0, 0.0, &limit);
        assert!(status.is_warning);
        assert!(!status.is_exceeded);
    }
}

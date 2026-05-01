//! 核心实体类型：Provider、Group、ModelMapping 等

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============ 认证类型 ============

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum AuthType { #[default]
ApiKey, OAuth }


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
#[derive(Default)]
pub enum EndpointType {
    #[default]
    Chat, Responses, Messages, Gemini, Embeddings, Audio, Images,
}


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

// ============ 模型端点能力 ============

/// 模型支持的端点类型
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum EndpointCapability {
    /// Chat Completions API (OpenAI 格式)
    #[default]
    Chat,
    /// Responses API (OpenAI 新格式)
    Responses,
    /// Claude Messages API (Anthropic 格式)
    Claude,
    /// Gemini API (Google 格式)
    Gemini,
    /// Embeddings API
    Embeddings,
    /// Image Generation
    Images,
    /// Video Generation
    Videos,
    /// Music Generation
    Music,
    /// Audio Transcription/Translation (ASR)
    Audio,
    /// Text-to-Speech (TTS)
    TTS,
    /// Content Moderation
    Moderation,
    /// Rerank
    Rerank,
}

impl EndpointCapability {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Chat => "Chat",
            Self::Responses => "Responses",
            Self::Claude => "Claude",
            Self::Gemini => "Gemini",
            Self::Embeddings => "Embeddings",
            Self::Images => "Images",
            Self::Videos => "Videos",
            Self::Music => "Music",
            Self::Audio => "Audio",
            Self::TTS => "TTS",
            Self::Moderation => "Moderation",
            Self::Rerank => "Rerank",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Chat => "\u{1f4ac}",
            Self::Responses => "\u{26a1}",
            Self::Claude => "\u{1f916}",
            Self::Gemini => "\u{2728}",
            Self::Embeddings => "\u{1f522}",
            Self::Images => "\u{1f5bc}\u{fe0f}",
            Self::Videos => "\u{1f3ac}",
            Self::Music => "\u{1f3b5}",
            Self::Audio => "\u{1f3a4}",
            Self::TTS => "\u{1f50a}",
            Self::Moderation => "\u{1f6e1}\u{fe0f}",
            Self::Rerank => "\u{1f4ca}",
        }
    }

    pub fn endpoint_path(&self) -> &'static str {
        match self {
            Self::Chat => "/v1/chat/completions",
            Self::Responses => "/v1/responses",
            Self::Claude => "/v1/messages",
            Self::Gemini => "/v1beta/models/{model}:generateContent",
            Self::Embeddings => "/v1/embeddings",
            Self::Images => "/v1/images/generations",
            Self::Videos => "/v1/videos/generations",
            Self::Music => "/v1/music/generations",
            Self::Audio => "/v1/audio/transcriptions",
            Self::TTS => "/v1/audio/speech",
            Self::Moderation => "/v1/moderations",
            Self::Rerank => "/v1/rerank",
        }
    }

    pub fn all() -> Vec<Self> {
        vec![
            Self::Chat, Self::Responses, Self::Claude, Self::Gemini,
            Self::Embeddings, Self::Images, Self::Videos, Self::Music,
            Self::Audio, Self::TTS, Self::Moderation, Self::Rerank
        ]
    }

    /// 常用端点（用于 UI 显示）
    pub fn common() -> Vec<Self> {
        vec![Self::Chat, Self::Responses, Self::Claude, Self::Gemini, Self::Embeddings, Self::Images]
    }

    /// 描述信息
    pub fn description(&self) -> &'static str {
        match self {
            Self::Chat => "标准 Chat API，用于对话补全",
            Self::Responses => "Responses API，用于 Codex、OpenCode 等工具",
            Self::Claude => "Claude Messages API，用于 Claude 模型",
            Self::Gemini => "Gemini API，用于 Google Gemini 模型",
            Self::Embeddings => "向量嵌入，用于文本向量化",
            Self::Images => "图像生成，用于创建图片",
            Self::Videos => "视频生成，用于创建视频",
            Self::Music => "音乐生成，用于创建音频",
            Self::Audio => "语音转文字，用于音频转录",
            Self::TTS => "文字转语音，用于语音合成",
            Self::Moderation => "内容审核，用于检测不当内容",
            Self::Rerank => "重排序，用于搜索结果重排",
        }
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub name: String,
    #[serde(default)]
    pub pricing: Option<ModelPricing>,
    /// 支持的端点能力
    #[serde(default)]
    pub endpoints: Vec<EndpointCapability>,
    /// RPM 限制 (requests per minute)
    #[serde(default)]
    pub rpm: Option<u32>,
    /// TPM 限制 (tokens per minute)
    #[serde(default)]
    pub tpm: Option<u32>,
}

impl ModelConfig {
    /// 创建新模型配置，默认支持 Chat 端点
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            pricing: None,
            endpoints: vec![EndpointCapability::Chat],
            rpm: None,
            tpm: None,
        }
    }

    /// 检查是否支持指定端点
    pub fn supports(&self, endpoint: EndpointCapability) -> bool {
        self.endpoints.contains(&endpoint)
    }

    /// 添加端点支持
    pub fn with_endpoint(mut self, endpoint: EndpointCapability) -> Self {
        if !self.endpoints.contains(&endpoint) {
            self.endpoints.push(endpoint);
        }
        self
    }

    /// 设置 RPM 限制
    pub fn with_rpm(mut self, rpm: u32) -> Self {
        self.rpm = Some(rpm);
        self
    }
}

impl From<&str> for ModelConfig {
    fn from(name: &str) -> Self {
        Self::new(name)
    }
}
impl From<String> for ModelConfig {
    fn from(name: String) -> Self {
        Self::new(name)
    }
}

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
    /// 定价货币单位: "USD" 或 "CNY"，默认 "CNY"
    #[serde(default = "default_currency")] pub currency: String,
    pub is_active: bool, pub is_builtin: bool,
    pub created_at: DateTime<Utc>, pub updated_at: DateTime<Utc>,
}

fn default_auth_header() -> String { "Authorization".to_string() }
fn default_enable_cost() -> bool { true }
fn default_currency() -> String { "CNY".to_string() }

impl Provider {
    pub fn new(name: String, prefix: String) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(), name, prefix,
            api_key: None, auth_type: AuthType::ApiKey, oauth: None, oauth_tokens: None,
            headers: HashMap::new(), auth_header: "Authorization".to_string(), auth_prefix: None,
            base_url: String::new(), api_format: ApiFormat::default(),
            models: Vec::new(), enable_cost: true, currency: "CNY".to_string(),
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
    pub fn builtin_templates() -> Vec<crate::models::ProviderTemplate> {
        crate::models::ProviderTemplate::builtin_templates()
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
        // 首先尝试精确匹配
        if let Some(pricing) = self.models.iter()
            .find(|m| m.name == model_name || m.name == "*")
            .and_then(|m| m.pricing.as_ref()) {
            return Some(pricing);
        }

        // 尝试后缀匹配（处理路由解析后去掉前缀的情况）
        // 例如：模型名 "DeepSeek-V3.2" 可以匹配 "Pro/deepseek-ai/DeepSeek-V3.2"
        if let Some(pricing) = self.models.iter()
            .find(|m| {
                if m.name == "*" {
                    return true;
                }
                // 检查 model_name 是否是 m.name 的后缀
                m.name.ends_with(model_name) ||
                // 或者去掉 m.name 前缀后匹配
                m.name.split('/').next_back() == Some(model_name)
            })
            .and_then(|m| m.pricing.as_ref()) {
            return Some(pricing);
        }

        None
    }
}

// ============ Group（模型组） ============

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum GroupStrategy { #[default]
Priority, RoundRobin, Random, Weighted, LeastUsed, CostOptimized }

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
    #[serde(default = "default_enabled")] pub enabled: bool,
}
fn default_weight() -> u32 { 1 }
fn default_enabled() -> bool { true }

impl GroupModel {
    pub fn new(model: String) -> Self {
        Self { model, priority: 0, weight: 1, enabled: true }
    }
    pub fn with_priority(mut self, priority: u32) -> Self {
        self.priority = priority as i32;
        self
    }
    pub fn with_weight(mut self, weight: u32) -> Self {
        self.weight = weight;
        self
    }
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
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
    /// 端点类型：用于指定此 Group 服务于哪种 API 格式
    /// - chat: 标准 Chat API（默认）
    /// - responses: OpenAI Responses API（Codex、OpenCode 等）
    /// - claude: Claude Messages API
    /// - gemini: Gemini API
    #[serde(default)]
    pub endpoint_type: Option<String>,
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
            endpoint_type: None,
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
    pub endpoint_type: Option<String>,
    pub status_code: i32,
    pub latency_ms: i64,
    pub first_token_ms: Option<i64>,
    pub prompt_tokens: Option<i32>,
    pub completion_tokens: Option<i32>,
    /// 原始请求的估算 token 数（转换前）
    pub original_input_tokens: Option<i32>,
    /// 转换后请求的估算 token 数（转换后）
    pub translated_input_tokens: Option<i32>,
    pub cost: Option<f64>,
    pub error: Option<String>,
    /// 原始请求（客户端发送的请求）
    pub original_request_body: Option<String>,
    /// 转换后的请求（发送给上游的请求）
    pub request_body: Option<String>,
    /// 原始响应（上游返回的响应）
    pub original_response_body: Option<String>,
    /// 转换后的响应（返回给客户端的响应）
    pub response_body: Option<String>,
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
            endpoint_type: None,
            status_code: 0,
            latency_ms: 0,
            first_token_ms: None,
            prompt_tokens: None,
            completion_tokens: None,
            original_input_tokens: None,
            translated_input_tokens: None,
            cost: None,
            error: None,
            original_request_body: None,
            request_body: None,
            original_response_body: None,
            response_body: None,
        }
    }

    pub fn with_status(mut self, status: i32) -> Self { self.status_code = status; self }
    pub fn with_latency(mut self, latency_ms: i64) -> Self { self.latency_ms = latency_ms; self }
    pub fn with_first_token(mut self, first_token_ms: i64) -> Self { self.first_token_ms = Some(first_token_ms); self }
    pub fn with_requested_model(mut self, model: String) -> Self { self.requested_model = Some(model); self }
    pub fn with_model(mut self, model: String) -> Self { self.model = Some(model); self }
    pub fn with_provider(mut self, name: String, prefix: String) -> Self { self.provider = Some(name); self.provider_prefix = Some(prefix); self }
    pub fn with_url(mut self, url: String) -> Self { self.url = Some(url); self }
    /// 保存原始请求（客户端发送的）
    pub fn with_original_request(mut self, request: String) -> Self { self.original_request_body = Some(request); self }
    /// 保存转换后的请求（发送给上游的）
    pub fn with_request(mut self, request: String) -> Self { self.request_body = Some(request); self }
    /// 保存原始响应（上游返回的）
    pub fn with_original_response(mut self, response: String) -> Self { self.original_response_body = Some(response); self }
    /// 转换后的响应（返回给客户端的）
    pub fn with_response(mut self, response: String) -> Self { self.response_body = Some(response); self }
    pub fn with_error(mut self, error: String) -> Self { self.error = Some(error); self }
    pub fn with_tokens(mut self, prompt: i32, completion: i32) -> Self { self.prompt_tokens = Some(prompt); self.completion_tokens = Some(completion); self }
    pub fn with_cost(mut self, cost: f64) -> Self { self.cost = Some(cost); self }
    pub fn with_protocol_transform(mut self, transform: String) -> Self { self.protocol_transform = Some(transform); self }
    pub fn with_endpoint_type(mut self, endpoint_type: String) -> Self { self.endpoint_type = Some(endpoint_type); self }
    /// 设置原始请求和转换后请求的估算 token 数
    pub fn with_input_tokens(mut self, original: i32, translated: i32) -> Self {
        self.original_input_tokens = Some(original);
        self.translated_input_tokens = Some(translated);
        self
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

// ============ Tool ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    #[serde(rename = "type", default = "default_tool_type")]
    pub tool_type: String,
    #[serde(default)]
    pub function: Option<ToolFunction>,
    /// 捕获额外字段（兼容旧版 functions 格式）
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

fn default_tool_type() -> String { "function".to_string() }

impl Tool {
    /// 规范化 Tool，处理旧版 functions 格式
    pub fn normalize(&self) -> Self {
        // 如果已经有 function 字段，直接返回
        if self.function.is_some() {
            return self.clone();
        }

        // 尝试从 extra 中提取旧版格式字段
        let name = self.extra.get("name").and_then(|v| v.as_str());
        let description = self.extra.get("description").and_then(|v| v.as_str());
        let parameters = self.extra.get("parameters").cloned();

        if let Some(name) = name {
            let function = ToolFunction {
                name: name.to_string(),
                description: description.map(|s| s.to_string()),
                parameters,
            };
            Tool {
                tool_type: "function".to_string(),
                function: Some(function),
                extra: serde_json::Map::new(),
            }
        } else {
            self.clone()
        }
    }
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

/// 工具选择策略
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolChoice {
    /// 字符串: "none", "auto", "required"
    String(String),
    /// 对象: {"type": "function", "function": {"name": "..."}}
    Object(serde_json::Value),
}

/// 响应格式
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseFormat {
    #[serde(rename = "type")]
    pub format_type: String,
    #[serde(default)]
    pub json_schema: Option<serde_json::Value>,
}

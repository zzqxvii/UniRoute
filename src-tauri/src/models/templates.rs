//! 供应商模板：ProviderTemplate 和内置模板

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::entities::{
    ApiFormat, AuthType, EndpointType, ModelConfig, OAuthConfig,
};

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

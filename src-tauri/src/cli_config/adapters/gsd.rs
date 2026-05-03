//! GSD CLI 配置适配器
//!
//! 配置文件: ~/.gsd/agent/{auth.json, settings.json, models.json}
//! 接管策略: 添加 uniroute provider 并切换 defaultProvider

use super::{get_home_dir, read_json_file, write_json_file};
use crate::cli_config::snapshot;
use crate::cli_config::traits::CliConfigurator;
use crate::cli_config::types::ConfigSnapshot;
use crate::error::AppError;
use async_trait::async_trait;
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;
use std::path::PathBuf;

pub struct GsdAdapter;

impl GsdAdapter {
    pub fn new() -> Self {
        Self
    }

    fn agent_dir() -> PathBuf {
        get_home_dir().join(".gsd").join("agent")
    }

    fn auth_path() -> PathBuf {
        Self::agent_dir().join("auth.json")
    }

    fn settings_path() -> PathBuf {
        Self::agent_dir().join("settings.json")
    }

    fn models_path() -> PathBuf {
        Self::agent_dir().join("models.json")
    }

    fn read_auth() -> Result<Option<JsonValue>, AppError> {
        read_json_file(&Self::auth_path())
    }

    fn read_settings() -> Result<Option<JsonValue>, AppError> {
        read_json_file(&Self::settings_path())
    }

    fn read_models() -> Result<Option<JsonValue>, AppError> {
        read_json_file(&Self::models_path())
    }
}

#[async_trait]
impl CliConfigurator for GsdAdapter {
    fn tool_id(&self) -> &str { "gsd" }

    fn display_name(&self) -> &str { "GSD" }

    fn description(&self) -> &str {
        "GSD AI Agent，支持多工作区、多模型、扩展系统"
    }

    fn config_path(&self) -> PathBuf {
        Self::agent_dir()
    }

    fn homepage(&self) -> Option<String> {
        None
    }

    fn is_installed(&self) -> bool {
        Self::agent_dir().exists()
    }

    fn is_taken_over(&self) -> Result<bool, AppError> {
        if let Some(settings) = Self::read_settings()? {
            let is_uniroute = settings
                .get("defaultProvider")
                .and_then(|v| v.as_str())
                .map(|s| s == "uniroute")
                .unwrap_or(false);
            Ok(is_uniroute)
        } else {
            Ok(false)
        }
    }

    fn snapshot(&self) -> Result<ConfigSnapshot, AppError> {
        let mut files = HashMap::new();
        snapshot::snapshot_file(&Self::auth_path(), &mut files)?;
        snapshot::snapshot_file(&Self::settings_path(), &mut files)?;
        snapshot::snapshot_file(&Self::models_path(), &mut files)?;
        Ok(snapshot::create_snapshot(self.tool_id(), files))
    }

    fn takeover(&self, proxy_url: &str, api_key: &str, model: &str) -> Result<(), AppError> {
        // 1. auth.json — 添加 uniroute provider 的 API key
        let mut auth = Self::read_auth()?
            .unwrap_or_else(|| json!({}));
        if let Some(root) = auth.as_object_mut() {
            root.insert("uniroute".to_string(), json!({
                "type": "api_key",
                "key": api_key
            }));
        }
        write_json_file(&Self::auth_path(), &auth)?;

        // 2. models.json — 添加 uniroute provider
        let mut models = Self::read_models()?
            .unwrap_or_else(|| json!({ "providers": {} }));
        let uniroute_provider = json!({
            "baseUrl": proxy_url.trim_end_matches('/'),
            "apiKey": "env:CUSTOM_OPENAI_API_KEY",
            "api": "openai-completions",
            "models": [
                {
                    "id": model,
                    "name": model,
                    "reasoning": false,
                    "input": ["text"],
                    "contextWindow": 128000,
                    "maxTokens": 16384,
                    "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 }
                }
            ]
        });
        if let Some(providers) = models.get_mut("providers").and_then(|v| v.as_object_mut()) {
            providers.insert("uniroute".to_string(), uniroute_provider);
        }
        write_json_file(&Self::models_path(), &models)?;

        // 3. settings.json — 切换 defaultProvider
        let mut settings = Self::read_settings()?
            .unwrap_or_else(|| json!({}));
        if let Some(root) = settings.as_object_mut() {
            root.insert("defaultProvider".to_string(), json!("uniroute"));
            root.insert("defaultModel".to_string(), json!(model));
        }
        write_json_file(&Self::settings_path(), &settings)?;

        tracing::info!("GSD 配置已接管: proxy={proxy_url}, model={model}");
        Ok(())
    }

    fn restore(&self, snapshot: &ConfigSnapshot) -> Result<(), AppError> {
        snapshot::restore_file(&Self::auth_path(), &snapshot.files)?;
        snapshot::restore_file(&Self::settings_path(), &snapshot.files)?;
        snapshot::restore_file(&Self::models_path(), &snapshot.files)?;
        tracing::info!("GSD 配置已恢复");
        Ok(())
    }

    fn get_current_model(&self) -> Result<Option<String>, AppError> {
        if let Some(settings) = Self::read_settings()? {
            let model = settings
                .get("defaultModel")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            Ok(model)
        } else {
            Ok(None)
        }
    }
}

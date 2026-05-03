//! Claude Code CLI 配置适配器
//!
//! 配置文件: ~/.claude/settings.json
//! 接管时修改 ANTHROPIC_BASE_URL 和 ANTHROPIC_AUTH_TOKEN

use super::{get_home_dir, read_json_file, write_json_file};
use crate::cli_config::snapshot;
use crate::cli_config::traits::CliConfigurator;
use crate::cli_config::types::ConfigSnapshot;
use crate::error::AppError;
use async_trait::async_trait;
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;
use std::path::PathBuf;

pub struct ClaudeAdapter;

impl ClaudeAdapter {
    pub fn new() -> Self {
        Self
    }

    fn config_dir() -> PathBuf {
        get_home_dir().join(".claude")
    }

    fn settings_path() -> PathBuf {
        Self::config_dir().join("settings.json")
    }

    fn read_settings() -> Result<Option<JsonValue>, AppError> {
        read_json_file(&Self::settings_path())
    }

    fn write_settings(settings: &JsonValue) -> Result<(), AppError> {
        write_json_file(&Self::settings_path(), settings)
    }
}

#[async_trait]
impl CliConfigurator for ClaudeAdapter {
    fn tool_id(&self) -> &str { "claude" }

    fn display_name(&self) -> &str { "Claude Code" }

    fn description(&self) -> &str {
        "Anthropic 官方 AI 编程助手，支持 agent 模式、MCP 工具"
    }

    fn config_path(&self) -> PathBuf {
        Self::settings_path()
    }

    fn homepage(&self) -> Option<String> {
        Some("https://claude.ai/code".to_string())
    }

    fn is_installed(&self) -> bool {
        Self::settings_path().exists()
    }

    fn is_taken_over(&self) -> Result<bool, AppError> {
        if let Some(settings) = Self::read_settings()? {
            let is_proxy = settings
                .get("env")
                .and_then(|v| v.get("ANTHROPIC_BASE_URL"))
                .and_then(|v| v.as_str())
                .map(|url| url.contains("127.0.0.1") || url.contains("localhost"))
                .unwrap_or(false);

            let is_uniroute_auth = settings
                .get("env")
                .and_then(|v| v.get("ANTHROPIC_AUTH_TOKEN"))
                .and_then(|v| v.as_str())
                .map(|token| token == "uniroute")
                .unwrap_or(false);

            Ok(is_proxy && is_uniroute_auth)
        } else {
            Ok(false)
        }
    }

    fn snapshot(&self) -> Result<ConfigSnapshot, AppError> {
        let mut files = HashMap::new();
        snapshot::snapshot_file(&Self::settings_path(), &mut files)?;
        Ok(snapshot::create_snapshot(self.tool_id(), files))
    }

    fn takeover(&self, proxy_url: &str, api_key: &str, model: &str) -> Result<(), AppError> {
        let mut settings = Self::read_settings()?
            .unwrap_or_else(|| json!({}));

        if !settings.get("env").map(|v| v.is_object()).unwrap_or(false) {
            settings["env"] = json!({});
        }

        let env = settings["env"]
            .as_object_mut()
            .expect("env must be object");

        let clean_url = proxy_url.trim_end_matches("/v1").trim_end_matches('/');
        env.insert("ANTHROPIC_BASE_URL".to_string(), json!(clean_url));
        env.insert("ANTHROPIC_AUTH_TOKEN".to_string(), json!(api_key));
        env.insert("ANTHROPIC_MODEL".to_string(), json!(model));
        env.insert("ANTHROPIC_DEFAULT_OPUS_MODEL".to_string(), json!(model));
        env.insert("ANTHROPIC_DEFAULT_SONNET_MODEL".to_string(), json!(model));
        env.insert("ANTHROPIC_DEFAULT_HAIKU_MODEL".to_string(), json!(model));

        if let Some(root) = settings.as_object_mut() {
            root.insert("primaryApiKey".to_string(), json!("any"));
        }

        Self::write_settings(&settings)?;
        tracing::info!("Claude Code 配置已接管: proxy={clean_url}, model={model}");
        Ok(())
    }

    fn restore(&self, snapshot: &ConfigSnapshot) -> Result<(), AppError> {
        snapshot::restore_file(&Self::settings_path(), &snapshot.files)?;
        tracing::info!("Claude Code 配置已恢复");
        Ok(())
    }

    fn get_current_model(&self) -> Result<Option<String>, AppError> {
        if let Some(settings) = Self::read_settings()? {
            let model = settings
                .get("env")
                .and_then(|v| v.get("ANTHROPIC_MODEL"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            Ok(model)
        } else {
            Ok(None)
        }
    }

    fn required_endpoint_type(&self) -> Option<&str> { Some("claude") }
}

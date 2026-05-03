//! Codex CLI 配置适配器
//!
//! 配置文件: ~/.codex/auth.json + config.toml
//! 接管时修改 base_url 和 API Key

use super::{get_home_dir, read_json_file, write_json_file};
use crate::cli_config::snapshot;
use crate::cli_config::traits::CliConfigurator;
use crate::cli_config::types::ConfigSnapshot;
use crate::error::AppError;
use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

pub struct CodexAdapter;

impl CodexAdapter {
    pub fn new() -> Self {
        Self
    }

    fn config_dir() -> PathBuf {
        get_home_dir().join(".codex")
    }

    fn auth_path() -> PathBuf {
        Self::config_dir().join("auth.json")
    }

    fn config_toml_path() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    fn read_auth() -> Result<Option<serde_json::Value>, AppError> {
        read_json_file(&Self::auth_path())
    }

    fn read_config_toml() -> Result<Option<String>, AppError> {
        let path = Self::config_toml_path();
        if path.exists() {
            fs::read_to_string(&path)
                .map(Some)
                .map_err(|e| AppError::Config(format!("读取 Codex config.toml 失败: {e}")))
        } else {
            Ok(None)
        }
    }
}

#[async_trait]
impl CliConfigurator for CodexAdapter {
    fn tool_id(&self) -> &str { "codex" }

    fn display_name(&self) -> &str { "Codex CLI" }

    fn description(&self) -> &str {
        "OpenAI 官方 AI 编程助手，支持 agent 模式"
    }

    fn config_path(&self) -> PathBuf {
        Self::config_dir()
    }

    fn homepage(&self) -> Option<String> {
        Some("https://github.com/openai/codex".to_string())
    }

    fn is_installed(&self) -> bool {
        Self::config_dir().exists()
    }

    fn is_taken_over(&self) -> Result<bool, AppError> {
        if let Some(auth) = Self::read_auth()? {
            let is_uniroute = auth
                .get("OPENAI_API_KEY")
                .and_then(|v| v.as_str())
                .map(|k| k == "uniroute")
                .unwrap_or(false);
            Ok(is_uniroute)
        } else {
            Ok(false)
        }
    }

    fn snapshot(&self) -> Result<ConfigSnapshot, AppError> {
        let mut files = HashMap::new();
        snapshot::snapshot_file(&Self::auth_path(), &mut files)?;
        snapshot::snapshot_file(&Self::config_toml_path(), &mut files)?;
        Ok(snapshot::create_snapshot(self.tool_id(), files))
    }

    fn takeover(&self, proxy_url: &str, api_key: &str, model: &str) -> Result<(), AppError> {
        let auth = json!({ "OPENAI_API_KEY": api_key });
        write_json_file(&Self::auth_path(), &auth)?;

        let config_toml = format!(
            r#"model_provider = "uniroute"
model = "{model}"

[model_providers.uniroute]
name = "UniRoute"
base_url = "{proxy_url}"
wire_api = "responses"
requires_openai_auth = true"#,
            model = model,
            proxy_url = proxy_url.trim_end_matches('/'),
        );
        fs::write(Self::config_toml_path(), config_toml)
            .map_err(|e| AppError::Config(format!("写入 Codex config.toml 失败: {e}")))?;

        tracing::info!("Codex CLI 配置已接管: proxy={proxy_url}, model={model}");
        Ok(())
    }

    fn restore(&self, snapshot: &ConfigSnapshot) -> Result<(), AppError> {
        snapshot::restore_file(&Self::auth_path(), &snapshot.files)?;
        snapshot::restore_file(&Self::config_toml_path(), &snapshot.files)?;
        tracing::info!("Codex CLI 配置已恢复");
        Ok(())
    }

    fn get_current_model(&self) -> Result<Option<String>, AppError> {
        if let Some(toml_str) = Self::read_config_toml()? {
            for line in toml_str.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("model ") || trimmed.starts_with("model=") {
                    if let Some(value) = trimmed.split_once('=').map(|(_, v)| v.trim().trim_matches('"')) {
                        return Ok(Some(value.to_string()));
                    }
                }
            }
        }
        Ok(None)
    }

    fn required_endpoint_type(&self) -> Option<&str> { Some("responses") }
}

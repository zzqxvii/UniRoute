//! pi / gsd-pi (Craft Agent) CLI 配置适配器
//!
//! 配置文件: ~/.craft-agent/config.json
//! 接管策略: Additive 模式 — 在 llmConnections 中添加 UniRoute 连接，切换 defaultLlmConnection

use super::{get_home_dir, read_json_file, write_json_file};
use crate::cli_config::snapshot;
use crate::cli_config::traits::CliConfigurator;
use crate::cli_config::types::ConfigSnapshot;
use crate::error::AppError;
use async_trait::async_trait;
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;
use std::path::PathBuf;

const UNIROUTE_SLUG: &str = "uniroute";

pub struct PiAdapter;

impl PiAdapter {
    pub fn new() -> Self {
        Self
    }

    fn config_dir() -> PathBuf {
        get_home_dir().join(".craft-agent")
    }

    fn config_file_path() -> PathBuf {
        Self::config_dir().join("config.json")
    }

    fn read_config() -> Result<Option<JsonValue>, AppError> {
        read_json_file(&Self::config_file_path())
    }

    fn write_config(config: &JsonValue) -> Result<(), AppError> {
        write_json_file(&Self::config_file_path(), config)
    }

    fn build_uniroute_connection(proxy_url: &str, _api_key: &str, model: &str) -> JsonValue {
        json!({
            "slug": UNIROUTE_SLUG,
            "name": "UniRoute",
            "providerType": "pi_compat",
            "authType": "api_key_with_endpoint",
            "baseUrl": proxy_url,
            "models": [model],
            "defaultModel": model,
            "modelSelectionMode": "userDefined3Tier",
            "piAuthProvider": "openai",
            "customEndpoint": {
                "api": "openai-completions"
            }
        })
    }
}

#[async_trait]
impl CliConfigurator for PiAdapter {
    fn tool_id(&self) -> &str { "pi" }

    fn display_name(&self) -> &str { "pi (Craft Agent)" }

    fn description(&self) -> &str {
        "AI Agent 编程助手，支持多工作区、多模型"
    }

    fn config_path(&self) -> PathBuf {
        Self::config_file_path()
    }

    fn homepage(&self) -> Option<String> {
        Some("https://craft.ai".to_string())
    }

    fn is_installed(&self) -> bool {
        Self::config_file_path().exists()
    }

    fn is_taken_over(&self) -> Result<bool, AppError> {
        if let Some(config) = Self::read_config()? {
            let has_uniroute = config
                .get("llmConnections")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().any(|conn| {
                    conn.get("slug").and_then(|v| v.as_str()) == Some(UNIROUTE_SLUG)
                }))
                .unwrap_or(false);

            let is_default = config
                .get("defaultLlmConnection")
                .and_then(|v| v.as_str())
                .map(|s| s == UNIROUTE_SLUG)
                .unwrap_or(false);

            Ok(has_uniroute && is_default)
        } else {
            Ok(false)
        }
    }

    fn snapshot(&self) -> Result<ConfigSnapshot, AppError> {
        let mut files = HashMap::new();
        let mut metadata = HashMap::new();

        snapshot::snapshot_file(&Self::config_file_path(), &mut files)?;

        if let Some(config) = Self::read_config()? {
            if let Some(default_conn) = config.get("defaultLlmConnection")
                .and_then(|v| v.as_str())
            {
                metadata.insert("defaultLlmConnection".to_string(), default_conn.to_string());
            }
        }

        let mut snap = snapshot::create_snapshot(self.tool_id(), files);
        snap.metadata = metadata;
        Ok(snap)
    }

    fn takeover(&self, proxy_url: &str, api_key: &str, model: &str) -> Result<(), AppError> {
        let mut config = Self::read_config()?
            .unwrap_or_else(|| json!({
                "llmConnections": [],
                "defaultLlmConnection": null
            }));

        let uniroute_conn = Self::build_uniroute_connection(proxy_url, api_key, model);

        let connections = config["llmConnections"]
            .as_array_mut()
            .ok_or_else(|| AppError::Config("pi 配置中 llmConnections 格式错误".to_string()))?;

        if let Some(existing) = connections.iter_mut().find(|c| {
            c.get("slug").and_then(|v| v.as_str()) == Some(UNIROUTE_SLUG)
        }) {
            *existing = uniroute_conn;
        } else {
            connections.push(uniroute_conn);
        }

        if let Some(root) = config.as_object_mut() {
            root.insert("defaultLlmConnection".to_string(), json!(UNIROUTE_SLUG));
        }

        Self::write_config(&config)?;
        tracing::info!("pi 配置已接管: proxy={proxy_url}, model={model}");
        Ok(())
    }

    fn restore(&self, snapshot: &ConfigSnapshot) -> Result<(), AppError> {
        snapshot::restore_file(&Self::config_file_path(), &snapshot.files)?;
        tracing::info!("pi 配置已恢复");
        Ok(())
    }

    fn get_current_model(&self) -> Result<Option<String>, AppError> {
        if let Some(config) = Self::read_config()? {
            if let Some(connections) = config.get("llmConnections").and_then(|v| v.as_array()) {
                for conn in connections {
                    if conn.get("slug").and_then(|v| v.as_str()) == Some(UNIROUTE_SLUG) {
                        let model = conn.get("defaultModel")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        return Ok(model);
                    }
                }
            }
        }
        Ok(None)
    }
}

//! droid CLI 配置适配器
//!
//! 配置文件: ~/.droid/config.json (推测)
//! 接管策略: 修改 base_url 和 model 字段

use super::{get_home_dir, read_json_file, write_json_file};
use crate::cli_config::snapshot;
use crate::cli_config::traits::CliConfigurator;
use crate::cli_config::types::ConfigSnapshot;
use crate::error::AppError;
use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;

pub struct DroidAdapter;

impl DroidAdapter {
    pub fn new() -> Self {
        Self
    }

    fn config_dir() -> PathBuf {
        get_home_dir().join(".droid")
    }

    fn config_file_path() -> PathBuf {
        Self::config_dir().join("config.json")
    }

    fn read_config() -> Result<Option<serde_json::Value>, AppError> {
        read_json_file(&Self::config_file_path())
    }

    fn write_config(config: &serde_json::Value) -> Result<(), AppError> {
        write_json_file(&Self::config_file_path(), config)
    }
}

#[async_trait]
impl CliConfigurator for DroidAdapter {
    fn tool_id(&self) -> &str { "droid" }

    fn display_name(&self) -> &str { "Droid CLI" }

    fn description(&self) -> &str {
        "AI 编程助手 CLI，支持多模型"
    }

    fn config_path(&self) -> PathBuf {
        Self::config_file_path()
    }

    fn homepage(&self) -> Option<String> {
        None
    }

    fn is_installed(&self) -> bool {
        Self::config_file_path().exists()
    }

    fn is_taken_over(&self) -> Result<bool, AppError> {
        if let Some(config) = Self::read_config()? {
            let is_proxy = config
                .get("base_url")
                .or_else(|| config.get("provider").and_then(|p| p.get("base_url")))
                .and_then(|v| v.as_str())
                .map(|url| url.contains("127.0.0.1") || url.contains("localhost"))
                .unwrap_or(false);

            Ok(is_proxy)
        } else {
            Ok(false)
        }
    }

    fn snapshot(&self) -> Result<ConfigSnapshot, AppError> {
        let mut files = HashMap::new();
        snapshot::snapshot_file(&Self::config_file_path(), &mut files)?;
        Ok(snapshot::create_snapshot(self.tool_id(), files))
    }

    fn takeover(&self, proxy_url: &str, api_key: &str, model: &str) -> Result<(), AppError> {
        let mut config = Self::read_config()?
            .unwrap_or_else(|| json!({}));

        if let Some(root) = config.as_object_mut() {
            if !root.contains_key("provider") {
                root.insert("base_url".to_string(), json!(proxy_url));
                root.insert("api_key".to_string(), json!(api_key));
                root.insert("model".to_string(), json!(model));
            }

            if let Some(provider) = root.get_mut("provider").and_then(|v| v.as_object_mut()) {
                provider.insert("base_url".to_string(), json!(proxy_url));
                provider.insert("api_key".to_string(), json!(api_key));
                provider.insert("model".to_string(), json!(model));
            }
        }

        Self::write_config(&config)?;
        tracing::info!("droid CLI 配置已接管: proxy={proxy_url}, model={model}");
        Ok(())
    }

    fn restore(&self, snapshot: &ConfigSnapshot) -> Result<(), AppError> {
        snapshot::restore_file(&Self::config_file_path(), &snapshot.files)?;
        tracing::info!("droid CLI 配置已恢复");
        Ok(())
    }

    fn get_current_model(&self) -> Result<Option<String>, AppError> {
        if let Some(config) = Self::read_config()? {
            let model = config
                .get("model")
                .or_else(|| config.get("provider").and_then(|p| p.get("model")))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            Ok(model)
        } else {
            Ok(None)
        }
    }
}

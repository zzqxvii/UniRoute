//! CLI 配置管理器
//!
//! 管理所有 CLI 工具配置器的注册、接管、恢复、快照生命周期。

use crate::cli_config::adapters::{claude::ClaudeAdapter, codex::CodexAdapter, droid::DroidAdapter, gsd::GsdAdapter, pi::PiAdapter};
use crate::cli_config::adapters::get_home_dir;
use crate::cli_config::traits::CliConfigurator;
use crate::cli_config::types::*;
use crate::error::AppError;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// CLI 配置管理器
pub struct CliConfigManager {
    /// 注册的配置器
    configurators: Vec<Box<dyn CliConfigurator>>,
    /// 当前接管状态（tool_id -> TakeoverState）
    takeover_states: parking_lot::RwLock<HashMap<String, TakeoverStateInternal>>,
    /// 快照持久化目录（预留）
    _snapshot_dir: PathBuf,
    /// 数据库引用（用于持久化配置）
    db: Arc<crate::storage::Database>,
    /// 全局设置
    settings: parking_lot::RwLock<CliGlobalSettings>,
}

#[derive(Debug, Clone)]
struct TakeoverStateInternal {
    snapshot: ConfigSnapshot,
    proxy_url: String,
    source_type: String,
    source_value: String,
    taken_over_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliGlobalSettings {
    pub auto_takeover_on_start: bool,
    pub auto_restore_on_stop: bool,
    pub api_key: String,
}

impl Default for CliGlobalSettings {
    fn default() -> Self {
        Self {
            auto_takeover_on_start: false,
            auto_restore_on_stop: false,
            api_key: "uniroute".to_string(),
        }
    }
}

impl CliConfigManager {
    /// 创建管理器并注册内置适配器
    pub fn new(db: Arc<crate::storage::Database>) -> Self {
        let snapshot_dir = get_home_dir()
            .join(".uniroute")
            .join("cli_snapshots");

        let mut manager = Self {
            configurators: Vec::new(),
            takeover_states: parking_lot::RwLock::new(HashMap::new()),
            _snapshot_dir: snapshot_dir,
            db,
            settings: parking_lot::RwLock::new(CliGlobalSettings::default()),
        };

        // 注册内置适配器
        manager.register(Box::new(ClaudeAdapter::new()));
        manager.register(Box::new(CodexAdapter::new()));
        manager.register(Box::new(PiAdapter::new()));
        manager.register(Box::new(DroidAdapter::new()));
        manager.register(Box::new(GsdAdapter::new()));

        manager
    }

    /// 注册配置器
    pub fn register(&mut self, configurator: Box<dyn CliConfigurator>) {
        self.configurators.push(configurator);
    }

    /// 获取所有支持的 CLI 工具信息
    pub fn get_supported_tools(&self) -> Vec<CliToolInfo> {
        self.configurators.iter().map(|c| c.info()).collect()
    }

    /// 获取所有工具的状态（包括安装检测）
    pub fn get_all_status(&self) -> Vec<CliToolStatus> {
        let takeover_states = self.takeover_states.read();
        self.configurators
            .iter()
            .map(|c| {
                let tool_id = c.tool_id().to_string();
                let taken_over = takeover_states.contains_key(&tool_id);
                let state = takeover_states.get(&tool_id);

                CliToolStatus {
                    tool_id: tool_id.clone(),
                    display_name: c.display_name().to_string(),
                    description: c.description().to_string(),
                    installed: c.is_installed(),
                    taken_over,
                    proxy_url: state.map(|s| s.proxy_url.clone()),
                    source_type: state.map(|s| s.source_type.clone()),
                    source_value: state.map(|s| s.source_value.clone()),
                    taken_over_at: state.map(|s| s.taken_over_at.clone()),
                    config_path: c.config_path().to_string_lossy().to_string(),
                    homepage: c.homepage(),
                    required_endpoint_type: c.required_endpoint_type().map(|s| s.to_string()),
                }
            })
            .collect()
    }

    /// 获取单个工具状态
    pub fn get_tool_status(&self, tool_id: &str) -> Option<CliToolStatus> {
        self.configurators
            .iter()
            .find(|c| c.tool_id() == tool_id)
            .map(|c| {
                let takeover_states = self.takeover_states.read();
                let taken_over = takeover_states.contains_key(tool_id);
                let state = takeover_states.get(tool_id);

                CliToolStatus {
                    tool_id: tool_id.to_string(),
                    display_name: c.display_name().to_string(),
                    description: c.description().to_string(),
                    installed: c.is_installed(),
                    taken_over,
                    proxy_url: state.map(|s| s.proxy_url.clone()),
                    source_type: state.map(|s| s.source_type.clone()),
                    source_value: state.map(|s| s.source_value.clone()),
                    taken_over_at: state.map(|s| s.taken_over_at.clone()),
                    config_path: c.config_path().to_string_lossy().to_string(),
                    homepage: c.homepage(),
                    required_endpoint_type: c.required_endpoint_type().map(|s| s.to_string()),
                }
            })
    }

    /// 获取配置器
    fn get_configurator(&self, tool_id: &str) -> Option<&dyn CliConfigurator> {
        self.configurators.iter().find(|c| c.tool_id() == tool_id).map(|c| c.as_ref())
    }

    /// 接管单个 CLI 工具
    pub fn takeover_tool(
        &self,
        tool_id: &str,
        proxy_url: &str,
        source_type: &str,
        source_value: &str,
    ) -> Result<TakeoverResult, AppError> {
        let configurator = self.get_configurator(tool_id)
            .ok_or_else(|| AppError::Config(format!("未知的 CLI 工具: {tool_id}")))?;

        let settings = self.settings.read();
        let api_key = settings.api_key.clone();
        drop(settings);

        // 1. 保存快照
        let snapshot = configurator.snapshot()?;

        // 2. 接管
        configurator.takeover(proxy_url, &api_key, source_value)?;

        // 3. 持久化快照到数据库
        let snapshot_id = format!("{}_{}", tool_id, chrono::Utc::now().format("%Y%m%d%H%M%S"));
        let snapshot_json = serde_json::to_string(&snapshot)
            .unwrap_or_else(|_| "{}".to_string());
        if let Err(e) = self.db.save_cli_config_snapshot(
            &snapshot_id, tool_id, &snapshot_json, &snapshot.created_at,
        ) {
            tracing::warn!("持久化快照失败: {e}");
        }

        // 4. 记录接管状态
        let taken_over_at = chrono::Utc::now().to_rfc3339();
        self.takeover_states.write().insert(
            tool_id.to_string(),
            TakeoverStateInternal {
                snapshot,
                proxy_url: proxy_url.to_string(),
                source_type: source_type.to_string(),
                source_value: source_value.to_string(),
                taken_over_at: taken_over_at.clone(),
            },
        );

        tracing::info!("CLI 工具 {tool_id} 接管成功: source={source_type}:{source_value}");
        Ok(TakeoverResult {
            tool_id: tool_id.to_string(),
            success: true,
            message: format!("接管成功 ({})", configurator.display_name()),
        })
    }

    /// 接管所有已启用的工具
    pub fn takeover_all_enabled(
        &self,
        proxy_url: &str,
        tool_configs: &HashMap<String, CliToolConfig>,
    ) -> Vec<TakeoverResult> {
        let mut results = Vec::new();

        for configurator in &self.configurators {
            let tool_id = configurator.tool_id();
            let config = tool_configs.get(tool_id);

            // 只接管已启用 + 自动接管的工具
            if let Some(cfg) = config {
                if !cfg.enabled || !cfg.auto_takeover {
                    continue;
                }
            } else {
                continue;
            }

            // 只接管已安装的工具
            if !configurator.is_installed() {
                results.push(TakeoverResult {
                    tool_id: tool_id.to_string(),
                    success: false,
                    message: "工具未安装".to_string(),
                });
                continue;
            }

            let config = tool_configs.get(tool_id).unwrap();
            match self.takeover_tool(tool_id, proxy_url, &config.source_type, &config.source_value) {
                Ok(r) => results.push(r),
                Err(e) => results.push(TakeoverResult {
                    tool_id: tool_id.to_string(),
                    success: false,
                    message: format!("接管失败: {e}"),
                }),
            }
        }

        results
    }

    /// 恢复单个 CLI 工具
    pub fn restore_tool(&self, tool_id: &str) -> Result<TakeoverResult, AppError> {
        let configurator = self.get_configurator(tool_id)
            .ok_or_else(|| AppError::Config(format!("未知的 CLI 工具: {tool_id}")))?;

        let state = {
            let states = self.takeover_states.read();
            states.get(tool_id).cloned()
        };

        match state {
            Some(state) => {
                configurator.restore(&state.snapshot)?;
                self.takeover_states.write().remove(tool_id);
                tracing::info!("CLI 工具 {tool_id} 恢复成功");
                Ok(TakeoverResult {
                    tool_id: tool_id.to_string(),
                    success: true,
                    message: format!("已恢复 ({})", configurator.display_name()),
                })
            }
            None => Ok(TakeoverResult {
                tool_id: tool_id.to_string(),
                success: false,
                message: "未处于接管状态".to_string(),
            }),
        }
    }

    /// 恢复所有已接管的工具
    pub fn restore_all(&self) -> Vec<TakeoverResult> {
        let tool_ids: Vec<String> = {
            self.takeover_states.read().keys().cloned().collect()
        };

        tool_ids
            .into_iter()
            .map(|id| match self.restore_tool(&id) {
                Ok(r) => r,
                Err(e) => TakeoverResult {
                    tool_id: id.clone(),
                    success: false,
                    message: format!("恢复失败: {e}"),
                },
            })
            .collect()
    }

    /// 更新已接管工具的模型（不恢复再接管，直接修改配置）
    pub fn update_model(
        &self,
        tool_id: &str,
        source_type: &str,
        source_value: &str,
    ) -> Result<(), AppError> {
        let configurator = self.get_configurator(tool_id)
            .ok_or_else(|| AppError::Config(format!("未知的 CLI 工具: {tool_id}")))?;

        let proxy_url = {
            let states = self.takeover_states.read();
            states.get(tool_id).map(|s| s.proxy_url.clone())
        };

        if let Some(url) = proxy_url {
            let settings = self.settings.read();
            let api_key = settings.api_key.clone();
            drop(settings);
            configurator.takeover(&url, &api_key, source_value)?;

            // 更新接管状态
            if let Some(state) = self.takeover_states.write().get_mut(tool_id) {
                state.source_type = source_type.to_string();
                state.source_value = source_value.to_string();
            }

            tracing::info!("CLI 工具 {tool_id} 模型已更新: {source_type}:{source_value}");
            Ok(())
        } else {
            Err(AppError::Config(format!("工具 {tool_id} 未处于接管状态")))
        }
    }

    /// 获取全局设置
    pub fn get_global_settings(&self) -> CliGlobalSettings {
        self.settings.read().clone()
    }

    /// 更新全局设置
    pub fn update_global_settings(&self, settings: CliGlobalSettings) {
        *self.settings.write() = settings;
    }

    /// 获取某个工具的配置
    pub fn get_tool_config(&self, tool_id: &str) -> Option<CliToolConfig> {
        // 从数据库或内存读取
        self.db.load_cli_tool_config(tool_id).ok().flatten()
    }

    /// 获取所有工具的配置
    pub fn get_all_tool_configs(&self) -> HashMap<String, CliToolConfig> {
        self.db.load_all_cli_tool_configs().unwrap_or_default()
    }

    /// 保存工具配置
    pub fn save_tool_config(&self, config: &CliToolConfig) -> Result<(), AppError> {
        self.db.save_cli_tool_config(config)
            .map_err(|e| AppError::Storage(anyhow::anyhow!("保存 CLI 工具配置失败: {e}")))
    }

    /// 获取当前配置文件内容（用于 UI 展示）
    pub fn get_current_config(&self, tool_id: &str) -> Result<Vec<ConfigFileEntry>, AppError> {
        let configurator = self.get_configurator(tool_id)
            .ok_or_else(|| AppError::Config(format!("未知的 CLI 工具: {tool_id}")))?;

        let config_path = configurator.config_path();
        let mut files = Vec::new();

        if config_path.is_dir() {
            // 目录型配置（如 codex、gsd）：列出并读取所有配置文件
            if let Ok(entries) = std::fs::read_dir(&config_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        if let Some(ext) = path.extension() {
                            if ext == "json" || ext == "toml" || ext == "yaml" || ext == "yml" {
                                let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                                let content = std::fs::read_to_string(&path)
                                    .unwrap_or_else(|e| format!("读取失败: {e}"));
                                files.push(ConfigFileEntry { filename, content });
                            }
                        }
                    }
                }
            }
        } else if config_path.exists() {
            // 单文件型配置（如 claude）
            let filename = config_path.file_name()
                .unwrap_or_default().to_string_lossy().to_string();
            let content = std::fs::read_to_string(&config_path)
                .unwrap_or_else(|e| format!("读取失败: {e}"));
            files.push(ConfigFileEntry { filename, content });
        }

        Ok(files)
    }

    /// 列出某工具的已保存快照
    pub fn list_saved_snapshots(&self, tool_id: &str) -> Vec<SnapshotInfo> {
        self.db.list_cli_config_snapshots(tool_id).unwrap_or_default()
    }

    /// 获取快照的配置内容（用于预览）
    pub fn get_snapshot_content(&self, snapshot_id: &str) -> Result<Vec<ConfigFileEntry>, AppError> {
        let snapshot_json = self.db.load_cli_config_snapshot(snapshot_id)?
            .ok_or_else(|| AppError::Config(format!("快照不存在: {snapshot_id}")))?;

        let snapshot: ConfigSnapshot = serde_json::from_str(&snapshot_json)
            .map_err(|e| AppError::Config(format!("解析快照失败: {e}")))?;

        let mut files = Vec::new();
        for (path, content) in &snapshot.files {
            let filename = path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let content_str = String::from_utf8_lossy(content).into_owned();
            files.push(ConfigFileEntry { filename, content: content_str });
        }
        Ok(files)
    }

    /// 从数据库快照恢复
    pub fn restore_from_saved_snapshot(&self, tool_id: &str, snapshot_id: &str) -> Result<TakeoverResult, AppError> {
        let configurator = self.get_configurator(tool_id)
            .ok_or_else(|| AppError::Config(format!("未知的 CLI 工具: {tool_id}")))?;

        let snapshot_json = self.db.load_cli_config_snapshot(snapshot_id)?
            .ok_or_else(|| AppError::Config(format!("快照不存在: {snapshot_id}")))?;

        let snapshot: ConfigSnapshot = serde_json::from_str(&snapshot_json)
            .map_err(|e| AppError::Config(format!("解析快照失败: {e}")))?;

        configurator.restore(&snapshot)?;

        // 清除内存中的接管状态（如果存在）
        self.takeover_states.write().remove(tool_id);

        tracing::info!("CLI 工具 {tool_id} 从快照 {snapshot_id} 恢复成功");
        Ok(TakeoverResult {
            tool_id: tool_id.to_string(),
            success: true,
            message: format!("已从快照恢复 ({})", configurator.display_name()),
        })
    }
}

//! CLI 工具配置器 trait

use crate::error::AppError;
use async_trait::async_trait;
use std::path::PathBuf;

use super::types::{CliToolInfo, ConfigSnapshot};

/// CLI 工具配置器 trait
///
/// 每个支持的 CLI 工具实现此 trait，提供接管/恢复能力。
#[async_trait]
pub trait CliConfigurator: Send + Sync {
    /// 工具标识符（如 "claude", "codex", "pi", "droid"）
    fn tool_id(&self) -> &str;

    /// 显示名称
    fn display_name(&self) -> &str;

    /// 工具描述
    fn description(&self) -> &str;

    /// 获取工具信息
    fn info(&self) -> CliToolInfo {
        CliToolInfo {
            tool_id: self.tool_id().to_string(),
            display_name: self.display_name().to_string(),
            description: self.description().to_string(),
            config_path: self.config_path().to_string_lossy().to_string(),
            homepage: self.homepage(),
        }
    }

    /// 配置文件路径
    fn config_path(&self) -> PathBuf;

    /// 官方网站（可选）
    fn homepage(&self) -> Option<String> { None }

    /// 检测工具是否已安装
    fn is_installed(&self) -> bool;

    /// 检测当前配置是否已被接管
    fn is_taken_over(&self) -> Result<bool, AppError>;

    /// 保存当前配置快照（用于接管前备份）
    fn snapshot(&self) -> Result<ConfigSnapshot, AppError>;

    /// 写入 UniRoute 代理配置（接管）
    ///
    /// # 参数
    /// - `proxy_url`: 代理地址 (如 "http://localhost:8080/v1")
    /// - `api_key`: API Key（对 UniRoute 来说通常是 "uniroute"）
    /// - `model`: 模型名（Group 名称 或 具体模型名）
    fn takeover(&self, proxy_url: &str, api_key: &str, model: &str) -> Result<(), AppError>;

    /// 从快照恢复原始配置
    fn restore(&self, snapshot: &ConfigSnapshot) -> Result<(), AppError>;

    /// 获取当前配置中的模型名
    fn get_current_model(&self) -> Result<Option<String>, AppError>;

    /// 此工具要求的 Group endpoint_type（用于过滤可选 Group）
    ///
    /// - `Some("messages")` → 只能选 endpoint_type = "messages" 的 Group
    /// - `Some("responses")` → 只能选 endpoint_type = "responses" 的 Group
    /// - `None` → 不限制，可选任意 Group
    fn required_endpoint_type(&self) -> Option<&str> { None }
}

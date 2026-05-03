//! CLI 配置模块共享类型

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// CLI 工具信息（用于 UI 展示）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliToolInfo {
    /// 工具标识符
    pub tool_id: String,
    /// 显示名称
    pub display_name: String,
    /// 工具描述
    pub description: String,
    /// 配置文件路径
    pub config_path: String,
    /// 官方网址
    pub homepage: Option<String>,
}

/// CLI 工具实时状态
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliToolStatus {
    pub tool_id: String,
    pub display_name: String,
    pub description: String,
    /// 是否已安装
    pub installed: bool,
    /// 是否已被接管
    pub taken_over: bool,
    /// 接管时的代理 URL
    pub proxy_url: Option<String>,
    /// 模型源类型: "group" | "model"
    pub source_type: Option<String>,
    /// 模型源值: Group 名称 或 模型全名
    pub source_value: Option<String>,
    /// 接管时间
    pub taken_over_at: Option<String>,
    /// 配置文件路径
    pub config_path: String,
    pub homepage: Option<String>,
    /// 此工具要求的 Group endpoint_type（如 "messages"、"responses"）
    pub required_endpoint_type: Option<String>,
}

/// CLI 工具配置（持久化到数据库）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliToolConfig {
    pub tool_id: String,
    /// 是否启用此工具
    pub enabled: bool,
    /// 代理启动时是否自动接管
    pub auto_takeover: bool,
    /// 模型源类型: "group" | "model"
    pub source_type: String,
    /// 模型源值
    pub source_value: String,
}

impl Default for CliToolConfig {
    fn default() -> Self {
        Self {
            tool_id: String::new(),
            enabled: true,
            auto_takeover: true,
            source_type: "group".to_string(),
            source_value: "free".to_string(),
        }
    }
}

/// 配置快照（接管前备份）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSnapshot {
    /// 工具 ID
    pub tool_id: String,
    /// 快照创建时间
    pub created_at: String,
    /// 原始文件内容（路径 -> 字节）
    pub files: HashMap<PathBuf, Vec<u8>>,
    /// 额外元数据（如 pi 的 defaultLlmConnection 原始值）
    pub metadata: HashMap<String, String>,
}

/// 接管结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TakeoverResult {
    pub tool_id: String,
    pub success: bool,
    pub message: String,
}

/// 快照元数据（用于列表展示，不含文件内容）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotInfo {
    pub id: String,
    pub tool_id: String,
    pub created_at: String,
    pub size_bytes: u64,
}

/// 单个配置文件内容
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFileEntry {
    pub filename: String,
    pub content: String,
}

/// 模型源类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ModelSource {
    /// 使用 Group 路由
    Group(String),
    /// 直接指定模型
    Model(String),
}

impl ModelSource {
    pub fn source_type(&self) -> &str {
        match self {
            ModelSource::Group(_) => "group",
            ModelSource::Model(_) => "model",
        }
    }

    pub fn source_value(&self) -> &str {
        match self {
            ModelSource::Group(v) | ModelSource::Model(v) => v.as_str(),
        }
    }
}

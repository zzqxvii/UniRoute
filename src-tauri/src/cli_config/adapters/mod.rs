pub mod claude;
pub mod codex;
pub mod pi;
pub mod droid;
pub mod gsd;

use crate::error::AppError;
use serde_json::Value as JsonValue;
use std::fs;
use std::path::{Path, PathBuf};

/// 获取用户主目录
pub fn get_home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// 确保目录存在
pub fn ensure_dir(path: &Path) -> Result<(), AppError> {
    if !path.exists() {
        fs::create_dir_all(path)
            .map_err(|e| AppError::Config(format!("创建目录失败 ({path:?}): {e}")))?;
    }
    Ok(())
}

/// 读取 JSON 文件，不存在时返回 None
pub fn read_json_file(path: &Path) -> Result<Option<JsonValue>, AppError> {
    if path.exists() {
        let content = fs::read_to_string(path)
            .map_err(|e| AppError::Config(format!("读取文件失败 ({path:?}): {e}")))?;
        let value: JsonValue = serde_json::from_str(&content)
            .map_err(|e| AppError::Config(format!("解析 JSON 失败 ({path:?}): {e}")))?;
        Ok(Some(value))
    } else {
        Ok(None)
    }
}

/// 写入 JSON 文件（自动创建目录）
pub fn write_json_file(path: &Path, value: &JsonValue) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    let content = serde_json::to_string_pretty(value)
        .map_err(|e| AppError::Config(format!("序列化 JSON 失败: {e}")))?;
    fs::write(path, content)
        .map_err(|e| AppError::Config(format!("写入文件失败 ({path:?}): {e}")))?;
    Ok(())
}

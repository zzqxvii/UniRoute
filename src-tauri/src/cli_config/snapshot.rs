//! 配置快照辅助函数

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::cli_config::types::ConfigSnapshot;
use crate::error::AppError;

/// 读取文件到快照
pub fn snapshot_file(path: &PathBuf, snapshot: &mut HashMap<PathBuf, Vec<u8>>) -> Result<(), AppError> {
    if path.exists() {
        let content = fs::read(path)
            .map_err(|e| AppError::Config(format!("备份文件失败 ({path:?}): {e}")))?;
        snapshot.insert(path.clone(), content);
    }
    Ok(())
}

/// 从快照恢复文件
pub fn restore_file(path: &PathBuf, snapshot: &HashMap<PathBuf, Vec<u8>>) -> Result<(), AppError> {
    if let Some(content) = snapshot.get(path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        fs::write(path, content)
            .map_err(|e| AppError::Config(format!("恢复文件失败 ({path:?}): {e}")))?;
    } else if path.exists() {
        // 快照中无此文件，删除
        fs::remove_file(path).ok();
    }
    Ok(())
}

/// 创建新快照
pub fn create_snapshot(tool_id: &str, files: HashMap<PathBuf, Vec<u8>>) -> ConfigSnapshot {
    ConfigSnapshot {
        tool_id: tool_id.to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
        files,
        metadata: HashMap::new(),
    }
}

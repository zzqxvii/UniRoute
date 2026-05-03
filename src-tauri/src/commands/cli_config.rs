//! CLI 工具配置 Tauri 命令

use crate::cli_config::manager::CliGlobalSettings;
use crate::cli_config::types::*;
use crate::state::{AppState, AppStateContainer};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::State;

fn get_state(container: &AppStateContainer) -> Option<Arc<AppState>> {
    container.try_get()
}

// ============ Tool List ============

/// 获取所有支持的 CLI 工具信息
#[tauri::command]
pub fn get_supported_cli_tools(state: State<'_, AppStateContainer>) -> Vec<CliToolInfo> {
    get_state(&state)
        .map(|s| s.cli_config_manager.get_supported_tools())
        .unwrap_or_default()
}

/// 获取所有 CLI 工具状态
#[tauri::command]
pub fn get_cli_tools_status(state: State<'_, AppStateContainer>) -> Vec<CliToolStatus> {
    get_state(&state)
        .map(|s| s.cli_config_manager.get_all_status())
        .unwrap_or_default()
}

/// 获取单个 CLI 工具状态
#[tauri::command]
pub fn get_cli_tool_status(tool_id: String, state: State<'_, AppStateContainer>) -> Option<CliToolStatus> {
    get_state(&state)
        .and_then(|s| s.cli_config_manager.get_tool_status(&tool_id))
}

// ============ Takeover / Restore ============

/// 接管 CLI 工具
#[tauri::command]
pub fn takeover_cli_tool(
    tool_id: String,
    proxy_url: String,
    source_type: String,
    source_value: String,
    state: State<'_, AppStateContainer>,
) -> Result<TakeoverResult, String> {
    let state = get_state(&state).ok_or("应用正在初始化")?;
    state.cli_config_manager
        .takeover_tool(&tool_id, &proxy_url, &source_type, &source_value)
        .map_err(|e| e.to_string())
}

/// 恢复 CLI 工具
#[tauri::command]
pub fn restore_cli_tool(
    tool_id: String,
    state: State<'_, AppStateContainer>,
) -> Result<TakeoverResult, String> {
    let state = get_state(&state).ok_or("应用正在初始化")?;
    state.cli_config_manager
        .restore_tool(&tool_id)
        .map_err(|e| e.to_string())
}

/// 恢复所有 CLI 工具
#[tauri::command]
pub fn restore_all_cli_tools(
    state: State<'_, AppStateContainer>,
) -> Vec<TakeoverResult> {
    get_state(&state)
        .map(|s| s.cli_config_manager.restore_all())
        .unwrap_or_default()
}

/// 更新已接管工具的模型
#[tauri::command]
pub fn update_cli_tool_model(
    tool_id: String,
    source_type: String,
    source_value: String,
    state: State<'_, AppStateContainer>,
) -> Result<(), String> {
    let state = get_state(&state).ok_or("应用正在初始化")?;
    state.cli_config_manager
        .update_model(&tool_id, &source_type, &source_value)
        .map_err(|e| e.to_string())
}

// ============ Config Management ============

/// 获取全局 CLI 配置
#[tauri::command]
pub fn get_cli_global_settings(state: State<'_, AppStateContainer>) -> CliGlobalSettings {
    get_state(&state)
        .map(|s| s.cli_config_manager.get_global_settings())
        .unwrap_or_default()
}

/// 更新全局 CLI 配置
#[tauri::command]
pub fn update_cli_global_settings(
    settings: CliGlobalSettings,
    state: State<'_, AppStateContainer>,
) -> Result<(), String> {
    let state = get_state(&state).ok_or("应用正在初始化")?;
    state.cli_config_manager.update_global_settings(settings);
    Ok(())
}

/// 获取 CLI 工具配置
#[tauri::command]
pub fn get_cli_tool_config(
    tool_id: String,
    state: State<'_, AppStateContainer>,
) -> Option<CliToolConfig> {
    get_state(&state)
        .and_then(|s| s.cli_config_manager.get_tool_config(&tool_id))
}

/// 获取所有 CLI 工具配置（批量）
#[tauri::command]
pub fn get_all_cli_tool_configs(
    state: State<'_, AppStateContainer>,
) -> HashMap<String, CliToolConfig> {
    get_state(&state)
        .map(|s| s.cli_config_manager.get_all_tool_configs())
        .unwrap_or_default()
}

/// 保存 CLI 工具配置
#[tauri::command]
pub fn save_cli_tool_config(
    config: CliToolConfig,
    state: State<'_, AppStateContainer>,
) -> Result<(), String> {
    let state = get_state(&state).ok_or("应用正在初始化")?;
    state.cli_config_manager
        .save_tool_config(&config)
        .map_err(|e| e.to_string())
}

// ============ Config Viewing & Snapshots ============

/// 获取当前 CLI 工具配置文件内容
#[tauri::command]
pub fn get_cli_tool_current_config(
    tool_id: String,
    state: State<'_, AppStateContainer>,
) -> Result<Vec<ConfigFileEntry>, String> {
    let state = get_state(&state).ok_or("应用正在初始化")?;
    state.cli_config_manager
        .get_current_config(&tool_id)
        .map_err(|e| e.to_string())
}

/// 列出 CLI 工具的已保存快照
#[tauri::command]
pub fn list_cli_tool_snapshots(
    tool_id: String,
    state: State<'_, AppStateContainer>,
) -> Vec<SnapshotInfo> {
    get_state(&state)
        .map(|s| s.cli_config_manager.list_saved_snapshots(&tool_id))
        .unwrap_or_default()
}

/// 获取快照的配置内容（用于预览）
#[tauri::command]
pub fn get_cli_tool_snapshot_content(
    snapshot_id: String,
    state: State<'_, AppStateContainer>,
) -> Result<Vec<ConfigFileEntry>, String> {
    let state = get_state(&state).ok_or("应用正在初始化")?;
    state.cli_config_manager
        .get_snapshot_content(&snapshot_id)
        .map_err(|e| e.to_string())
}

/// 从已保存快照恢复 CLI 工具配置
#[tauri::command]
pub fn restore_cli_tool_from_snapshot(
    tool_id: String,
    snapshot_id: String,
    state: State<'_, AppStateContainer>,
) -> Result<TakeoverResult, String> {
    let state = get_state(&state).ok_or("应用正在初始化")?;
    state.cli_config_manager
        .restore_from_saved_snapshot(&tool_id, &snapshot_id)
        .map_err(|e| e.to_string())
}

/// 打开 CLI 工具配置目录
#[tauri::command]
pub fn open_cli_config_dir(tool_id: String, state: State<'_, AppStateContainer>) -> Result<(), String> {
    let state = get_state(&state).ok_or("应用正在初始化")?;
    let status = state.cli_config_manager.get_tool_status(&tool_id)
        .ok_or_else(|| format!("未知的 CLI 工具: {tool_id}"))?;
    let path = std::path::PathBuf::from(&status.config_path);

    let dir = if path.is_dir() { path } else {
        path.parent().map(|p| p.to_path_buf()).unwrap_or(path)
    };

    // 确保目录存在
    std::fs::create_dir_all(&dir).ok();

    #[cfg(target_os = "windows")]
    std::process::Command::new("explorer").arg(&dir).spawn().ok();
    #[cfg(target_os = "macos")]
    std::process::Command::new("open").arg(&dir).spawn().ok();
    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open").arg(&dir).spawn().ok();

    Ok(())
}


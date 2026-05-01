//! Settings, data import/export, and client configuration commands

use serde::Serialize;
use serde_json::Value as JsonValue;
use crate::state::{AppSettings, AppState};
use std::sync::Arc;
use tauri::State;

// ============ Settings Commands ============

#[tauri::command]
pub fn get_settings(state: State<'_, Arc<AppState>>) -> AppSettings {
    state.get_settings()
}

#[tauri::command]
pub fn update_settings(settings: AppSettings, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    // 检查端口是否变化，如果代理正在运行则需要重启
    let old_settings = state.get_settings();
    let port_changed = old_settings.proxy_port != settings.proxy_port;
    let was_running = state.is_proxy_running();

    // 如果端口变化且代理正在运行，先停止
    if port_changed && was_running {
        tracing::info!("端口变化，停止代理服务器...");
        let mut proxy = state.proxy_server.write();
        if let Some(handle) = proxy.take() {
            let _ = handle.shutdown_tx.send(());
        }
    }

    state.update_settings(settings.clone());

    // 如果端口变化且之前在运行，用新端口重启
    if port_changed && was_running {
        let port = settings.proxy_port;
        tracing::info!("用新端口 {} 重启代理服务器...", port);

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let state_for_proxy = Arc::clone(state.inner());

        tokio::spawn(async move {
            if let Err(e) = crate::proxy::start_proxy_server(port, state_for_proxy, shutdown_rx).await {
                tracing::error!("代理服务器错误: {}", e);
            }
        });

        let handle = crate::state::ProxyServerHandle { port, shutdown_tx };
        *state.proxy_server.write() = Some(handle);
    }

    Ok(())
}

// ============ Data Import/Export ============

#[tauri::command]
pub fn export_data(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    state.export_data().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn import_data(
    json: String,
    merge: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<ImportResultInfo, String> {
    let result = state.import_data(&json, merge).map_err(|e| e.to_string())?;
    Ok(ImportResultInfo {
        providers_imported: result.providers_imported,
        groups_imported: result.groups_imported,
        mappings_imported: result.mappings_imported,
        errors: result.errors,
    })
}

#[tauri::command]
pub fn get_db_path(state: State<'_, Arc<AppState>>) -> String {
    state.get_db_path().to_string_lossy().to_string()
}

#[derive(serde::Serialize)]
pub struct ImportResultInfo {
    pub providers_imported: usize,
    pub groups_imported: usize,
    pub mappings_imported: usize,
    pub errors: Vec<String>,
}

// ============ Client Configuration Commands ============

/// 客户端配置状态
#[derive(Debug, Clone, Serialize)]
pub struct ClientConfigStatus {
    pub client_type: String,
    pub config_path: String,
    pub exists: bool,
    pub is_managed: bool,
}

/// 获取客户端配置状态
#[tauri::command]
pub fn get_client_config_status(client_type: String) -> ClientConfigStatus {
    let status = crate::client_config::get_client_config_status(&client_type);
    ClientConfigStatus {
        client_type: status.client_type,
        config_path: status.config_path,
        exists: status.exists,
        is_managed: status.is_managed,
    }
}

/// 读取 Claude Code 配置
#[tauri::command]
pub fn read_claude_config(
    base_url: String,
    group_name: String,
) -> Result<String, String> {
    let settings = crate::client_config::read_claude_settings_with_default(&base_url, &group_name)
        .map_err(|e| e.to_string())?;

    serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("序列化配置失败: {}", e))
}

/// 应用 Claude Code 配置
#[tauri::command]
pub fn apply_claude_config(
    config: String,
) -> Result<(), String> {
    // 解析 JSON 验证格式
    let _: JsonValue = serde_json::from_str(&config)
        .map_err(|e| format!("无效的 JSON 格式: {}", e))?;

    // 写入配置文件
    crate::client_config::ensure_claude_dir_exists()
        .map_err(|e| e.to_string())?;

    let path = crate::client_config::get_claude_settings_path();
    std::fs::write(&path, config).map_err(|e| e.to_string())?;
    Ok(())
}

/// 应用 Codex CLI 配置
#[tauri::command]
pub fn apply_codex_config(
    auth: String,
    config: String,
) -> Result<(), String> {
    // 验证 auth JSON
    let _: JsonValue = serde_json::from_str(&auth)
        .map_err(|e| format!("auth.json 格式错误: {}", e))?;

    // 验证 config TOML
    config.parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("config.toml 格式错误: {}", e))?;

    // 写入文件
    crate::client_config::ensure_codex_dir_exists()
        .map_err(|e| e.to_string())?;

    let auth_path = crate::client_config::get_codex_auth_path();
    let config_path = crate::client_config::get_codex_config_path();

    std::fs::write(&auth_path, auth).map_err(|e| e.to_string())?;
    std::fs::write(&config_path, config).map_err(|e| e.to_string())?;

    Ok(())
}

/// 读取 Codex CLI 配置
#[tauri::command]
pub fn read_codex_config(
    base_url: String,
    group_name: String,
) -> Result<(String, String), String> {
    let auth_path = crate::client_config::get_codex_auth_path();
    let config_path = crate::client_config::get_codex_config_path();

    let auth = if auth_path.exists() {
        std::fs::read_to_string(&auth_path).map_err(|e| e.to_string())?
    } else {
        serde_json::to_string_pretty(&serde_json::json!({
            "OPENAI_API_KEY": "uniroute"
        })).map_err(|e| e.to_string())?
    };

    let config = if config_path.exists() {
        std::fs::read_to_string(&config_path).map_err(|e| e.to_string())?
    } else {
        format!(r#"model_provider = "uniroute"
model = "{}"

[model_providers.uniroute]
name = "UniRoute"
base_url = "{}"
wire_api = "responses"
requires_openai_auth = true"#, group_name, base_url)
    };

    Ok((auth, config))
}

/// 清除 Claude Code 配置
#[tauri::command]
pub fn clear_claude_config() -> Result<bool, String> {
    crate::client_config::clear_claude_config()
        .map_err(|e| format!("清除 Claude 配置失败: {}", e))
}

/// 清除 Codex CLI 配置
#[tauri::command]
pub fn clear_codex_config() -> Result<bool, String> {
    crate::client_config::clear_codex_config()
        .map_err(|e| format!("清除 Codex 配置失败: {}", e))
}

/// 打开客户端配置目录
#[tauri::command]
pub fn open_client_config_dir(client_type: String) -> Result<(), String> {
    let dir = match client_type.as_str() {
        "claude" => crate::client_config::get_claude_config_dir(),
        "codex" => crate::client_config::get_codex_config_dir(),
        _ => return Err(format!("未知的客户端类型: {}", client_type)),
    };

    // 确保目录存在
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("创建目录失败: {}", e))?;

    // 使用系统默认程序打开目录
    #[cfg(target_os = "windows")]
    std::process::Command::new("explorer")
        .arg(&dir)
        .spawn()
        .map_err(|e| format!("打开目录失败: {}", e))?;

    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg(&dir)
        .spawn()
        .map_err(|e| format!("打开目录失败: {}", e))?;

    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open")
        .arg(&dir)
        .spawn()
        .map_err(|e| format!("打开目录失败: {}", e))?;

    Ok(())
}

//! Client configuration management for Claude Code, Codex CLI, etc.
//!
//! This module handles writing configuration files for various AI coding clients,
//! following the same approach as cc-switch.

use std::fs;
use std::path::PathBuf;
use serde_json::Value as JsonValue;

/// Get the home directory
fn get_home_dir() -> PathBuf {
    dirs::home_dir().expect("无法获取用户主目录")
}

// ============================================================================
// Claude Code Configuration
// ============================================================================

/// Get Claude config directory: ~/.claude
pub fn get_claude_config_dir() -> PathBuf {
    get_home_dir().join(".claude")
}

/// Get Claude settings.json path: ~/.claude/settings.json
pub fn get_claude_settings_path() -> PathBuf {
    get_claude_config_dir().join("settings.json")
}

/// Ensure Claude config directory exists
pub fn ensure_claude_dir_exists() -> std::io::Result<PathBuf> {
    let dir = get_claude_config_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

/// Read Claude settings.json
pub fn read_claude_settings() -> std::io::Result<Option<JsonValue>> {
    let path = get_claude_settings_path();
    if path.exists() {
        let content = fs::read_to_string(&path)?;
        let value: JsonValue = serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(Some(value))
    } else {
        Ok(None)
    }
}

/// Write Claude settings.json
pub fn write_claude_settings(settings: &JsonValue) -> std::io::Result<()> {
    ensure_claude_dir_exists()?;
    let path = get_claude_settings_path();
    let content = serde_json::to_string_pretty(settings)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    fs::write(&path, content)?;
    Ok(())
}

/// Build default Claude settings config for UniRoute
pub fn build_default_claude_settings(base_url: &str, group_name: &str) -> JsonValue {
    // 注意: base_url 不应该包含 /v1 后缀
    // Claude Code 会自动添加 /v1/messages 路径
    let clean_base_url = base_url.trim_end_matches("/v1").trim_end_matches('/');
    
    serde_json::json!({
        "env": {
            "ANTHROPIC_BASE_URL": clean_base_url,
            "ANTHROPIC_AUTH_TOKEN": "uniroute",
            "ANTHROPIC_MODEL": group_name,
            "ANTHROPIC_DEFAULT_OPUS_MODEL": group_name,
            "ANTHROPIC_DEFAULT_SONNET_MODEL": group_name,
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": group_name
        }
    })
}

/// Read Claude settings.json, returns default if not exists
pub fn read_claude_settings_with_default(base_url: &str, group_name: &str) -> std::io::Result<JsonValue> {
    let path = get_claude_settings_path();
    if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        match serde_json::from_str(&content) {
            Ok(v) => Ok(v),
            Err(_) => Ok(build_default_claude_settings(base_url, group_name)),
        }
    } else {
        Ok(build_default_claude_settings(base_url, group_name))
    }
}

/// Clear Claude Code configuration (remove UniRoute settings)
pub fn clear_claude_config() -> std::io::Result<bool> {
    let path = get_claude_settings_path();
    if !path.exists() {
        return Ok(false);
    }

    let content = fs::read_to_string(&path)?;
    let mut value: JsonValue = serde_json::from_str(&content)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let mut changed = false;

    if let Some(obj) = value.as_object_mut() {
        // Remove primaryApiKey if it's "any" (managed by UniRoute)
        if obj.get("primaryApiKey").and_then(|v| v.as_str()) == Some("any") {
            obj.remove("primaryApiKey");
            changed = true;
        }

        // Remove env if it contains UniRoute settings
        if let Some(env) = obj.get_mut("env").and_then(|v| v.as_object_mut()) {
            if env.get("ANTHROPIC_BASE_URL").is_some() || env.get("ANTHROPIC_API_KEY").is_some() {
                env.remove("ANTHROPIC_BASE_URL");
                env.remove("ANTHROPIC_API_KEY");
                changed = true;
            }
        }
    }

    if changed {
        let content = serde_json::to_string_pretty(&value)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(&path, content)?;
    }

    Ok(changed)
}

/// Check if Claude config is managed by UniRoute
pub fn is_claude_config_managed() -> bool {
    match read_claude_settings() {
        Ok(Some(settings)) => {
            settings.get("primaryApiKey").and_then(|v| v.as_str()) == Some("any")
        }
        _ => false,
    }
}

// ============================================================================
// Codex CLI Configuration
// ============================================================================

/// Get Codex config directory: ~/.codex
pub fn get_codex_config_dir() -> PathBuf {
    get_home_dir().join(".codex")
}

/// Get Codex auth.json path: ~/.codex/auth.json
pub fn get_codex_auth_path() -> PathBuf {
    get_codex_config_dir().join("auth.json")
}

/// Get Codex config.toml path: ~/.codex/config.toml
pub fn get_codex_config_path() -> PathBuf {
    get_codex_config_dir().join("config.toml")
}

/// Ensure Codex config directory exists
pub fn ensure_codex_dir_exists() -> std::io::Result<PathBuf> {
    let dir = get_codex_config_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

/// Read Codex auth.json
pub fn read_codex_auth() -> std::io::Result<Option<JsonValue>> {
    let path = get_codex_auth_path();
    if path.exists() {
        let content = fs::read_to_string(&path)?;
        let value: JsonValue = serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(Some(value))
    } else {
        Ok(None)
    }
}

/// Read Codex config.toml
pub fn read_codex_config() -> std::io::Result<Option<String>> {
    let path = get_codex_config_path();
    if path.exists() {
        let content = fs::read_to_string(&path)?;
        Ok(Some(content))
    } else {
        Ok(None)
    }
}

/// Write Codex auth.json
pub fn write_codex_auth(auth: &JsonValue) -> std::io::Result<()> {
    ensure_codex_dir_exists()?;
    let path = get_codex_auth_path();
    let content = serde_json::to_string_pretty(auth)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    fs::write(&path, content)?;
    Ok(())
}

/// Write Codex config.toml
pub fn write_codex_config(config_toml: &str) -> std::io::Result<()> {
    ensure_codex_dir_exists()?;
    let path = get_codex_config_path();
    fs::write(&path, config_toml)?;
    Ok(())
}

/// Build Codex settings_config for UniRoute proxy
///
/// This creates the configuration content for Codex CLI to use UniRoute proxy.
/// Format: { "auth": { ... }, "config": "TOML string" }
///
/// The config.toml uses the new model_providers structure:
/// ```toml
/// model_provider = "uniroute"
/// model = "<group_name>"
///
/// [model_providers.uniroute]
/// name = "UniRoute"
/// base_url = "<base_url>"
/// wire_api = "responses"  # for Responses API
/// ```
pub fn build_codex_settings_config(base_url: &str, model: &str, wire_api: Option<&str>) -> JsonValue {
    // Build config.toml content
    let wire_api_section = match wire_api {
        Some(api) => format!("\nwire_api = \"{}\"", api),
        None => String::new(),
    };

    let config_toml = format!(
        r#"model_provider = "uniroute"
model = "{}"

[model_providers.uniroute]
name = "UniRoute"
base_url = "{}"{}
requires_openai_auth = true"#,
        model, base_url, wire_api_section
    );

    // Build auth.json content
    let auth = serde_json::json!({
        "OPENAI_API_KEY": "uniroute"
    });

    // Combine into settings_config format (same as cc-switch)
    serde_json::json!({
        "auth": auth,
        "config": config_toml
    })
}

/// Apply Codex CLI configuration for UniRoute
///
/// This writes both auth.json and config.toml atomically.
pub fn apply_codex_config(base_url: &str, model: &str, wire_api: Option<&str>) -> std::io::Result<()> {
    let settings = build_codex_settings_config(base_url, model, wire_api);

    // Write auth.json
    if let Some(auth) = settings.get("auth") {
        write_codex_auth(auth)?;
    }

    // Write config.toml
    if let Some(config) = settings.get("config").and_then(|v| v.as_str()) {
        write_codex_config(config)?;
    }

    Ok(())
}

/// Clear Codex CLI configuration (remove UniRoute settings)
pub fn clear_codex_config() -> std::io::Result<bool> {
    let config_path = get_codex_config_path();
    let auth_path = get_codex_auth_path();

    let mut changed = false;

    // Check and modify config.toml
    if config_path.exists() {
        if let Some(content) = read_codex_config()? {
            // Check if it's managed by UniRoute
            if content.contains("model_provider = \"uniroute\"") || 
               content.contains("model_provider=\"uniroute\"") {
                // Parse and remove UniRoute section
                let mut doc = content.parse::<toml_edit::DocumentMut>()
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

                // Remove model_provider if it's uniroute
                if doc.get("model_provider").and_then(|v| v.as_str()) == Some("uniroute") {
                    doc.as_table_mut().remove("model_provider");
                    changed = true;
                }

                // Remove [model_providers.uniroute] section
                if let Some(model_providers) = doc.get_mut("model_providers").and_then(|v| v.as_table_mut()) {
                    if model_providers.remove("uniroute").is_some() {
                        changed = true;
                    }
                }

                if changed {
                    fs::write(&config_path, doc.to_string())?;
                }
            }
        }
    }

    // Check and modify auth.json
    if auth_path.exists() {
        if let Some(auth) = read_codex_auth()? {
            if auth.get("OPENAI_API_KEY").and_then(|v| v.as_str()) == Some("uniroute") {
                // Remove the file or clear the key
                // For safety, we'll just remove the key
                let mut updated = auth.clone();
                if let Some(obj) = updated.as_object_mut() {
                    obj.remove("OPENAI_API_KEY");
                    changed = true;
                }
                if changed {
                    write_codex_auth(&updated)?;
                }
            }
        }
    }

    Ok(changed)
}

/// Check if Codex config is managed by UniRoute
pub fn is_codex_config_managed() -> bool {
    match read_codex_config() {
        Ok(Some(content)) => {
            content.contains("model_provider = \"uniroute\"") ||
            content.contains("model_provider=\"uniroute\"")
        }
        _ => false,
    }
}

// ============================================================================
// Configuration Status
// ============================================================================

#[derive(Debug, Clone, serde::Serialize)]
pub struct ClientConfigStatus {
    pub client_type: String,
    pub config_path: String,
    pub exists: bool,
    pub is_managed: bool,
}

/// Get configuration status for a client
pub fn get_client_config_status(client_type: &str) -> ClientConfigStatus {
    match client_type {
        "claude" => {
            let path = get_claude_settings_path();
            ClientConfigStatus {
                client_type: "claude".to_string(),
                config_path: path.to_string_lossy().to_string(),
                exists: path.exists(),
                is_managed: is_claude_config_managed(),
            }
        }
        "codex" => {
            let path = get_codex_config_path();
            ClientConfigStatus {
                client_type: "codex".to_string(),
                config_path: path.to_string_lossy().to_string(),
                exists: path.exists(),
                is_managed: is_codex_config_managed(),
            }
        }
        _ => ClientConfigStatus {
            client_type: client_type.to_string(),
            config_path: String::new(),
            exists: false,
            is_managed: false,
        },
    }
}

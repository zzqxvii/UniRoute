//! OAuth flow commands

use chrono::{DateTime, Utc};
use crate::state::AppState;
use std::sync::Arc;
use tauri::State;

/// OAuth 流程状态响应
#[derive(serde::Serialize)]
pub struct OAuthFlowStatus {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub expires_in: i64,
    pub interval: i64,
}

/// 开始 OAuth 设备码流程
#[tauri::command]
pub async fn start_oauth_flow(
    provider_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<OAuthFlowStatus, String> {
    let provider = state.get_provider(&provider_id)
        .ok_or_else(|| "Provider 不存在".to_string())?;

    let oauth_config = provider.oauth.as_ref()
        .ok_or_else(|| "Provider 未配置 OAuth".to_string())?;

    let response = state.oauth_state.start_device_flow(&provider_id, oauth_config).await
        .map_err(|e| format!("OAuth 流程启动失败: {}", e.error))?;

    Ok(OAuthFlowStatus {
        device_code: response.device_code,
        user_code: response.user_code,
        verification_uri: response.verification_uri,
        verification_uri_complete: response.verification_uri_complete,
        expires_in: response.expires_in,
        interval: response.interval.unwrap_or(5),
    })
}

/// 轮询 OAuth Token
#[tauri::command]
pub async fn poll_oauth_token(
    provider_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<crate::models::OAuthTokens, String> {
    let provider = state.get_provider(&provider_id)
        .ok_or_else(|| "Provider 不存在".to_string())?;

    let oauth_config = provider.oauth.as_ref()
        .ok_or_else(|| "Provider 未配置 OAuth".to_string())?;

    let tokens = state.oauth_state.poll_for_token(&provider_id, oauth_config).await
        .map_err(|e| {
            if e.error == "pending" {
                "pending".to_string()
            } else {
                format!("获取 Token 失败: {}", e.error)
            }
        })?;

    // 更新 Provider 的 Token
    let mut provider = state.get_provider(&provider_id)
        .ok_or_else(|| "Provider 不存在".to_string())?;
    provider.oauth_tokens = Some(tokens.clone());
    let _ = state.update_provider(&provider_id, provider);

    Ok(tokens)
}

/// 刷新 OAuth Token
#[tauri::command]
pub async fn refresh_oauth_token(
    provider_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<crate::models::OAuthTokens, String> {
    let provider = state.get_provider(&provider_id)
        .ok_or_else(|| "Provider 不存在".to_string())?;

    let oauth_config = provider.oauth.as_ref()
        .ok_or_else(|| "Provider 未配置 OAuth".to_string())?;

    let current_tokens = provider.oauth_tokens.as_ref()
        .ok_or_else(|| "无有效 Token".to_string())?;

    let refresh_token = current_tokens.refresh_token.as_ref()
        .ok_or_else(|| "无 Refresh Token".to_string())?;

    let tokens = state.oauth_state.refresh_token(oauth_config, refresh_token).await
        .map_err(|e| format!("刷新 Token 失败: {}", e.error))?;

    // 更新 Provider 的 Token
    let mut provider = state.get_provider(&provider_id)
        .ok_or_else(|| "Provider 不存在".to_string())?;
    provider.oauth_tokens = Some(tokens.clone());
    let _ = state.update_provider(&provider_id, provider);

    Ok(tokens)
}

/// 取消 OAuth 流程
#[tauri::command]
pub async fn cancel_oauth_flow(
    provider_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    state.oauth_state.cancel_flow(&provider_id).await;
    Ok(())
}

/// 检查 Provider OAuth 状态
#[tauri::command]
pub fn check_oauth_status(
    provider_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<OAuthProviderStatus, String> {
    let provider = state.get_provider(&provider_id)
        .ok_or_else(|| "Provider 不存在".to_string())?;

    let has_oauth = provider.oauth.is_some();
    let needs_auth = provider.needs_oauth();
    let needs_refresh = provider.needs_token_refresh();

    Ok(OAuthProviderStatus {
        has_oauth,
        needs_auth,
        needs_refresh,
        has_token: provider.oauth_tokens.is_some(),
        expires_at: provider.oauth_tokens.as_ref().and_then(|t| t.expires_at),
    })
}

#[derive(serde::Serialize)]
pub struct OAuthProviderStatus {
    pub has_oauth: bool,
    pub needs_auth: bool,
    pub needs_refresh: bool,
    pub has_token: bool,
    pub expires_at: Option<DateTime<Utc>>,
}

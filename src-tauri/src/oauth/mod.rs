//! OAuth 认证支持
//!
//! 支持多种 OAuth 流程：
//! - 设备码流程 (Device Code Flow) - 用于 CLI/桌面应用
//! - 授权码流程 (Authorization Code Flow) - 用于有浏览器的应用

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::models::{OAuthConfig, OAuthTokens};

/// OAuth 设备码响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub expires_in: i64,
    pub interval: Option<i64>,
}

/// OAuth Token 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<i64>,
    #[serde(default)]
    pub scope: Option<String>,
}

/// OAuth 错误响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthError {
    pub error: String,
    #[serde(default)]
    pub error_description: Option<String>,
}

/// OAuth 流程状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthFlow {
    pub provider_id: String,
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub code_verifier: String,
    pub expires_at: DateTime<Utc>,
    pub interval: i64,
}

/// OAuth 状态管理器
pub struct OAuthState {
    client: Client,
    pending_flows: Arc<RwLock<HashMap<String, OAuthFlow>>>,
}

impl OAuthState {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .user_agent("OmniRoute/1.0")
                .build()
                .unwrap_or_default(),
            pending_flows: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 开始设备码流程
    pub async fn start_device_flow(
        &self,
        provider_id: &str,
        config: &OAuthConfig,
    ) -> Result<DeviceCodeResponse, OAuthError> {
        let code_verifier = generate_code_verifier();
        let code_challenge = generate_code_challenge(&code_verifier);

        // 构建请求
        let params = vec![
            ("client_id", config.client_id.as_str()),
            ("code_challenge", &code_challenge),
            ("code_challenge_method", "S256"),
        ];

        // 发送请求到授权端点
        let auth_url = config.auth_url.as_ref()
            .or(config.initiate_url.as_ref())
            .ok_or_else(|| OAuthError {
                error: "not_configured".into(),
                error_description: Some("OAuth auth_url not configured".into()),
            })?;

        let response = self.client
            .post(auth_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| OAuthError {
                error: "request_failed".into(),
                error_description: Some(e.to_string()),
            })?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(OAuthError {
                error: "auth_failed".into(),
                error_description: Some(error_text),
            });
        }

        let device_response: DeviceCodeResponse = response.json().await.map_err(|e| OAuthError {
            error: "parse_error".into(),
            error_description: Some(e.to_string()),
        })?;

        // 保存流程状态
        let flow = OAuthFlow {
            provider_id: provider_id.to_string(),
            device_code: device_response.device_code.clone(),
            user_code: device_response.user_code.clone(),
            verification_uri: device_response.verification_uri.clone(),
            code_verifier,
            expires_at: Utc::now() + chrono::Duration::seconds(device_response.expires_in),
            interval: device_response.interval.unwrap_or(5),
        };

        self.pending_flows.write().await.insert(provider_id.to_string(), flow);

        Ok(device_response)
    }

    /// 轮询等待授权完成
    pub async fn poll_for_token(
        &self,
        provider_id: &str,
        config: &OAuthConfig,
    ) -> Result<OAuthTokens, OAuthError> {
        let flow = self.pending_flows.read().await.get(provider_id).cloned()
            .ok_or_else(|| OAuthError {
                error: "no_pending_flow".into(),
                error_description: Some("No pending OAuth flow".into()),
            })?;

        // 检查是否过期
        if Utc::now() > flow.expires_at {
            self.pending_flows.write().await.remove(provider_id);
            return Err(OAuthError {
                error: "expired".into(),
                error_description: Some("Device code expired".into()),
            });
        }

        // 构建轮询请求
        let token_url = config.token_url.as_ref()
            .ok_or_else(|| OAuthError {
                error: "not_configured".into(),
                error_description: Some("Token URL not configured".into()),
            })?;

        let mut params = vec![
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", config.client_id.as_str()),
            ("device_code", &flow.device_code),
        ];

        if let Some(ref secret) = config.client_secret {
            params.push(("client_secret", secret.as_str()));
        }

        let response = self.client
            .post(token_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| OAuthError {
                error: "request_failed".into(),
                error_description: Some(e.to_string()),
            })?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        // 检查是否还在等待
        if status.as_u16() == 400 {
            // 解析错误
            if let Ok(error) = serde_json::from_str::<OAuthError>(&body) {
                if error.error == "authorization_pending" {
                    return Err(OAuthError {
                        error: "pending".into(),
                        error_description: Some("Authorization pending".into()),
                    });
                }
                return Err(error);
            }
        }

        if !status.is_success() {
            return Err(OAuthError {
                error: "token_failed".into(),
                error_description: Some(body),
            });
        }

        // 解析 token 响应
        let token_response: TokenResponse = serde_json::from_str(&body).map_err(|e| OAuthError {
            error: "parse_error".into(),
            error_description: Some(e.to_string()),
        })?;

        // 清理流程状态
        self.pending_flows.write().await.remove(provider_id);

        // 构建 OAuthTokens
        let tokens = OAuthTokens {
            access_token: token_response.access_token,
            refresh_token: token_response.refresh_token,
            expires_at: token_response.expires_in.map(|secs| {
                Utc::now() + chrono::Duration::seconds(secs)
            }),
            email: None,
        };

        Ok(tokens)
    }

    /// 刷新 Token
    pub async fn refresh_token(
        &self,
        config: &OAuthConfig,
        refresh_token: &str,
    ) -> Result<OAuthTokens, OAuthError> {
        let refresh_url = config.refresh_url.as_ref()
            .or(config.token_url.as_ref())
            .ok_or_else(|| OAuthError {
                error: "not_configured".into(),
                error_description: Some("Refresh URL not configured".into()),
            })?;

        let mut params = vec![
            ("grant_type", "refresh_token"),
            ("client_id", config.client_id.as_str()),
            ("refresh_token", refresh_token),
        ];

        if let Some(ref secret) = config.client_secret {
            params.push(("client_secret", secret.as_str()));
        }

        let response = self.client
            .post(refresh_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| OAuthError {
                error: "request_failed".into(),
                error_description: Some(e.to_string()),
            })?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(OAuthError {
                error: "refresh_failed".into(),
                error_description: Some(error_text),
            });
        }

        let token_response: TokenResponse = response.json().await.map_err(|e| OAuthError {
            error: "parse_error".into(),
            error_description: Some(e.to_string()),
        })?;

        Ok(OAuthTokens {
            access_token: token_response.access_token,
            refresh_token: token_response.refresh_token.or(Some(refresh_token.to_string())),
            expires_at: token_response.expires_in.map(|secs| {
                Utc::now() + chrono::Duration::seconds(secs)
            }),
            email: None,
        })
    }

    /// 获取当前流程状态
    pub async fn get_flow(&self, provider_id: &str) -> Option<OAuthFlow> {
        self.pending_flows.read().await.get(provider_id).cloned()
    }

    /// 取消流程
    pub async fn cancel_flow(&self, provider_id: &str) {
        self.pending_flows.write().await.remove(provider_id);
    }
}

impl Default for OAuthState {
    fn default() -> Self {
        Self::new()
    }
}

// ============ 工具函数 ============

/// 生成 code_verifier (RFC 7636)
fn generate_code_verifier() -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    let mut rng = rand::thread_rng();
    (0..128).map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char).collect()
}

/// 生成 code_challenge (S256)
fn generate_code_challenge(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

/// 生成随机 state 参数
pub fn generate_state() -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..32).map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code_challenge() {
        let verifier = "dBjftJeZ4CVP-mP92ZjgTbTVa_F5jE7fZ6L7pF7r9KM";
        let challenge = generate_code_challenge(verifier);
        // 验证生成的是有效的 base64url 字符串
        assert!(challenge.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_'));
    }
}

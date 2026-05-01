//! Provider CRUD and test commands

use crate::models::{Provider, ProviderTemplate};
use crate::state::AppState;
use std::sync::Arc;
use tauri::State;

// ============ Provider Commands ============

#[tauri::command]
pub fn get_providers(state: State<'_, Arc<AppState>>) -> Vec<Provider> {
    state.get_providers()
}

#[tauri::command]
pub fn get_builtin_templates() -> Vec<ProviderTemplate> {
    AppState::get_builtin_templates()
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn create_provider(
    name: String,
    prefix: String,
    base_url: String,
    api_key: Option<String>,
    models: Option<Vec<crate::models::ModelConfig>>,
    auth_type: Option<String>,
    oauth: Option<crate::models::OAuthConfig>,
    headers: Option<std::collections::HashMap<String, String>>,
    auth_header: Option<String>,
    auth_prefix: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<Provider, String> {
    // 检查前缀是否已存在
    if state.get_provider_by_prefix(&prefix).is_some() {
        return Err(format!("前缀 '{}' 已被使用", prefix));
    }

    let mut provider = Provider::new(name, prefix).with_base_url(base_url);
    provider.api_key = api_key;
    if let Some(models) = models {
        provider.models = models;
    }
    if let Some(at) = auth_type {
        provider.auth_type = match at.as_str() {
            "oauth" => crate::models::AuthType::OAuth,
            _ => crate::models::AuthType::ApiKey,
        };
    }
    provider.oauth = oauth;
    if let Some(h) = headers {
        provider.headers = h;
    }
    if let Some(ah) = auth_header {
        provider.auth_header = ah;
    }
    provider.auth_prefix = auth_prefix;

    state.add_provider(provider.clone()).map_err(|e| e.to_string())?;
    Ok(provider)
}

#[tauri::command]
pub fn update_provider(
    id: String,
    provider: Provider,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    // 检查前缀是否与其他 Provider 冲突
    if let Some(existing) = state.get_provider_by_prefix(&provider.prefix) {
        if existing.id != id {
            return Err(format!("前缀 '{}' 已被其他供应商使用", provider.prefix));
        }
    }

    state.update_provider(&id, provider).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_provider(id: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let provider = state.get_provider(&id)
        .ok_or_else(|| "供应商不存在".to_string())?;

    if provider.is_builtin {
        return Err("内置供应商不能删除".to_string());
    }

    state.delete_provider(&id).map_err(|e| e.to_string())
}

// ============ Provider Test Commands ============

/// Provider 测试结果
#[derive(serde::Serialize)]
pub struct ProviderTestResult {
    pub success: bool,
    pub message: String,
    pub balance: Option<BalanceInfo>,
    pub latency_ms: Option<u64>,
}

/// 余额信息
#[derive(serde::Serialize)]
pub struct BalanceInfo {
    pub available: String,
    pub currency: String,
    pub details: Option<serde_json::Value>,
}

#[tauri::command]
pub async fn test_provider(
    id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<ProviderTestResult, String> {
    let provider = state.get_provider(&id)
        .ok_or_else(|| "供应商不存在".to_string())?;

    let api_key = provider.api_key.clone()
        .ok_or_else(|| "未配置 API Key".to_string())?;

    let start = std::time::Instant::now();

    // 根据供应商类型选择测试方式
    let base_url = provider.base_url.trim_end_matches('/');

    // 先尝试余额查询
    let balance_result = query_balance(base_url, &api_key, &provider.prefix).await;

    match balance_result {
        Ok(balance) => {
            let latency = start.elapsed().as_millis() as u64;
            Ok(ProviderTestResult {
                success: true,
                message: "连接成功".to_string(),
                balance: Some(balance),
                latency_ms: Some(latency),
            })
        }
        Err(e) => {
            // 余额查询失败，尝试模型列表查询
            let models_result = test_models_endpoint(base_url, &api_key).await;
            let latency = start.elapsed().as_millis() as u64;

            match models_result {
                Ok(_) => Ok(ProviderTestResult {
                    success: true,
                    message: format!("连接成功（余额查询不支持: {}）", e),
                    balance: None,
                    latency_ms: Some(latency),
                }),
                Err(e2) => Ok(ProviderTestResult {
                    success: false,
                    message: format!("连接失败: {}", e2),
                    balance: None,
                    latency_ms: Some(latency),
                }),
            }
        }
    }
}

/// 查询余额
async fn query_balance(_base_url: &str, api_key: &str, prefix: &str) -> Result<BalanceInfo, String> {
    let client = reqwest::Client::new();

    // 根据供应商选择不同的余额查询端点
    let (url, auth_header) = match prefix {
        "ds" => {
            // DeepSeek
            ("https://api.deepseek.com/user/balance", format!("Bearer {}", api_key))
        }
        "ms" => {
            // Moonshot (Kimi) - 使用正确的余额查询端点
            ("https://api.moonshot.cn/v1/users/me/balance", format!("Bearer {}", api_key))
        }
        "zp" => {
            // 智谱AI
            ("https://open.bigmodel.cn/api/paas/v4/balance", format!("Bearer {}", api_key))
        }
        _ => {
            // 尝试通用端点
            return Err("该供应商不支持余额查询".to_string());
        }
    };

    tracing::info!("查询余额: prefix={}, url={}", prefix, url);

    let response = client
        .get(url)
        .header("Authorization", &auth_header)
        .send()
        .await
        .map_err(|e| {
            tracing::error!("余额查询请求失败: {}", e);
            format!("请求失败: {}", e)
        })?;

    let status = response.status();
    tracing::info!("余额查询响应状态: {}", status);

    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        tracing::error!("余额查询失败: status={}, body={}", status, body);
        return Err(format!("请求失败: {} - {}", status, body));
    }

    let json: serde_json::Value = response.json().await
        .map_err(|e| {
            tracing::error!("解析余额响应失败: {}", e);
            format!("解析响应失败: {}", e)
        })?;

    tracing::debug!("余额查询响应: {}", serde_json::to_string(&json).unwrap_or_default());

    // 解析不同供应商的余额格式
    match prefix {
        "ds" => {
            // DeepSeek 格式
            tracing::info!("DeepSeek 响应: {}", serde_json::to_string(&json).unwrap_or_default());

            let is_available = json.get("is_available")
                .and_then(|v| v.as_bool())
                .unwrap_or(true); // 默认为可用

            if !is_available {
                return Err("账户不可用".to_string());
            }

            // 尝试解析余额信息
            if let Some(balance_infos) = json.get("balance_infos").and_then(|v| v.as_array()) {
                if let Some(info) = balance_infos.first() {
                    let total = info.get("total_balance")
                        .and_then(|v| {
                            v.as_str().map(|s| s.to_string())
                                .or_else(|| v.as_f64().map(|f| format!("{:.6}", f)))
                        })
                        .unwrap_or_else(|| "0".to_string());
                    let currency = info.get("currency")
                        .and_then(|v| v.as_str())
                        .unwrap_or("CNY");

                    return Ok(BalanceInfo {
                        available: total,
                        currency: currency.to_string(),
                        details: Some(info.clone()),
                    });
                }
            }

            // 备选格式：直接返回余额
            if let Some(balance) = json.get("balance") {
                let total = balance.as_f64()
                    .map(|f| format!("{:.6}", f))
                    .or_else(|| balance.as_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| "0".to_string());

                return Ok(BalanceInfo {
                    available: total,
                    currency: "CNY".to_string(),
                    details: Some(json.clone()),
                });
            }

            // 无法解析，返回原始响应
            Err(format!("无法解析余额信息: {}", serde_json::to_string(&json).unwrap_or_default()))
        }
        "ms" => {
            // Moonshot (Kimi) 格式
            tracing::info!("Kimi API 响应: {}", serde_json::to_string(&json).unwrap_or_default());

            // 尝试多种格式解析
            // 格式1: { "data": { "available_balance": "xxx" } }
            if let Some(data) = json.get("data") {
                let available = data.get("available_balance")
                    .or_else(|| data.get("availableBalance"))
                    .or_else(|| data.get("balance"))
                    .and_then(|v| {
                        v.as_str().map(|s| s.to_string())
                            .or_else(|| v.as_f64().map(|f| format!("{:.2}", f)))
                            .or_else(|| v.as_i64().map(|i| i.to_string()))
                    })
                    .unwrap_or_else(|| "0".to_string());

                return Ok(BalanceInfo {
                    available,
                    currency: "CNY".to_string(),
                    details: Some(data.clone()),
                });
            }

            // 格式2: 直接返回余额字段
            let available = json.get("available_balance")
                .or_else(|| json.get("availableBalance"))
                .or_else(|| json.get("balance"))
                .and_then(|v| {
                    v.as_str().map(|s| s.to_string())
                        .or_else(|| v.as_f64().map(|f| format!("{:.2}", f)))
                        .or_else(|| v.as_i64().map(|i| i.to_string()))
                })
                .unwrap_or_else(|| "0".to_string());

            if available != "0" {
                return Ok(BalanceInfo {
                    available,
                    currency: "CNY".to_string(),
                    details: Some(json.clone()),
                });
            }

            // 无法解析，返回原始响应作为详情
            Err(format!("无法解析余额信息: {}", serde_json::to_string(&json).unwrap_or_default()))
        }
        "zp" => {
            // 智谱AI 格式
            tracing::info!("智谱AI 响应: {}", serde_json::to_string(&json).unwrap_or_default());

            let total_balance = json.get("totalBalance")
                .or_else(|| json.get("total_balance"))
                .or_else(|| json.get("balance"))
                .and_then(|v| {
                    v.as_str().map(|s| s.to_string())
                        .or_else(|| v.as_f64().map(|f| format!("{:.2}", f)))
                        .or_else(|| v.as_i64().map(|i| i.to_string()))
                })
                .unwrap_or_else(|| "0".to_string());

            if total_balance != "0" {
                return Ok(BalanceInfo {
                    available: total_balance,
                    currency: "CNY".to_string(),
                    details: Some(json.clone()),
                });
            }

            Err(format!("无法解析余额信息: {}", serde_json::to_string(&json).unwrap_or_default()))
        }
        _ => Err("未知供应商格式".to_string())
    }
}

/// 测试模型端点
async fn test_models_endpoint(base_url: &str, api_key: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
    let url = format!("{}/v1/models", base_url);

    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?;

    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("状态码: {}", response.status()))
    }
}

//! UniRoute Tauri 命令

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value as JsonValue;
use crate::models::{Group, GroupModel, GroupStrategy, ModelMapping, Provider, ProviderTemplate, RequestLog};
use crate::state::{AppSettings, AppState};
use crate::proxy::start_proxy_server;
use std::sync::Arc;
use tauri::State;

// ============ Proxy Commands ============

#[tauri::command]
pub async fn start_proxy(port: u16, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    if state.is_proxy_running() {
        return Err("代理服务器已在运行".to_string());
    }

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let state_clone = Arc::clone(&state);

    tokio::spawn(async move {
        if let Err(e) = start_proxy_server(port, state_clone, shutdown_rx).await {
            tracing::error!("代理服务器错误: {}", e);
        }
    });

    let handle = crate::state::ProxyServerHandle { port, shutdown_tx };
    *state.proxy_server.write() = Some(handle);

    tracing::info!("代理服务器已启动，端口: {}", port);
    Ok(())
}

#[tauri::command]
pub async fn stop_proxy(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut proxy = state.proxy_server.write();
    if let Some(handle) = proxy.take() {
        let _ = handle.shutdown_tx.send(());
        tracing::info!("代理服务器已停止");
    }
    Ok(())
}

#[tauri::command]
pub fn get_proxy_status(state: State<'_, Arc<AppState>>) -> ProxyStatus {
    ProxyStatus {
        is_running: state.is_proxy_running(),
        port: state.get_proxy_port(),
    }
}

#[derive(serde::Serialize)]
pub struct ProxyStatus {
    pub is_running: bool,
    pub port: Option<u16>,
}

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
    let balance_result = query_balance(&base_url, &api_key, &provider.prefix).await;

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

// ============ Group Commands ============

#[tauri::command]
pub fn get_groups(state: State<'_, Arc<AppState>>) -> Vec<Group> {
    state.get_groups()
}

#[tauri::command]
pub fn get_group(id: String, state: State<'_, Arc<AppState>>) -> Result<Group, String> {
    state.get_group(&id).ok_or_else(|| "Group 不存在".to_string())
}

#[tauri::command]
pub fn create_group(
    name: String,
    description: Option<String>,
    strategy: Option<String>,
    endpoint_type: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<Group, String> {
    if state.get_group_by_name(&name, endpoint_type.as_deref()).is_some() {
        return Err("该端点下已存在同名 Group".to_string());
    }

    let mut group = Group::new(name);
    if let Some(desc) = description {
        group.description = Some(desc);
    }
    if let Some(s) = strategy {
        group.strategy = match s.as_str() {
            "weighted" => GroupStrategy::Weighted,
            "round_robin" => GroupStrategy::RoundRobin,
            "random" => GroupStrategy::Random,
            "least_used" => GroupStrategy::LeastUsed,
            "cost_optimized" => GroupStrategy::CostOptimized,
            _ => GroupStrategy::Priority,
        };
    }
    group.endpoint_type = endpoint_type;

    state.add_group(group.clone()).map_err(|e| e.to_string())?;
    Ok(group)
}

#[tauri::command]
pub fn update_group(id: String, group: Group, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.update_group(&id, group).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_group(id: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.delete_group(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_model_to_group(
    group_id: String,
    model: String,
    priority: Option<u32>,
    weight: Option<u32>,
    state: State<'_, Arc<AppState>>,
) -> Result<Group, String> {
    let mut group = state.get_group(&group_id).ok_or_else(|| "Group 不存在".to_string())?;

    let group_model = GroupModel::new(model)
        .with_priority(priority.unwrap_or(group.models.len() as u32))
        .with_weight(weight.unwrap_or(1));

    group.add_model(group_model);
    state.update_group(&group_id, group.clone()).map_err(|e| e.to_string())?;
    Ok(group)
}

#[tauri::command]
pub fn remove_model_from_group(
    group_id: String,
    model: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Group, String> {
    let mut group = state.get_group(&group_id).ok_or_else(|| "Group 不存在".to_string())?;
    group.models.retain(|m| m.model != model);
    group.updated_at = chrono::Utc::now();
    state.update_group(&group_id, group.clone()).map_err(|e| e.to_string())?;
    Ok(group)
}

// ============ Model Mapping Commands ============

#[tauri::command]
pub fn get_model_mappings(state: State<'_, Arc<AppState>>) -> Vec<ModelMapping> {
    state.get_model_mappings()
}

#[tauri::command]
pub fn create_model_mapping(
    pattern: String,
    group_id: String,
    priority: Option<u32>,
    state: State<'_, Arc<AppState>>,
) -> Result<ModelMapping, String> {
    let mut mapping = ModelMapping::new(pattern, group_id);
    if let Some(p) = priority {
        mapping.priority = p;
    }
    state.add_model_mapping(mapping.clone()).map_err(|e| e.to_string())?;
    Ok(mapping)
}

#[tauri::command]
pub fn delete_model_mapping(id: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.delete_model_mapping(&id).map_err(|e| e.to_string())
}

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
        let state_for_proxy = Arc::clone(&state.inner());

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
    state.export_data()
}

#[tauri::command]
pub fn import_data(
    json: String,
    merge: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<ImportResultInfo, String> {
    let result = state.import_data(&json, merge)?;
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

// ============ Request Logs ============

#[tauri::command]
pub fn get_request_logs(
    limit: Option<i64>,
    offset: Option<i64>,
    state: State<'_, Arc<AppState>>,
) -> Vec<RequestLog> {
    state.get_request_logs(limit.unwrap_or(100), offset.unwrap_or(0))
}

#[tauri::command]
pub fn get_request_stats(state: State<'_, Arc<AppState>>) -> crate::storage::RequestStats {
    state.get_request_stats()
}

#[tauri::command]
pub fn clear_request_logs(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.clear_request_logs()
}

// ============ Cost Statistics ============

#[tauri::command]
pub fn get_cost_by_model(
    limit: Option<i64>,
    state: State<'_, Arc<AppState>>,
) -> Vec<crate::storage::CostByModel> {
    state.db.get_cost_by_model(limit.unwrap_or(10)).unwrap_or_default()
}

#[tauri::command]
pub fn get_cost_by_provider(
    limit: Option<i64>,
    state: State<'_, Arc<AppState>>,
) -> Vec<crate::storage::CostByProvider> {
    state.db.get_cost_by_provider(limit.unwrap_or(10)).unwrap_or_default()
}

#[tauri::command]
pub fn get_daily_cost(
    days: Option<i64>,
    state: State<'_, Arc<AppState>>,
) -> Vec<crate::storage::DailyCost> {
    state.db.get_daily_cost(days.unwrap_or(30)).unwrap_or_default()
}

#[tauri::command]
pub fn get_hourly_traffic(
    hours: Option<i64>,
    state: State<'_, Arc<AppState>>,
) -> Vec<crate::storage::HourlyTraffic> {
    state.db.get_hourly_traffic(hours.unwrap_or(24)).unwrap_or_default()
}

#[tauri::command]
pub fn get_provider_health(
    hours: Option<i64>,
    state: State<'_, Arc<AppState>>,
) -> Vec<crate::storage::ProviderHealth> {
    state.db.get_provider_health(hours.unwrap_or(24)).unwrap_or_default()
}

#[tauri::command]
pub fn get_pricing(state: State<'_, Arc<AppState>>) -> serde_json::Value {
    let pricing = state.pricing_manager.read().get_all_pricing();
    serde_json::to_value(pricing).unwrap_or(serde_json::json!({}))
}

#[tauri::command]
pub fn set_pricing(
    provider: String,
    model: String,
    input: f64,
    output: f64,
    cached: Option<f64>,
    reasoning: Option<f64>,
    cache_creation: Option<f64>,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let mut pricing = crate::pricing::PricingEntry::new(input, output);
    if let Some(c) = cached {
        pricing = pricing.with_cached(c);
    }
    if let Some(r) = reasoning {
        pricing = pricing.with_reasoning(r);
    }
    if let Some(cc) = cache_creation {
        pricing.cache_creation = cc;
    }

    state.pricing_manager.write().set_user_pricing(provider, model, pricing);
    let json = state.pricing_manager.read().export_user_pricing();
    state.db.save_setting("user_pricing", &json).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_pricing(
    provider: String,
    model: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    state.pricing_manager.write().clear_user_pricing(Some(&provider), Some(&model));
    let json = state.pricing_manager.read().export_user_pricing();
    state.db.save_setting("user_pricing", &json).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn reset_pricing(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.pricing_manager.write().clear_user_pricing(None, None);
    state.db.save_setting("user_pricing", "{}").map_err(|e| e.to_string())
}

// ============ Quota Commands ============

#[tauri::command]
pub fn get_quota_limit(state: State<'_, Arc<AppState>>) -> crate::models::QuotaLimit {
    state.get_quota_limit()
}

#[tauri::command]
pub fn update_quota_limit(
    daily_limit: Option<f64>,
    monthly_limit: Option<f64>,
    warning_threshold: Option<f64>,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let mut quota = state.get_quota_limit();
    if let Some(d) = daily_limit {
        quota.daily_limit = if d > 0.0 { Some(d) } else { None };
    }
    if let Some(m) = monthly_limit {
        quota.monthly_limit = if m > 0.0 { Some(m) } else { None };
    }
    if let Some(t) = warning_threshold {
        quota.warning_threshold = t.clamp(0.0, 1.0);
    }
    state.update_quota_limit(quota)
}

#[tauri::command]
pub fn get_quota_status(state: State<'_, Arc<AppState>>) -> crate::models::QuotaStatus {
    state.get_quota_status()
}

// ============ OAuth Commands ============

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

// ============ Provider Benchmark ============

/// 单个 provider 测速结果
#[derive(serde::Serialize, Clone)]
pub struct ProviderBenchmark {
    pub provider_id: String,
    pub provider_name: String,
    pub provider_prefix: String,
    pub model: String,
    pub latency_ms: u64,
    pub success: bool,
    pub error: Option<String>,
}

/// 批量测速请求
#[derive(serde::Deserialize)]
pub struct BenchmarkRequest {
    pub provider_ids: Vec<String>,
    pub model: Option<String>,
}

/// 批量测速结果
#[derive(serde::Serialize)]
pub struct BenchmarkResult {
    pub results: Vec<ProviderBenchmark>,
    pub total_ms: u64,
}

/// 对指定 provider 进行测速（ping）
#[tauri::command]
pub async fn benchmark_provider(
    request: BenchmarkRequest,
    state: State<'_, Arc<AppState>>,
) -> Result<BenchmarkResult, String> {
    let start = std::time::Instant::now();
    let model = request.model.unwrap_or_else(|| "gpt-3.5-turbo".to_string());

    let mut results = Vec::new();

    // 并行测速
    let mut handles = Vec::new();
    for provider_id in &request.provider_ids {
        let provider_id = provider_id.clone();
        let model = model.clone();
        let state = Arc::clone(&state.inner());

        handles.push(tokio::spawn(async move {
            ping_single_provider(&provider_id, &model, &state).await
        }));
    }

    for handle in handles {
        match handle.await {
            Ok(result) => results.push(result),
            Err(e) => {
                results.push(ProviderBenchmark {
                    provider_id: "unknown".to_string(),
                    provider_name: "Unknown".to_string(),
                    provider_prefix: "unknown".to_string(),
                    model: model.clone(),
                    latency_ms: 0,
                    success: false,
                    error: Some(format!("测速任务执行失败: {}", e)),
                });
            }
        }
    }

    // 按延迟排序
    results.sort_by(|a, b| {
        match (a.success, b.success) {
            (true, true) => a.latency_ms.cmp(&b.latency_ms),
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            (false, false) => std::cmp::Ordering::Equal,
        }
    });

    Ok(BenchmarkResult {
        results,
        total_ms: start.elapsed().as_millis() as u64,
    })
}

// ============ 诊断命令 ============

/// 路由诊断结果
#[derive(serde::Serialize)]
pub struct RouteDiagnostic {
    pub requested_model: String,
    pub group_found: Option<GroupDiagnostic>,
    pub provider_resolution: Option<ProviderDiagnostic>,
    pub warnings: Vec<String>,
}

#[derive(serde::Serialize)]
pub struct GroupDiagnostic {
    pub name: String,
    pub strategy: String,
    pub models: Vec<String>,
    pub is_active: bool,
}

#[derive(serde::Serialize)]
pub struct ProviderDiagnostic {
    pub provider_name: String,
    pub provider_prefix: String,
    pub actual_model: String,
    pub base_url: String,
    pub api_format: String,
    pub has_api_key: bool,
    pub is_active: bool,
}

/// 诊断模型路由
#[tauri::command]
pub fn diagnose_route(
    model: String,
    state: State<'_, Arc<AppState>>,
) -> RouteDiagnostic {
    let mut warnings = Vec::new();

    // 检查 Group（不限制端点类型）
    let group = state.get_group_by_name(&model, None);
    let group_info = group.as_ref().map(|g| {
        GroupDiagnostic {
            name: g.name.clone(),
            strategy: format!("{:?}", g.strategy),
            models: g.models.iter().map(|m| m.model.clone()).collect(),
            is_active: g.is_active,
        }
    });

    // 如果找到 Group，检查模型配置
    if let Some(ref g) = group {
        if !g.is_active {
            warnings.push(format!("Group '{}' 未激活", g.name));
        }
        if g.models.is_empty() {
            warnings.push(format!("Group '{}' 没有配置模型", g.name));
        }
        for m in &g.models {
            // 检查模型格式
            if !m.model.contains('/') {
                warnings.push(format!(
                    "模型 '{}' 缺少供应商前缀，格式应为 '前缀/模型名'",
                    m.model
                ));
            } else {
                let parts: Vec<&str> = m.model.splitn(2, '/').collect();
                if parts.len() == 2 {
                    let prefix = parts[0];
                    let model_name = parts[1];
                    if let Some(provider) = state.get_provider_by_prefix(prefix) {
                        if !provider.is_active {
                            warnings.push(format!(
                                "Provider '{}' (前缀 '{}') 未激活",
                                provider.name, prefix
                            ));
                        }
                        if provider.api_key.is_none() && provider.oauth_tokens.is_none() {
                            warnings.push(format!(
                                "Provider '{}' 未配置 API Key",
                                provider.name
                            ));
                        }
                    } else {
                        warnings.push(format!(
                            "找不到前缀为 '{}' 的 Provider",
                            prefix
                        ));
                    }
                }
            }
        }
    } else {
        warnings.push(format!(
            "未找到 Group '{}'，将尝试推断 Provider",
            model
        ));

        // 尝试推断 Provider
        let inferred = infer_provider_for_diagnosis(&model);
        warnings.push(format!("推断的 Provider 前缀: '{}'", inferred));

        if let Some(provider) = state.get_provider_by_prefix(&inferred) {
            warnings.push(format!(
                "将使用 Provider '{}'，模型名 '{}'",
                provider.name, model
            ));
        } else {
            warnings.push(format!(
                "推断的 Provider '{}' 不存在，请求可能失败",
                inferred
            ));
        }
    }

    RouteDiagnostic {
        requested_model: model,
        group_found: group_info,
        provider_resolution: None, // TODO: 可以扩展
        warnings,
    }
}

fn infer_provider_for_diagnosis(model: &str) -> String {
    let model_lower = model.to_lowercase();

    if model_lower.starts_with("claude") || model_lower.starts_with("anthropic") {
        "ant"
    } else if model_lower.starts_with("gemini") || model_lower.starts_with("gemma") {
        "gc"
    } else if model_lower.starts_with("gpt") || model_lower.starts_with("o1") || model_lower.starts_with("o3") {
        "oai"
    } else if model_lower.starts_with("deepseek") {
        "ds"
    } else if model_lower.starts_with("moonshot") || model_lower.starts_with("kimi") {
        "ms"
    } else if model_lower.starts_with("glm") {
        "zp"
    } else if model_lower.starts_with("qwen") {
        "qw"
    } else if model_lower.starts_with("ernie") {
        "bd"
    } else if model_lower.starts_with("llama") || model_lower.starts_with("mixtral") {
        "sf"
    } else {
        "ds"
    }.to_string()
}

async fn ping_single_provider(
    provider_id: &str,
    model: &str,
    state: &Arc<AppState>,
) -> ProviderBenchmark {
    let provider = match state.get_provider(provider_id) {
        Some(p) => p,
        None => {
            return ProviderBenchmark {
                provider_id: provider_id.to_string(),
                provider_name: "Unknown".to_string(),
                provider_prefix: "unknown".to_string(),
                model: model.to_string(),
                latency_ms: 0,
                success: false,
                error: Some("Provider 不存在".to_string()),
            };
        }
    };

    let auth_value = match provider.get_auth_value() {
        Some(v) => v,
        None => {
            return ProviderBenchmark {
                provider_id: provider_id.to_string(),
                provider_name: provider.name.clone(),
                provider_prefix: provider.prefix.clone(),
                model: model.to_string(),
                latency_ms: 0,
                success: false,
                error: Some("未配置认证信息".to_string()),
            };
        }
    };

    // 构建测试请求
    let url = format!("{}/v1/chat/completions", provider.base_url.trim_end_matches('/'));
    let test_body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "Hi"}],
        "max_tokens": 1,
    });

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().unwrap());
    headers.insert(
        reqwest::header::HeaderName::from_bytes(provider.auth_header.as_bytes()).unwrap(),
        auth_value.parse().unwrap(),
    );
    for (key, value) in &provider.headers {
        if let Ok(header_name) = reqwest::header::HeaderName::from_bytes(key.as_bytes()) {
            headers.insert(header_name, value.parse().unwrap());
        }
    }

    let start = std::time::Instant::now();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap();

    let result = client.post(&url).headers(headers).json(&test_body).send().await;

    let latency = start.elapsed().as_millis() as u64;

    match result {
        Ok(response) => {
            let success = response.status().is_success();
            ProviderBenchmark {
                provider_id: provider_id.to_string(),
                provider_name: provider.name.clone(),
                provider_prefix: provider.prefix.clone(),
                model: model.to_string(),
                latency_ms: latency,
                success,
                error: if success {
                    None
                } else {
                    Some(format!("HTTP {}", response.status()))
                },
            }
        }
        Err(e) => ProviderBenchmark {
            provider_id: provider_id.to_string(),
            provider_name: provider.name.clone(),
            provider_prefix: provider.prefix.clone(),
            model: model.to_string(),
            latency_ms: latency,
            success: false,
            error: Some(e.to_string()),
        },
    }
}

/// 端点测试结果
#[derive(Debug, Clone, Serialize)]
pub struct EndpointTestResult {
    pub provider_id: String,
    pub provider_name: String,
    pub model: String,
    pub endpoint: String,
    pub success: bool,
    pub latency_ms: u64,
    pub status_code: Option<u16>,
    pub error: Option<String>,
    pub response_preview: Option<String>,
}

/// 测试模型的指定端点是否可用
#[tauri::command]
pub async fn test_model_endpoint(
    provider_id: String,
    model: String,
    endpoint: String,
    state: State<'_, Arc<AppState>>,
) -> Result<EndpointTestResult, String> {
    let provider = state.get_provider(&provider_id)
        .ok_or_else(|| "Provider 不存在".to_string())?;

    let auth_value = provider.get_auth_value()
        .ok_or_else(|| "未配置认证信息".to_string())?;

    // 解析端点类型
    let endpoint_type = match endpoint.as_str() {
        "chat" => crate::models::EndpointCapability::Chat,
        "responses" => crate::models::EndpointCapability::Responses,
        "claude" => crate::models::EndpointCapability::Claude,
        "gemini" => crate::models::EndpointCapability::Gemini,
        "embeddings" => crate::models::EndpointCapability::Embeddings,
        "images" => crate::models::EndpointCapability::Images,
        "audio" => crate::models::EndpointCapability::Audio,
        "tts" => crate::models::EndpointCapability::TTS,
        _ => return Err(format!("未知的端点类型: {}", endpoint)),
    };

    // 根据端点类型构建测试请求
    let (url, body) = build_test_request(&provider, &model, endpoint_type);

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().unwrap());
    headers.insert(
        reqwest::header::HeaderName::from_bytes(provider.auth_header.as_bytes()).unwrap(),
        auth_value.parse().unwrap(),
    );
    for (key, value) in &provider.headers {
        if let Ok(header_name) = reqwest::header::HeaderName::from_bytes(key.as_bytes()) {
            headers.insert(header_name, value.parse().unwrap());
        }
    }

    let start = std::time::Instant::now();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let result = client.post(&url).headers(headers).json(&body).send().await;
    let latency = start.elapsed().as_millis() as u64;

    match result {
        Ok(response) => {
            let status = response.status();
            let status_code = status.as_u16();
            let success = status.is_success();

            // 读取响应内容（截取前 500 字符）
            let response_text = response.text().await.unwrap_or_default();
            let response_preview = if response_text.len() > 500 {
                Some(format!("{}... (truncated)", &response_text[..500]))
            } else if response_text.is_empty() {
                None
            } else {
                Some(response_text)
            };

            Ok(EndpointTestResult {
                provider_id: provider_id.clone(),
                provider_name: provider.name.clone(),
                model: model.clone(),
                endpoint: endpoint.clone(),
                success,
                latency_ms: latency,
                status_code: Some(status_code),
                error: if success { None } else { Some(format!("HTTP {}", status_code)) },
                response_preview,
            })
        }
        Err(e) => Ok(EndpointTestResult {
            provider_id: provider_id.clone(),
            provider_name: provider.name.clone(),
            model: model.clone(),
            endpoint: endpoint.clone(),
            success: false,
            latency_ms: latency,
            status_code: None,
            error: Some(e.to_string()),
            response_preview: None,
        }),
    }
}

/// 根据端点类型构建测试请求
/// 智能拼接 API URL，处理 base_url 已包含 /v1 的情况
fn build_api_url(base_url: &str, path: &str) -> String {
    let base = base_url.trim_end_matches('/');
    // 如果 path 以 /v1 开头且 base 已包含 /v1，则去掉重复
    if base.ends_with("/v1") && path.starts_with("/v1/") {
        format!("{}{}", base, &path[3..]) // 去掉 path 的 /v1
    } else {
        format!("{}{}", base, path)
    }
}

fn build_test_request(
    provider: &crate::models::Provider,
    model: &str,
    endpoint_type: crate::models::EndpointCapability,
) -> (String, serde_json::Value) {
    match endpoint_type {
        crate::models::EndpointCapability::Chat => {
            let url = build_api_url(&provider.base_url, "/v1/chat/completions");
            let body = serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": "Hi"}],
                "max_tokens": 5,
            });
            (url, body)
        }
        crate::models::EndpointCapability::Responses => {
            let url = build_api_url(&provider.base_url, "/v1/responses");
            let body = serde_json::json!({
                "model": model,
                "input": "Hi",
                "max_output_tokens": 5,
            });
            (url, body)
        }
        crate::models::EndpointCapability::Claude => {
            let url = build_api_url(&provider.base_url, "/v1/messages");
            let body = serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": "Hi"}],
                "max_tokens": 5,
            });
            (url, body)
        }
        crate::models::EndpointCapability::Gemini => {
            // Gemini API 路径格式: /v1beta/models/{model}:generateContent
            let url = build_api_url(&provider.base_url, &format!("/v1beta/models/{}:generateContent", model));
            let body = serde_json::json!({
                "contents": [{"parts": [{"text": "Hi"}]}],
                "generationConfig": {"maxOutputTokens": 5},
            });
            (url, body)
        }
        crate::models::EndpointCapability::Embeddings => {
            let url = build_api_url(&provider.base_url, "/v1/embeddings");
            let body = serde_json::json!({
                "model": model,
                "input": "test",
            });
            (url, body)
        }
        crate::models::EndpointCapability::Images => {
            let url = build_api_url(&provider.base_url, "/v1/images/generations");
            let body = serde_json::json!({
                "model": model,
                "prompt": "a white cat",
                "n": 1,
                "size": "256x256",
            });
            (url, body)
        }
        crate::models::EndpointCapability::Videos => {
            let url = build_api_url(&provider.base_url, "/v1/videos/generations");
            let body = serde_json::json!({
                "model": model,
                "prompt": "a cat walking",
            });
            (url, body)
        }
        crate::models::EndpointCapability::Music => {
            let url = build_api_url(&provider.base_url, "/v1/music/generations");
            let body = serde_json::json!({
                "model": model,
                "prompt": "a happy tune",
            });
            (url, body)
        }
        crate::models::EndpointCapability::Audio => {
            // Audio 需要文件，这里只测试端点是否存在
            let url = build_api_url(&provider.base_url, "/v1/audio/transcriptions");
            let body = serde_json::json!({
                "model": model,
            });
            (url, body)
        }
        crate::models::EndpointCapability::TTS => {
            let url = build_api_url(&provider.base_url, "/v1/audio/speech");
            let body = serde_json::json!({
                "model": model,
                "input": "test",
                "voice": "alloy",
            });
            (url, body)
        }
        crate::models::EndpointCapability::Moderation => {
            let url = build_api_url(&provider.base_url, "/v1/moderations");
            let body = serde_json::json!({
                "model": model,
                "input": "test",
            });
            (url, body)
        }
        crate::models::EndpointCapability::Rerank => {
            let url = build_api_url(&provider.base_url, "/v1/rerank");
            let body = serde_json::json!({
                "model": model,
                "query": "test",
                "documents": ["doc1", "doc2"],
            });
            (url, body)
        }
    }
}

/// 从 Provider 获取可用模型列表
#[derive(Debug, Clone, Serialize)]
pub struct RemoteModel {
    pub id: String,
    pub name: String,
    pub owned_by: Option<String>,
}

#[tauri::command]
pub async fn fetch_provider_models(
    provider_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<RemoteModel>, String> {
    let provider = state.get_provider(&provider_id)
        .ok_or_else(|| "Provider 不存在".to_string())?;

    let auth_value = provider.get_auth_value()
        .ok_or_else(|| "未配置认证信息".to_string())?;

    let url = build_api_url(&provider.base_url, "/v1/models");

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().unwrap());
    headers.insert(
        reqwest::header::HeaderName::from_bytes(provider.auth_header.as_bytes()).unwrap(),
        auth_value.parse().unwrap(),
    );
    // OpenRouter 需要这些 header
    headers.insert("HTTP-Referer", "https://uniroute.app".parse().unwrap());
    headers.insert("X-Title", "UniRoute".parse().unwrap());
    for (key, value) in &provider.headers {
        if let Ok(header_name) = reqwest::header::HeaderName::from_bytes(key.as_bytes()) {
            headers.insert(header_name, value.parse().unwrap());
        }
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let response = client.get(&url).headers(headers).send().await
        .map_err(|e| format!("请求失败: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("获取模型列表失败: HTTP {}", response.status()));
    }

    let json: serde_json::Value = response.json().await
        .map_err(|e| format!("解析响应失败: {}", e))?;

    // 解析 OpenAI 格式的模型列表
    let models = json.get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter().filter_map(|m| {
                let id = m.get("id")?.as_str()?.to_string();
                let name = m.get("id").and_then(|v| v.as_str()).unwrap_or(&id).to_string();
                let owned_by = m.get("owned_by").and_then(|v| v.as_str()).map(|s| s.to_string());
                Some(RemoteModel { id, name, owned_by })
            }).collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(models)
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

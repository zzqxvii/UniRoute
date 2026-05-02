//! Pricing, cost statistics, and request log commands

use crate::models::RequestLog;
use crate::state::{AppState, AppStateContainer};
use std::sync::Arc;
use tauri::State;

fn get_state(container: &AppStateContainer) -> Option<Arc<AppState>> {
    container.try_get()
}

// ============ Pricing Commands ============

#[tauri::command]
pub fn get_pricing(state: State<'_, AppStateContainer>) -> serde_json::Value {
    let Some(state) = get_state(&state) else {
        return serde_json::json!({});
    };
    let pricing = state.pricing_manager.read().get_all_pricing();
    serde_json::to_value(pricing).unwrap_or(serde_json::json!({}))
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn set_pricing(
    provider: String,
    model: String,
    input: f64,
    output: f64,
    cached: Option<f64>,
    reasoning: Option<f64>,
    cache_creation: Option<f64>,
    state: State<'_, AppStateContainer>,
) -> Result<(), String> {
    let state = get_state(&state).ok_or("应用正在初始化")?;
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
    state: State<'_, AppStateContainer>,
) -> Result<(), String> {
    let state = get_state(&state).ok_or("应用正在初始化")?;
    state.pricing_manager.write().clear_user_pricing(Some(&provider), Some(&model));
    let json = state.pricing_manager.read().export_user_pricing();
    state.db.save_setting("user_pricing", &json).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn reset_pricing(state: State<'_, AppStateContainer>) -> Result<(), String> {
    let state = get_state(&state).ok_or("应用正在初始化")?;
    state.pricing_manager.write().clear_user_pricing(None, None);
    state.db.save_setting("user_pricing", "{}").map_err(|e| e.to_string())
}

// ============ Request Logs ============

#[tauri::command]
pub fn get_request_logs(
    limit: Option<i64>,
    offset: Option<i64>,
    state: State<'_, AppStateContainer>,
) -> Vec<RequestLog> {
    get_state(&state)
        .map(|s| s.get_request_logs(limit.unwrap_or(100), offset.unwrap_or(0)))
        .unwrap_or_default()
}

#[tauri::command]
pub fn get_request_stats(state: State<'_, AppStateContainer>) -> crate::storage::RequestStats {
    get_state(&state)
        .map(|s| s.get_request_stats())
        .unwrap_or(crate::storage::RequestStats {
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            total_tokens: 0,
            total_cost: 0.0,
            avg_latency_ms: 0.0,
        })
}

#[tauri::command]
pub fn clear_request_logs(state: State<'_, AppStateContainer>) -> Result<(), String> {
    let state = get_state(&state).ok_or("应用正在初始化")?;
    state.clear_request_logs().map_err(|e| e.to_string())
}

// ============ Cost Statistics ============

#[tauri::command]
pub fn get_cost_by_model(
    limit: Option<i64>,
    state: State<'_, AppStateContainer>,
) -> Vec<crate::storage::CostByModel> {
    get_state(&state)
        .and_then(|s| s.db.get_cost_by_model(limit.unwrap_or(10)).ok())
        .unwrap_or_default()
}

#[tauri::command]
pub fn get_cost_by_provider(
    limit: Option<i64>,
    state: State<'_, AppStateContainer>,
) -> Vec<crate::storage::CostByProvider> {
    get_state(&state)
        .and_then(|s| s.db.get_cost_by_provider(limit.unwrap_or(10)).ok())
        .unwrap_or_default()
}

#[tauri::command]
pub fn get_daily_cost(
    days: Option<i64>,
    state: State<'_, AppStateContainer>,
) -> Vec<crate::storage::DailyCost> {
    get_state(&state)
        .and_then(|s| s.db.get_daily_cost(days.unwrap_or(30)).ok())
        .unwrap_or_default()
}

#[tauri::command]
pub fn get_hourly_traffic(
    hours: Option<i64>,
    state: State<'_, AppStateContainer>,
) -> Vec<crate::storage::HourlyTraffic> {
    get_state(&state)
        .and_then(|s| s.db.get_hourly_traffic(hours.unwrap_or(24)).ok())
        .unwrap_or_default()
}

#[tauri::command]
pub fn get_provider_health(
    hours: Option<i64>,
    state: State<'_, AppStateContainer>,
) -> Vec<crate::storage::ProviderHealth> {
    get_state(&state)
        .and_then(|s| s.db.get_provider_health(hours.unwrap_or(24)).ok())
        .unwrap_or_default()
}

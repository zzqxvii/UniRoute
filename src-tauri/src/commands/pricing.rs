//! Pricing, cost statistics, and request log commands

use crate::models::RequestLog;
use crate::state::AppState;
use std::sync::Arc;
use tauri::State;

// ============ Pricing Commands ============

#[tauri::command]
pub fn get_pricing(state: State<'_, Arc<AppState>>) -> serde_json::Value {
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
    state.clear_request_logs().map_err(|e| e.to_string())
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

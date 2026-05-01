//! Quota management commands

use crate::state::AppState;
use std::sync::Arc;
use tauri::State;

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
    state.update_quota_limit(quota).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_quota_status(state: State<'_, Arc<AppState>>) -> crate::models::QuotaStatus {
    state.get_quota_status()
}

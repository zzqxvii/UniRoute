//! Quota management commands

use crate::state::{AppState, AppStateContainer};
use std::sync::Arc;
use tauri::State;

fn get_state(container: &AppStateContainer) -> Option<Arc<AppState>> {
    container.try_get()
}

#[tauri::command]
pub fn get_quota_limit(state: State<'_, AppStateContainer>) -> crate::models::QuotaLimit {
    get_state(&state)
        .map(|s| s.get_quota_limit())
        .unwrap_or_default()
}

#[tauri::command]
pub fn update_quota_limit(
    daily_limit: Option<f64>,
    monthly_limit: Option<f64>,
    warning_threshold: Option<f64>,
    state: State<'_, AppStateContainer>,
) -> Result<(), String> {
    let state = get_state(&state).ok_or("应用正在初始化")?;
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
pub fn get_quota_status(state: State<'_, AppStateContainer>) -> crate::models::QuotaStatus {
    get_state(&state)
        .map(|s| s.get_quota_status())
        .unwrap_or_default()
}

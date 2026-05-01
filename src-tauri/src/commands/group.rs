//! Group and model mapping commands

use crate::models::{Group, GroupModel, GroupStrategy, ModelMapping};
use crate::state::AppState;
use std::sync::Arc;
use tauri::State;

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

//! UniRoute 应用状态
//!
//! 简化架构：Provider 是核心实体

use crate::models::{Group, ModelMapping, Provider, ProviderTemplate, QuotaLimit, QuotaStatus, RequestLog};
use crate::oauth::OAuthState;
use crate::pricing::PricingManager;
use crate::router::{RateLimiter, CircuitBreaker};
use crate::storage::Database;
use anyhow::{Context, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// 应用设置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub proxy_port: u16,
    pub auto_start_proxy: bool,
    pub log_level: String,
    /// 配额限制
    #[serde(default)]
    pub quota: QuotaLimit,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            proxy_port: 8080,
            auto_start_proxy: false,
            log_level: "info".to_string(),
            quota: QuotaLimit::default(),
        }
    }
}

/// Group 策略状态
pub struct GroupStrategyState {
    /// 轮询索引（Group ID -> 当前索引）
    round_robin_index: RwLock<HashMap<String, u64>>,
    /// 模型使用计数（"group_id:model" -> count）
    model_usage: RwLock<HashMap<String, u64>>,
}

impl GroupStrategyState {
    pub fn new() -> Self {
        Self {
            round_robin_index: RwLock::new(HashMap::new()),
            model_usage: RwLock::new(HashMap::new()),
        }
    }

    /// 获取并递增轮询索引
    pub fn next_round_robin_index(&self, group_id: &str, model_count: usize) -> usize {
        let mut map = self.round_robin_index.write();
        let index = map.entry(group_id.to_string()).or_insert(0);
        let current = *index as usize;
        *index = (*index + 1) % model_count as u64;
        current % model_count
    }

    /// 记录模型使用
    pub fn record_model_usage(&self, group_id: &str, model: &str) {
        let key = format!("{}:{}", group_id, model);
        let mut map = self.model_usage.write();
        *map.entry(key).or_insert(0) += 1;
    }

    /// 获取模型使用次数
    pub fn get_model_usage(&self, group_id: &str, model: &str) -> u64 {
        let key = format!("{}:{}", group_id, model);
        let map = self.model_usage.read();
        *map.get(&key).unwrap_or(&0)
    }
}

impl Default for GroupStrategyState {
    fn default() -> Self {
        Self::new()
    }
}

/// 应用状态
pub struct AppState {
    /// Provider 列表
    pub providers: RwLock<Vec<Provider>>,
    /// Group 路由组
    pub groups: RwLock<Vec<Group>>,
    /// 模型映射表
    pub model_mappings: RwLock<Vec<ModelMapping>>,
    /// 数据库
    pub db: Arc<Database>,
    /// 代理服务器
    pub proxy_server: RwLock<Option<ProxyServerHandle>>,
    /// 应用设置
    pub settings: RwLock<AppSettings>,
    /// OAuth 状态
    pub oauth_state: OAuthState,
    /// 定价管理器
    pub pricing_manager: Arc<RwLock<PricingManager>>,
    /// Group 策略状态
    pub group_strategy_state: Arc<GroupStrategyState>,
    /// 共享 HTTP 客户端（复用连接池）
    pub http_client: reqwest::Client,
    /// 共享速率限制器（跨请求保持状态）
    pub rate_limiter: Arc<RateLimiter>,
    /// 共享熔断器（跨请求保持状态）
    pub circuit_breaker: Arc<CircuitBreaker>,
}

/// 代理服务器句柄
pub struct ProxyServerHandle {
    pub port: u16,
    pub shutdown_tx: tokio::sync::oneshot::Sender<()>,
}

impl AppState {
    pub fn new() -> Result<Self> {
        let db = Database::new(Database::default_path())
            .context("初始化数据库失败")?;

        // 从数据库加载
        let providers = db.load_providers().context("加载供应商失败")?;
        let groups = db.load_groups().context("加载组合失败")?;
        let model_mappings = db.load_model_mappings().context("加载模型映射失败")?;
        let settings = db.load_setting("settings")
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        // 加载用户定价
        let mut pricing_manager = PricingManager::new();
        if let Ok(Some(pricing_json)) = db.load_setting("user_pricing") {
            let _ = pricing_manager.load_user_pricing(&pricing_json);
        }

        Ok(Self {
            providers: RwLock::new(providers),
            groups: RwLock::new(groups),
            model_mappings: RwLock::new(model_mappings),
            db: Arc::new(db),
            proxy_server: RwLock::new(None),
            settings: RwLock::new(settings),
            oauth_state: OAuthState::new(),
            pricing_manager: Arc::new(RwLock::new(pricing_manager)),
            group_strategy_state: Arc::new(GroupStrategyState::new()),
            http_client: reqwest::Client::new(),
            rate_limiter: Arc::new(RateLimiter::new()),
            circuit_breaker: Arc::new(CircuitBreaker::new()),
        })
    }

    // ============ Provider 管理 ============

    pub fn get_providers(&self) -> Vec<Provider> {
        self.providers.read().clone()
    }

    pub fn get_provider(&self, id: &str) -> Option<Provider> {
        self.providers.read().iter().find(|p| p.id == id).cloned()
    }

    pub fn get_provider_by_prefix(&self, prefix: &str) -> Option<Provider> {
        self.providers.read().iter().find(|p| p.prefix == prefix && p.is_active).cloned()
    }

    pub fn get_builtin_templates() -> Vec<ProviderTemplate> {
        Provider::builtin_templates()
    }

    pub fn add_provider(&self, provider: Provider) -> Result<()> {
        self.db.save_provider(&provider).context("保存供应商失败")?;
        self.providers.write().push(provider);
        Ok(())
    }

    pub fn update_provider(&self, id: &str, updated: Provider) -> Result<()> {
        self.db.save_provider(&updated).context("更新供应商失败")?;
        let mut providers = self.providers.write();
        if let Some(idx) = providers.iter().position(|p| p.id == id) {
            providers[idx] = updated;
        }
        Ok(())
    }

    pub fn delete_provider(&self, id: &str) -> Result<()> {
        self.db.delete_provider(id).context("删除供应商失败")?;
        self.providers.write().retain(|p| p.id != id);
        Ok(())
    }

    // ============ Group 管理 ============

    pub fn get_groups(&self) -> Vec<Group> {
        self.groups.read().clone()
    }

    pub fn get_group(&self, id: &str) -> Option<Group> {
        self.groups.read().iter().find(|g| g.id == id).cloned()
    }

    pub fn get_group_by_name(&self, name: &str, endpoint_type: Option<&str>) -> Option<Group> {
        let endpoint = endpoint_type.unwrap_or("chat");
        self.groups.read()
            .iter()
            .find(|g| {
                let g_endpoint = g.endpoint_type.as_deref().unwrap_or("chat");
                g.name == name && g_endpoint == endpoint && g.is_active
            })
            .cloned()
    }

    pub fn add_group(&self, group: Group) -> anyhow::Result<()> {
        self.db.save_group(&group).context("保存组合失败")?;
        self.groups.write().push(group);
        Ok(())
    }

    pub fn update_group(&self, id: &str, updated: Group) -> anyhow::Result<()> {
        self.db.save_group(&updated)?;
        let mut groups = self.groups.write();
        if let Some(idx) = groups.iter().position(|g| g.id == id) {
            groups[idx] = updated;
        }
        Ok(())
    }

    pub fn delete_group(&self, id: &str) -> anyhow::Result<()> {
        self.db.delete_group(id).context("删除组合失败")?;
        self.groups.write().retain(|g| g.id != id);
        Ok(())
    }

    // ============ Model Mapping 管理 ============

    pub fn get_model_mappings(&self) -> Vec<ModelMapping> {
        self.model_mappings.read().clone()
    }

    pub fn add_model_mapping(&self, mapping: ModelMapping) -> anyhow::Result<()> {
        self.db.save_model_mapping(&mapping).context("保存模型映射失败")?;
        self.model_mappings.write().push(mapping);
        Ok(())
    }

    pub fn delete_model_mapping(&self, id: &str) -> anyhow::Result<()> {
        self.db.delete_model_mapping(id).context("删除模型映射失败")?;
        self.model_mappings.write().retain(|m| m.id != id);
        Ok(())
    }

    // ============ 设置管理 ============

    pub fn get_settings(&self) -> AppSettings {
        self.settings.read().clone()
    }

    pub fn update_settings(&self, settings: AppSettings) {
        let _ = self.db.save_setting("settings", &serde_json::to_string(&settings).unwrap());
        *self.settings.write() = settings;
    }

    // ============ 代理状态 ============

    pub fn is_proxy_running(&self) -> bool {
        self.proxy_server.read().is_some()
    }

    pub fn get_proxy_port(&self) -> Option<u16> {
        self.proxy_server.read().as_ref().map(|h| h.port)
    }

    // ============ 数据导入导出 ============

    pub fn export_data(&self) -> anyhow::Result<String> {
        self.db.export_json()
    }

    pub fn import_data(&self, json: &str, merge: bool) -> anyhow::Result<crate::storage::ImportResult> {
        let result = self.db.import_json(json, merge)?;
        // 刷新内存
        let providers = self.db.load_providers()?;
        let groups = self.db.load_groups()?;
        let mappings = self.db.load_model_mappings()?;
        *self.providers.write() = providers;
        *self.groups.write() = groups;
        *self.model_mappings.write() = mappings;
        Ok(result)
    }

    pub fn get_db_path(&self) -> std::path::PathBuf {
        self.db.path().clone()
    }

    // ============ 请求日志 ============

    pub fn save_request_log(&self, log: &RequestLog) {
        if let Err(e) = self.db.save_request_log(log) {
            tracing::error!("Failed to save request log: {}", e);
        }
    }

    pub fn get_request_logs(&self, limit: i64, offset: i64) -> Vec<RequestLog> {
        self.db.load_request_logs(limit, offset).unwrap_or_default()
    }

    pub fn get_request_stats(&self) -> crate::storage::RequestStats {
        self.db.get_stats().unwrap_or(crate::storage::RequestStats {
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            total_tokens: 0,
            total_cost: 0.0,
            avg_latency_ms: 0.0,
        })
    }

    pub fn clear_request_logs(&self) -> anyhow::Result<()> {
        self.db.clear_request_logs()
    }

    // ============ 定价管理 ============

    /// 获取所有定价
    pub fn get_all_pricing(&self) -> crate::pricing::PricingByProvider {
        self.pricing_manager.read().get_all_pricing()
    }

    /// 获取指定模型的定价
    pub fn get_model_pricing(&self, provider: &str, model: &str) -> Option<crate::pricing::PricingEntry> {
        self.pricing_manager.read().get_pricing(provider, model)
    }

    /// 设置用户定价
    pub fn set_pricing(&self, provider: String, model: String, pricing: crate::pricing::PricingEntry) -> anyhow::Result<()> {
        self.pricing_manager.write().set_user_pricing(provider, model, pricing);
        self.save_user_pricing()
    }

    /// 删除用户定价
    pub fn delete_pricing(&self, provider: Option<&str>, model: Option<&str>) -> anyhow::Result<()> {
        self.pricing_manager.write().clear_user_pricing(provider, model);
        self.save_user_pricing()
    }

    /// 重置用户定价
    pub fn reset_pricing(&self) -> anyhow::Result<()> {
        self.pricing_manager.write().clear_user_pricing(None, None);
        self.save_user_pricing()
    }

    // ============ 配额管理 ============

    /// 获取配额限制配置
    pub fn get_quota_limit(&self) -> QuotaLimit {
        self.settings.read().quota.clone()
    }

    /// 更新配额限制配置
    pub fn update_quota_limit(&self, quota: QuotaLimit) -> anyhow::Result<()> {
        let mut settings = self.settings.write();
        settings.quota = quota;
        self.db.save_setting("settings", &serde_json::to_string(&*settings)?)?;
        Ok(())
    }

    /// 获取配额使用状态
    pub fn get_quota_status(&self) -> QuotaStatus {
        let daily_used = self.db.get_today_cost().unwrap_or(0.0);
        let monthly_used = self.db.get_month_cost().unwrap_or(0.0);
        let limit = self.settings.read().quota.clone();
        QuotaStatus::compute(daily_used, monthly_used, &limit)
    }

    /// 检查是否允许请求（配额检查）
    pub fn check_quota(&self) -> Result<(), String> {
        let status = self.get_quota_status();
        status.allow_request()
    }

    /// 保存用户定价到数据库
    fn save_user_pricing(&self) -> anyhow::Result<()> {
        let json = self.pricing_manager.read().export_user_pricing();
        self.db.save_setting("user_pricing", &json)?;
        Ok(())
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new().expect("初始化应用状态失败")
    }
}

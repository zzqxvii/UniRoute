//! UniRoute 路由器
//!
//! 简化架构：请求模型名 → Group → 模型列表 → 选择模型 → 通过前缀找 Provider → 发送请求

mod circuit_breaker;
mod conversion;
mod rate_limiter;
pub mod retry;
pub mod strategies;

pub use circuit_breaker::*;
pub use conversion::*;
pub use rate_limiter::*;
pub use retry::RetryConfig;

use crate::models::{ApiFormat, ChatRequest, EmbeddingRequest, ResponsesRequest, Group, GroupModel, ModelMapping, Provider, EndpointCapability};
use crate::state::AppState;
use std::sync::Arc;

/// 路由信息
#[derive(Debug, Clone, Default)]
pub struct RouteInfo {
    /// Provider 名称
    pub provider_name: Option<String>,
    /// Provider 前缀
    pub provider_prefix: Option<String>,
    /// 实际使用的模型名
    pub actual_model: Option<String>,
    /// 请求的模型名
    pub requested_model: String,
    /// 实际请求的 URL
    pub actual_url: Option<String>,
    /// 协议转换类型: "direct" 直发, "openai->claude" 等
    pub protocol_transform: Option<String>,
    /// 端点类型（如果指定）
    pub endpoint_type: Option<String>,
}

/// 路由结果 - 返回原始 HTTP 响应
pub struct RouteResult {
    /// 原始 HTTP 响应
    pub response: Option<reqwest::Response>,
    /// 错误信息
    pub error: Option<String>,
    /// 路由信息
    pub info: RouteInfo,
    /// 实际发送的请求体（模型名已替换）
    pub actual_request_body: Option<serde_json::Value>,
}

impl RouteResult {
    /// 创建错误结果
    fn error_result(
        provider: &Provider,
        actual_model: &str,
        requested_model: String,
        endpoint_type: Option<&str>,
        error: String,
    ) -> Self {
        RouteResult {
            response: None,
            error: Some(error),
            info: RouteInfo {
                provider_name: Some(provider.name.clone()),
                provider_prefix: Some(provider.prefix.clone()),
                actual_model: Some(actual_model.to_string()),
                requested_model,
                actual_url: None,
                protocol_transform: None,
                endpoint_type: endpoint_type.map(|s| s.to_string()),
            },
            actual_request_body: None,
        }
    }
}

/// 智能拼接 API URL，处理 base_url 已包含 /v1 的情况
pub fn build_api_url(base_url: &str, path: &str) -> String {
    let base = base_url.trim_end_matches('/');
    // 如果 path 以 /v1 开头且 base 已包含 /v1，则去掉重复
    if base.ends_with("/v1") && path.starts_with("/v1/") {
        format!("{}{}", base, &path[3..]) // 去掉 path 的 /v1
    } else {
        format!("{}{}", base, path)
    }
}

/// 路由器
pub struct Router {
    state: Arc<AppState>,
    rate_limiter: Arc<RateLimiter>,
    circuit_breaker: Arc<CircuitBreaker>,
}

impl Router {
    pub fn new(state: Arc<AppState>) -> Self {
        let rate_limiter = Arc::clone(&state.rate_limiter);
        let circuit_breaker = Arc::clone(&state.circuit_breaker);
        Self {
            state,
            rate_limiter,
            circuit_breaker,
        }
    }

    /// 获取共享 HTTP 客户端
    fn http_client(&self) -> reqwest::Client {
        self.state.http_client.clone()
    }

    /// 路由聊天请求
    pub async fn route_chat(&self, request: ChatRequest) -> RouteResult {
        let requested_model = request.model.clone();
        tracing::info!("收到请求: model='{}'", requested_model);

        // 1. 查找 Group (chat 端点)
        let group = self.find_group(&requested_model, Some("chat"));

        if let Some(group) = group {
            tracing::info!(
                "找到 Group: name='{}', models_count={}, strategy={:?}, models={:?}",
                group.name,
                group.models.len(),
                group.strategy,
                group.models.iter().map(|m| &m.model).collect::<Vec<_>>()
            );
            if group.models.is_empty() {
                tracing::warn!("Group '{}' 没有配置任何模型", group.name);
                return RouteResult {
                    response: None,
                    error: Some(format!("Group '{}' 没有配置任何模型", group.name)),
                    info: RouteInfo {
                        provider_name: None,
                        provider_prefix: None,
                        actual_model: None,
                        requested_model,
                        actual_url: None,
                        protocol_transform: None,
                    endpoint_type: None,
                    },
                    actual_request_body: None,
                };
            }
            self.execute_with_group(&group, request, requested_model).await
        } else {
            // 检查是否有任何可用的供应商
            let providers = self.state.get_providers();
            let active_providers: Vec<_> = providers.iter().filter(|p| p.is_active).collect();

            if active_providers.is_empty() {
                tracing::error!("没有可用的供应商");
                return RouteResult {
                    response: None,
                    error: Some("没有配置任何可用的供应商".to_string()),
                    info: RouteInfo {
                        provider_name: None,
                        provider_prefix: None,
                        actual_model: None,
                        requested_model,
                        actual_url: None,
                        protocol_transform: None,
                    endpoint_type: None,
                    },
                    actual_request_body: None,
                };
            }

            tracing::warn!(
                "未找到 Group '{}', 尝试推断供应商。可用供应商: {:?}",
                requested_model,
                active_providers.iter().map(|p| format!("{}({})", p.name, p.prefix)).collect::<Vec<_>>()
            );
            self.execute_single_model(&requested_model, request, requested_model.clone()).await
        }
    }

    /// 路由 embedding 请求
    pub async fn route_embedding(&self, request: EmbeddingRequest) -> RouteResult {
        let requested_model = request.model.clone();
        tracing::info!("收到 Embedding 请求: model='{}'", requested_model);

        let group = self.find_group(&requested_model, Some("embeddings"));

        if let Some(group) = group {
            if group.models.is_empty() {
                return RouteResult {
                    response: None,
                    error: Some(format!("Group '{}' 没有配置任何模型", group.name)),
                    info: RouteInfo {
                        provider_name: None,
                        provider_prefix: None,
                        actual_model: None,
                        requested_model,
                        actual_url: None,
                        protocol_transform: None,
                    endpoint_type: None,
                    },
                    actual_request_body: None,
                };
            }
            self.execute_embedding_with_group(&group, &request, requested_model).await
        } else {
            let providers = self.state.get_providers();
            let active_providers: Vec<_> = providers.iter().filter(|p| p.is_active).collect();
            if active_providers.is_empty() {
                return RouteResult {
                    response: None,
                    error: Some("没有配置任何可用的供应商".to_string()),
                    info: RouteInfo {
                        provider_name: None,
                        provider_prefix: None,
                        actual_model: None,
                        requested_model,
                        actual_url: None,
                        protocol_transform: None,
                    endpoint_type: None,
                    },
                    actual_request_body: None,
                };
            }
            self.execute_embedding_single_model(&requested_model, &request, requested_model.clone()).await
        }
    }

    /// 路由 Responses API 请求
    /// 路由 Responses API 请求（使用原始 JSON）
    pub async fn route_responses_raw(&self, raw_body: &serde_json::Value) -> RouteResult {
        // 从 JSON 中提取 model 字段
        let requested_model = raw_body.get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        
        tracing::info!("收到 Responses 请求: model='{}'", requested_model);

        // 1. 先尝试直接解析模型（格式: prefix/model 或 prefix/org/model）
        if let Some((provider, actual_model, _)) = self.resolve_model(&requested_model) {
            // 检查该模型是否支持 responses 端点
            let supports_responses = provider.models.iter()
                .find(|m| m.name == actual_model)
                .map(|m| m.supports(EndpointCapability::Responses))
                .unwrap_or(false);

            if supports_responses {
                tracing::info!(
                    "模型 '{}' 支持 Responses 端点，直连发送 (provider='{}')",
                    actual_model, provider.name
                );
                return self.execute_responses_with_provider_raw(
                    &provider,
                    &actual_model,
                    raw_body,
                    requested_model,
                ).await;
            }
        }

        // 2. 遍历所有活跃 Provider，查找是否有模型名完全匹配（处理无前缀请求）
        let providers = self.state.get_providers();
        for provider in providers.iter().filter(|p| p.is_active) {
            if let Some(model_config) = provider.models.iter().find(|m| m.name == requested_model) {
                if model_config.supports(EndpointCapability::Responses) {
                    tracing::info!(
                        "在 Provider '{}' 中找到模型 '{}' 支持 Responses 端点，直连发送",
                        provider.name, requested_model
                    );
                    return self.execute_responses_with_provider_raw(
                        provider,
                        &requested_model,
                        raw_body,
                        requested_model.clone(),
                    ).await;
                }
            }
        }

        // 3. 尝试通过 Group 查找 (responses 端点)
        let group = self.find_group(&requested_model, Some("responses"));

        if let Some(group) = group {
            // 检查 Group 中是否有模型支持 responses 端点
            let ordered_models = self.select_model_by_strategy(&group);

            for group_model in &ordered_models {
                if let Some((provider, actual_model, _)) = self.resolve_model(&group_model.model) {
                    // 检查该模型是否支持 responses 端点
                    let supports_responses = provider.models.iter()
                        .find(|m| m.name == actual_model)
                        .map(|m| m.supports(EndpointCapability::Responses))
                        .unwrap_or(false);

                    if supports_responses {
                        tracing::info!(
                            "Group 模型 '{}' 支持 Responses 端点，直连发送",
                            group_model.model
                        );
                        // 直接发送 Responses 请求
                        let result = self.execute_responses_with_provider_raw(
                            &provider,
                            &actual_model,
                            raw_body,
                            requested_model.clone(),
                        ).await;

                        if result.response.is_some() {
                            return result;
                        }

                        tracing::warn!("直连 Responses 失败: {:?}", result.error);
                        continue;
                    }
                }
            }

            // 没有模型支持 responses 端点，转换为 Chat 格式
            tracing::info!("Group '{}' 没有模型支持 Responses 端点，转换为 Chat 格式", group.name);
        }

        // 4. 降级：转换为 Chat 请求
        tracing::info!("模型 '{}' 不支持 Responses 端点或未找到，转换为 Chat 格式", requested_model);
        
        // 解析为 ResponsesRequest 再转换
        match serde_json::from_value::<ResponsesRequest>(raw_body.clone()) {
            Ok(responses_request) => {
                let chat_request = responses_to_chat_request(&responses_request);
                self.route_chat(chat_request).await
            }
            Err(e) => {
                RouteResult {
                    response: None,
                    error: Some(format!("无效的 Responses 请求格式: {}", e)),
                    info: RouteInfo {
                        provider_name: None,
                        provider_prefix: None,
                        actual_model: None,
                        requested_model,
                        actual_url: None,
                        protocol_transform: None,
                        endpoint_type: None,
                    },
                    actual_request_body: None,
                }
            }
        }
    }

    /// 路由 Responses API 请求（兼容旧接口）
    pub async fn route_responses(&self, request: ResponsesRequest) -> RouteResult {
        let raw_body = serde_json::to_value(&request).unwrap_or_default();
        self.route_responses_raw(&raw_body).await
    }

    /// 路由 Claude Messages API 请求（直连模式：不做格式转换，直接转发原始请求）
    pub async fn route_claude_messages_raw(&self, raw_body: &serde_json::Value) -> RouteResult {
        // 从 JSON 中提取 model 字段
        let requested_model = raw_body.get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        
        tracing::info!("收到 Claude Messages 请求: model='{}'", requested_model);

        // 1. 尝试通过 Group 查找 (endpoint_type = claude 或 messages)
        let group = self.find_group(&requested_model, Some("claude"))
            .or_else(|| self.find_group(&requested_model, Some("messages")));

        if let Some(group) = group {
            tracing::info!(
                "Claude Messages 找到 Group: name='{}', endpoint_type={:?}",
                group.name, group.endpoint_type
            );
            let ordered_models = self.select_model_by_strategy(&group);

            for group_model in &ordered_models {
                if let Some((provider, actual_model, _)) = self.resolve_model(&group_model.model) {
                    let result = self.execute_claude_messages_with_provider_raw(
                        &provider,
                        &actual_model,
                        raw_body,
                        requested_model.clone(),
                    ).await;

                    if result.response.is_some() {
                        return result;
                    }

                    tracing::warn!("Claude Messages 直连失败: {:?}", result.error);
                    continue;
                }
            }
        }

        // 2. 尝试直接解析模型（格式: prefix/model 或 直接模型名）
        if let Some((provider, actual_model, _)) = self.resolve_model(&requested_model) {
            tracing::info!(
                "Claude Messages 直连: model='{}' -> provider='{}', actual_model='{}'",
                requested_model, provider.name, actual_model
            );
            return self.execute_claude_messages_with_provider_raw(
                &provider,
                &actual_model,
                raw_body,
                requested_model,
            ).await;
        }

        // 3. 未找到 Provider
        RouteResult {
            response: None,
            error: Some(format!("未找到模型 '{}' 对应的 Provider", requested_model)),
            info: RouteInfo {
                provider_name: None,
                provider_prefix: None,
                actual_model: None,
                requested_model,
                actual_url: None,
                protocol_transform: None,
                endpoint_type: Some("messages".to_string()),
            },
            actual_request_body: None,
        }
    }

    /// 直接发送 Claude Messages 请求到 Provider（保留原始请求体）
    async fn execute_claude_messages_with_provider_raw(
        &self,
        provider: &Provider,
        actual_model: &str,
        raw_body: &serde_json::Value,
        requested_model: String,
    ) -> RouteResult {
        self.execute_raw_request(
            provider,
            actual_model,
            raw_body,
            requested_model,
            "/v1/messages",
            "messages",
        ).await
    }

    /// 直接发送 Responses 请求到 Provider（保留原始请求体）
    async fn execute_responses_with_provider_raw(
        &self,
        provider: &Provider,
        actual_model: &str,
        raw_body: &serde_json::Value,
        requested_model: String,
    ) -> RouteResult {
        self.execute_raw_request(
            provider,
            actual_model,
            raw_body,
            requested_model,
            "/v1/responses",
            "responses",
        ).await
    }

    /// 通用原始请求发送方法（带三级延迟重试）
    async fn execute_raw_request(
        &self,
        provider: &Provider,
        actual_model: &str,
        raw_body: &serde_json::Value,
        requested_model: String,
        endpoint_path: &str,
        endpoint_type: &str,
    ) -> RouteResult {
        let retry_config = RetryConfig::default();
        let circuit_key = format!("{}:{}", provider.prefix, actual_model);

        // 检查熔断器
        if !self.circuit_breaker.allow_request(&circuit_key).await {
            return RouteResult::error_result(
                provider,
                actual_model,
                requested_model,
                Some(endpoint_type),
                format!("Provider '{}' 处于熔断状态", provider.name),
            );
        }

        // 检查速率限制
        if let Err(e) = self.rate_limiter.check_rate_limit(&provider.id).await {
            return RouteResult::error_result(
                provider,
                actual_model,
                requested_model,
                Some(endpoint_type),
                format!("Provider '{}' 触发速率限制: {:?}", provider.name, e),
            );
        }

        // 获取认证凭证
        let auth_value = match provider.get_auth_value() {
            Some(v) => v,
            None => {
                return RouteResult::error_result(
                    provider,
                    actual_model,
                    requested_model,
                    Some(endpoint_type),
                    format!("Provider '{}' 未配置认证信息", provider.name),
                );
            }
        };

        // 构建 API URL
        let url = build_api_url(&provider.base_url, endpoint_path);

        // 保留原始请求体，只替换 model 字段
        let mut body = raw_body.clone();
        if let Some(obj) = body.as_object_mut() {
            obj.insert("model".to_string(), serde_json::json!(actual_model));
        }

        // 构建请求头
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::CONTENT_TYPE, reqwest::header::HeaderValue::from_static("application/json"));
        if let (Ok(h), Ok(v)) = (
            reqwest::header::HeaderName::from_bytes(provider.auth_header.as_bytes()),
            auth_value.parse(),
        ) {
            headers.insert(h, v);
        }
        for (key, value) in &provider.headers {
            if let (Ok(k), Ok(v)) = (
                reqwest::header::HeaderName::from_bytes(key.as_bytes()),
                reqwest::header::HeaderValue::from_str(value),
            ) {
                headers.insert(k, v);
            }
        }

        tracing::info!(
            "直连发送 {} 请求: url='{}', model='{}', provider='{}'",
            endpoint_type, url, actual_model, provider.name
        );

        let info = RouteInfo {
            provider_name: Some(provider.name.clone()),
            provider_prefix: Some(provider.prefix.clone()),
            actual_model: Some(actual_model.to_string()),
            requested_model,
            actual_url: Some(url.clone()),
            protocol_transform: Some("direct".to_string()),
            endpoint_type: Some(endpoint_type.to_string()),
        };

        // 重试循环
        let mut last_error: Option<String> = None;
        for attempt in 0..=retry_config.max_retries {
            if attempt > 0 {
                tracing::info!(
                    "重试 {} 请求: provider='{}', model='{}', attempt={}/{}",
                    endpoint_type, provider.name, actual_model, attempt, retry_config.max_retries
                );
            }

            let response = self.http_client()
                .post(&url)
                .headers(headers.clone())
                .json(&body)
                .send()
                .await;

            match response {
                Ok(resp) => {
                    let status = resp.status();

                    if status.is_success() {
                        self.circuit_breaker.record_success(&circuit_key).await;
                        self.rate_limiter.clear_cooldown(&provider.id).await;
                        return RouteResult {
                            response: Some(resp),
                            error: None,
                            info,
                            actual_request_body: Some(body),
                        };
                    }

                    // 检查是否可重试
                    if retry::is_retryable_status(status.as_u16()) && attempt < retry_config.max_retries {
                        self.circuit_breaker.record_failure(&circuit_key).await;
                        self.rate_limiter.start_cooldown(&provider.id, std::time::Duration::from_secs(30)).await;

                        let retry_after = resp
                            .headers()
                            .get("retry-after")
                            .and_then(|v| v.to_str().ok())
                            .and_then(retry::parse_retry_after);

                        let cooldown = self.rate_limiter.get_cooldown_remaining(&provider.id).await;

                        let delay = retry::compute_retry_delay(attempt, &retry_config, retry_after, cooldown);
                        tracing::warn!(
                            "{} 请求返回可重试状态码 {}: 延迟 {:?} 后重试",
                            endpoint_type, status, delay
                        );
                        tokio::time::sleep(delay).await;
                        continue;
                    }

                    // 不可重试
                    if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                        self.circuit_breaker.record_failure(&circuit_key).await;
                        self.rate_limiter.start_cooldown(&provider.id, std::time::Duration::from_secs(30)).await;
                    }

                    return RouteResult {
                        response: Some(resp),
                        error: None,
                        info,
                        actual_request_body: Some(body),
                    };
                }
                Err(e) => {
                    self.circuit_breaker.record_failure(&circuit_key).await;
                    last_error = Some(format!("请求失败: {}", e));
                    if attempt < retry_config.max_retries {
                        let delay = retry::compute_retry_delay(attempt, &retry_config, None, None);
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    return RouteResult {
                        response: None,
                        error: last_error,
                        info,
                        actual_request_body: Some(body),
                    };
                }
            }
        }

        // 所有重试都失败
        RouteResult {
            response: None,
            error: last_error,
            info,
            actual_request_body: Some(body),
        }
    }

    /// 查找 Group
    fn find_group(&self, model_name: &str, endpoint_type: Option<&str>) -> Option<Group> {
        // 1. 精确匹配（根据端点类型）
        if let Some(group) = self.state.get_group_by_name(model_name, endpoint_type) {
            return Some(group);
        }

        // 2. 映射表匹配
        let mappings = self.state.get_model_mappings();
        let mut matched: Vec<&ModelMapping> = mappings
            .iter()
            .filter(|m| m.matches(model_name))
            .collect();

        matched.sort_by_key(|m| m.priority);
        if let Some(mapping) = matched.first() {
            return self.state.get_group(&mapping.group_id);
        }

        None
    }

    /// 根据策略选择模型
    fn select_model_by_strategy(&self, group: &Group) -> Vec<GroupModel> {
        strategies::select_model_by_strategy(self, group)
    }

    /// 使用 Group 执行请求
    async fn execute_with_group(&self, group: &Group, mut request: ChatRequest, requested_model: String) -> RouteResult {
        if group.models.is_empty() {
            return RouteResult {
                response: None,
                error: Some(format!("Group '{}' 没有配置模型", group.name)),
                info: RouteInfo {
                    provider_name: None,
                    provider_prefix: None,
                    actual_model: None,
                    requested_model,
                    actual_url: None,
                    protocol_transform: None,
                    endpoint_type: None,
                },
                actual_request_body: None,
            };
        }

        // 根据策略选择模型顺序
        let ordered_models = self.select_model_by_strategy(group);
        let mut last_result: Option<RouteResult> = None;

        for group_model in &ordered_models {
            // 解析模型：前缀/模型名
            let (provider, actual_model, _target_format) = match self.resolve_model(&group_model.model) {
                Some(result) => result,
                None => {
                    tracing::warn!("无法解析模型: {}", group_model.model);
                    continue;
                }
            };

            tracing::info!(
                "Group 路由: strategy={:?}, group_model='{}' -> provider='{}', actual_model='{}'",
                group.strategy, group_model.model, provider.name, actual_model
            );

            request.model = actual_model.clone();

            let result = self.execute_with_provider(&provider, &actual_model, request.clone(), requested_model.clone()).await;

            if result.response.is_some() {
                // 记录模型使用
                self.state.group_strategy_state
                    .record_model_usage(&group.id, &group_model.model);
                return result;
            }

            tracing::warn!("Provider '{}' 执行失败: {:?}", provider.name, result.error);

            if let Some(ref error) = result.error {
                if !Self::should_fallback(error) {
                    return result;
                }
            }

            last_result = Some(result);

            tokio::time::sleep(tokio::time::Duration::from_millis(
                group.config.retry_delay_ms as u64,
            ))
            .await;
        }

        last_result.unwrap_or_else(|| RouteResult {
            response: None,
            error: Some(format!("Group '{}' 所有模型都失败了", group.name)),
            info: RouteInfo {
                provider_name: None,
                provider_prefix: None,
                actual_model: None,
                requested_model,
                actual_url: None,
                protocol_transform: None,
                    endpoint_type: None,
            },
            actual_request_body: None,
        })
    }

    /// 执行单个模型请求
    async fn execute_single_model(&self, model: &str, request: ChatRequest, requested_model: String) -> RouteResult {
        let (provider, actual_model, _target_format) = match self.resolve_model(model) {
            Some(result) => result,
            None => {
                return RouteResult {
                    response: None,
                    error: Some(format!("无法解析模型: {}", model)),
                    info: RouteInfo {
                        provider_name: None,
                        provider_prefix: None,
                        actual_model: None,
                        requested_model,
                        actual_url: None,
                        protocol_transform: None,
                    endpoint_type: None,
                    },
                    actual_request_body: None,
                };
            }
        };

        self.execute_with_provider(&provider, &actual_model, request, requested_model).await
    }

    /// 使用 Group 执行 embedding 请求
    async fn execute_embedding_with_group(&self, group: &Group, request: &EmbeddingRequest, requested_model: String) -> RouteResult {
        if group.models.is_empty() {
            return RouteResult {
                response: None,
                error: Some(format!("Group '{}' 没有配置模型", group.name)),
                info: RouteInfo { provider_name: None, provider_prefix: None, actual_model: None, requested_model, actual_url: None, protocol_transform: None, endpoint_type: None },
                actual_request_body: None,
            };
        }

        let ordered_models = self.select_model_by_strategy(group);
        let mut last_result: Option<RouteResult> = None;

        for group_model in &ordered_models {
            let (provider, actual_model, _target_format) = match self.resolve_model(&group_model.model) {
                Some(result) => result,
                None => continue,
            };

            let result = self.execute_embedding_with_provider(&provider, &actual_model, request, requested_model.clone()).await;

            if result.response.is_some() {
                self.state.group_strategy_state.record_model_usage(&group.id, &group_model.model);
                return result;
            }

            if let Some(ref error) = result.error {
                if !Self::should_fallback(error) {
                    return result;
                }
            }

            last_result = Some(result);
            tokio::time::sleep(tokio::time::Duration::from_millis(group.config.retry_delay_ms as u64)).await;
        }

        last_result.unwrap_or_else(|| RouteResult {
            response: None,
            error: Some(format!("Group '{}' 所有模型都失败了", group.name)),
            info: RouteInfo { provider_name: None, provider_prefix: None, actual_model: None, requested_model, actual_url: None, protocol_transform: None, endpoint_type: None },
            actual_request_body: None,
        })
    }

    /// 执行单个模型 embedding 请求
    async fn execute_embedding_single_model(&self, model: &str, request: &EmbeddingRequest, requested_model: String) -> RouteResult {
        let (provider, actual_model, _target_format) = match self.resolve_model(model) {
            Some(result) => result,
            None => {
                return RouteResult {
                    response: None,
                    error: Some(format!("无法解析模型: {}", model)),
                    info: RouteInfo { provider_name: None, provider_prefix: None, actual_model: None, requested_model, actual_url: None, protocol_transform: None, endpoint_type: None },
                    actual_request_body: None,
                };
            }
        };

        self.execute_embedding_with_provider(&provider, &actual_model, request, requested_model).await
    }

    /// 使用 Provider 执行 embedding 请求
    async fn execute_embedding_with_provider(
        &self,
        provider: &Provider,
        actual_model: &str,
        request: &EmbeddingRequest,
        requested_model: String,
    ) -> RouteResult {
        let auth_value = match provider.get_auth_value() {
            Some(v) => v,
            None => {
                return RouteResult::error_result(
                    provider,
                    actual_model,
                    requested_model,
                    None,
                    format!("Provider '{}' 未配置认证信息", provider.name),
                );
            }
        };

        let url = build_api_url(&provider.base_url, "/v1/embeddings");

        let info = RouteInfo {
            provider_name: Some(provider.name.clone()),
            provider_prefix: Some(provider.prefix.clone()),
            actual_model: Some(actual_model.to_string()),
            requested_model,
            actual_url: Some(url.clone()),
            protocol_transform: Some("direct".to_string()),
            endpoint_type: None,
        };

        let body = serde_json::json!({
            "model": actual_model,
            "input": request.input,
        });

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::CONTENT_TYPE, reqwest::header::HeaderValue::from_static("application/json"));
        if let (Ok(h), Ok(v)) = (
            reqwest::header::HeaderName::from_bytes(provider.auth_header.as_bytes()),
            auth_value.parse(),
        ) {
            headers.insert(h, v);
        }
        for (key, value) in &provider.headers {
            if let (Ok(k), Ok(v)) = (
                reqwest::header::HeaderName::from_bytes(key.as_bytes()),
                reqwest::header::HeaderValue::from_str(value),
            ) {
                headers.insert(k, v);
            }
        }

        // 使用共享 HTTP 客户端
        let response = match self.http_client().post(&url).headers(headers).json(&body).send().await {
            Ok(r) => r,
            Err(e) => {
                return RouteResult {
                    response: None,
                    error: Some(format!("请求发送失败: {}", e)),
                    info,
                    actual_request_body: Some(body),
                };
            }
        };

        RouteResult {
            response: Some(response),
            error: None,
            info,
            actual_request_body: Some(body),
        }
    }

    /// 解析模型：前缀/模型名 或 前缀/端点类型/模型名 或 模型名（推断）
    /// 返回 (Provider, 模型名, 端点类型)
    /// 解析模型：前缀/模型名 或 前缀/端点类型/模型名 或 模型名（推断）
    /// 返回 (Provider, 模型名, 目标ApiFormat)
    fn resolve_model(&self, model: &str) -> Option<(Provider, String, ApiFormat)> {
        let (provider, model_name, endpoint_format) = self.resolve_model_with_endpoint(model)?;
        // 如果路由指定了格式，使用指定的；否则使用 Provider 默认的
        let target_format = endpoint_format.unwrap_or(provider.api_format);
        Some((provider, model_name, target_format))
    }

    /// 解析模型并返回端点类型
    /// 格式：
    /// - `prefix/model` -> 使用 Provider 默认 api_format
    /// - `prefix/endpoint_type/model` -> 使用指定端点类型（endpoint_type 必须是有效的端点名）
    fn resolve_model_with_endpoint(&self, model: &str) -> Option<(Provider, String, Option<ApiFormat>)> {
        let parts: Vec<&str> = model.splitn(3, '/').collect();

        // 有效的端点类型名称
        let valid_endpoints = ["chat", "responses", "messages", "gemini", "claude"];

        match parts.len() {
            3 => {
                let prefix = parts[0];
                let possible_endpoint = parts[1].to_lowercase();
                let remaining = parts[2];

                // 检查第二部分是否是有效的端点类型
                if valid_endpoints.contains(&possible_endpoint.as_str()) {
                    // 格式: prefix/endpoint_type/model
                    let api_format = match possible_endpoint.as_str() {
                        "chat" => ApiFormat::OpenAI,
                        "responses" => ApiFormat::OpenAI,
                        "messages" => ApiFormat::Claude,
                        "gemini" => ApiFormat::Gemini,
                        "claude" => ApiFormat::Claude,
                        _ => ApiFormat::OpenAI,
                    };

                    if let Some(provider) = self.state.get_provider_by_prefix(prefix) {
                        tracing::debug!("解析模型(带端点): '{}' -> provider='{}', model='{}', format={:?}", model, provider.name, remaining, api_format);
                        return Some((provider, remaining.to_string(), Some(api_format)));
                    }
                    if let Some(provider) = self.state.get_provider(prefix) {
                        return Some((provider, remaining.to_string(), Some(api_format)));
                    }
                } else {
                    // 格式: prefix/model（模型名本身包含 /）
                    let model_name = format!("{}/{}", parts[1], remaining);
                    if let Some(provider) = self.state.get_provider_by_prefix(prefix) {
                        tracing::debug!("解析模型(模型名含/): '{}' -> provider='{}', model='{}'", model, provider.name, model_name);
                        return Some((provider, model_name, None));
                    }
                    if let Some(provider) = self.state.get_provider(prefix) {
                        return Some((provider, model_name, None));
                    }
                }

                tracing::warn!("解析模型失败: '{}', 未找到 Provider 前缀 '{}'", model, prefix);
                None
            }
            2 => {
                // 格式: prefix/model
                let prefix = parts[0];
                let model_name = parts[1];

                if let Some(provider) = self.state.get_provider_by_prefix(prefix) {
                    tracing::debug!("解析模型: '{}' -> provider='{}', model='{}'", model, provider.name, model_name);
                    return Some((provider, model_name.to_string(), None));
                }

                if let Some(provider) = self.state.get_provider(prefix) {
                    tracing::debug!("解析模型: '{}' -> provider='{}', model='{}'", model, provider.name, model_name);
                    return Some((provider, model_name.to_string(), None));
                }

                tracing::warn!("解析模型失败: '{}', 未找到 Provider 前缀 '{}'", model, prefix);
                None
            }
            1 => {
                // 没有前缀，推断 Provider
                let provider_id = Self::infer_provider(model);
                tracing::warn!(
                    "模型 '{}' 没有供应商前缀，推断为 '{}'",
                    model, provider_id
                );
                if let Some(provider) = self.state.get_provider_by_prefix(&provider_id)
                    .or_else(|| self.state.get_provider(&provider_id))
                {
                    tracing::warn!(
                        "使用推断的 Provider: '{}', 请确认模型 '{}' 是否正确",
                        provider.name, model
                    );
                    return Some((provider, model.to_string(), None));
                }

                // 最后尝试任意活跃的 Provider
                tracing::warn!("推断的 Provider '{}' 不存在，使用任意活跃的 Provider", provider_id);
                let providers = self.state.get_providers();
                providers.into_iter().find(|p| p.is_active).map(|p| {
                    tracing::warn!("使用 Provider: '{}', 模型: '{}'", p.name, model);
                    (p, model.to_string(), None)
                })
            }
            _ => None,
        }
    }

    /// 使用 Provider 执行请求（带三级延迟重试）
    async fn execute_with_provider(
        &self,
        provider: &Provider,
        actual_model: &str,
        mut request: ChatRequest,
        requested_model: String,
    ) -> RouteResult {
        let retry_config = RetryConfig::default();
        let circuit_key = format!("{}:{}", provider.prefix, actual_model);

        // 检查熔断器
        if !self.circuit_breaker.allow_request(&circuit_key).await {
            tracing::warn!("熔断器阻止请求: provider={}, model={}", provider.name, actual_model);
            return RouteResult::error_result(
                provider,
                actual_model,
                requested_model,
                None,
                format!("Provider '{}' 处于熔断状态", provider.name),
            );
        }

        // 检查速率限制
        if let Err(e) = self.rate_limiter.check_rate_limit(&provider.id).await {
            tracing::warn!("速率限制阻止请求: provider={}, error={:?}", provider.name, e);
            return RouteResult::error_result(
                provider,
                actual_model,
                requested_model,
                None,
                format!("Provider '{}' 触发速率限制: {:?}", provider.name, e),
            );
        }

        // 获取认证凭证
        let auth_value = match provider.get_auth_value() {
            Some(v) => v,
            None => {
                return RouteResult::error_result(
                    provider,
                    actual_model,
                    requested_model,
                    None,
                    format!("Provider '{}' 未配置认证信息", provider.name),
                );
            }
        };

        tracing::info!(
            "发送请求: provider='{}', base_url='{}', model='{}'",
            provider.name, provider.base_url, actual_model
        );

        // 构建 URL（始终使用 OpenAI Chat Completions 格式）
        let url = match self.build_url(&provider.base_url, &ApiFormat::OpenAI, actual_model, request.stream) {
            Ok(u) => u,
            Err(e) => {
                self.circuit_breaker.record_failure(&circuit_key).await;
                return RouteResult::error_result(
                    provider,
                    actual_model,
                    requested_model,
                    None,
                    e.to_string(),
                );
            }
        };

        let info = RouteInfo {
            provider_name: Some(provider.name.clone()),
            provider_prefix: Some(provider.prefix.clone()),
            actual_model: Some(actual_model.to_string()),
            requested_model: requested_model.clone(),
            actual_url: Some(url.clone()),
            protocol_transform: Some("direct".to_string()),
            endpoint_type: None,
        };

        request.model = actual_model.to_string();

        // 直接序列化请求，不做格式转换
        let mut body = match serde_json::to_value(&request) {
            Ok(b) => b,
            Err(e) => {
                return RouteResult {
                    response: None,
                    error: Some(format!("请求序列化失败: {}", e)),
                    info,
                    actual_request_body: None,
                };
            }
        };

        // 清理 null 值和空数组
        fn clean_json(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::Object(map) => {
                    for v in map.values_mut() {
                        clean_json(v);
                    }
                    let keys_to_remove: Vec<String> = map
                        .iter()
                        .filter(|(_, v)| {
                            v.is_null() ||
                            (v.is_array() && v.as_array().unwrap().is_empty()) ||
                            (v.is_object() && v.as_object().unwrap().is_empty())
                        })
                        .map(|(k, _)| k.clone())
                        .collect();
                    for key in keys_to_remove {
                        map.remove(&key);
                    }
                }
                serde_json::Value::Array(arr) => {
                    for v in arr.iter_mut() {
                        clean_json(v);
                    }
                    arr.retain(|v| !v.is_null());
                }
                _ => {}
            }
        }
        clean_json(&mut body);

        // 构建请求头
        let headers = match self.build_headers_with_provider(&auth_value, provider) {
            Ok(h) => h,
            Err(e) => {
                self.circuit_breaker.record_failure(&circuit_key).await;
                return RouteResult {
                    response: None,
                    error: Some(e.to_string()),
                    info,
                    actual_request_body: None,
                };
            }
        };

        tracing::info!(">>> 直连请求 URL: {}", url);
        tracing::info!(
            ">>> 直连请求 Body: {}",
            serde_json::to_string(&body).unwrap_or_else(|_| "serialize error".to_string())
        );

        // 重试循环
        let mut last_error: Option<String> = None;
        for attempt in 0..=retry_config.max_retries {
            if attempt > 0 {
                tracing::info!(
                    "重试请求: provider='{}', model='{}', attempt={}/{}",
                    provider.name, actual_model, attempt, retry_config.max_retries
                );
            }

            let req = self.http_client().post(&url).headers(headers.clone()).json(&body);

            let response = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    self.circuit_breaker.record_failure(&circuit_key).await;
                    self.rate_limiter.start_cooldown(&provider.id, std::time::Duration::from_secs(30)).await;
                    last_error = Some(format!("请求发送失败: {}", e));
                    // 网络错误可重试
                    if attempt < retry_config.max_retries {
                        let delay = retry::compute_retry_delay(attempt, &retry_config, None, None);
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    return RouteResult {
                        response: None,
                        error: last_error,
                        info,
                        actual_request_body: Some(body),
                    };
                }
            };

            let status = response.status();

            if status.is_success() {
                self.circuit_breaker.record_success(&circuit_key).await;
                self.rate_limiter.clear_cooldown(&provider.id).await;
                return RouteResult {
                    response: Some(response),
                    error: None,
                    info,
                    actual_request_body: Some(body),
                };
            }

            // 检查是否可重试
            if retry::is_retryable_status(status.as_u16()) && attempt < retry_config.max_retries {
                self.circuit_breaker.record_failure(&circuit_key).await;
                self.rate_limiter.start_cooldown(&provider.id, std::time::Duration::from_secs(30)).await;

                // 三级延迟：retry-after > 熔断器冷却 > 指数退避
                let retry_after = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(retry::parse_retry_after);

                let cooldown = self.rate_limiter.get_cooldown_remaining(&provider.id).await;

                let delay = retry::compute_retry_delay(attempt, &retry_config, retry_after, cooldown);
                tracing::warn!(
                    "请求返回可重试状态码 {}: 延迟 {:?} 后重试 (attempt={}/{})",
                    status, delay, attempt, retry_config.max_retries
                );
                tokio::time::sleep(delay).await;
                continue;
            }

            // 不可重试的状态码
            if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                self.circuit_breaker.record_failure(&circuit_key).await;
                self.rate_limiter.start_cooldown(&provider.id, std::time::Duration::from_secs(30)).await;
            }

            return RouteResult {
                response: Some(response),
                error: None,
                info,
                actual_request_body: Some(body),
            };
        }

        // 所有重试都失败
        RouteResult {
            response: None,
            error: last_error,
            info,
            actual_request_body: Some(body),
        }
    }

    /// 从模型名推断 Provider
    pub fn infer_provider(model: &str) -> String {
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
            "sf"  // 默认用 SiliconFlow
        } else {
            "ds"  // 默认用 DeepSeek
        }.to_string()
    }

    /// 构建 URL
    fn build_url(
        &self,
        base_url: &str,
        format: &ApiFormat,
        model: &str,
        stream: bool,
    ) -> anyhow::Result<String> {
        let base = base_url.trim_end_matches('/');

        let url = match format {
            ApiFormat::OpenAI => {
                // 如果 base_url 已经包含 /chat/completions，直接使用
                if base.ends_with("/chat/completions") {
                    base.to_string()
                } else if base.ends_with("/v1") {
                    // base_url 已包含 /v1，不需要再加
                    format!("{}/chat/completions", base)
                } else if base.contains("/v2/coding") || base.contains("/v2") {
                    // 百度千帆格式：不需要 /v1 前缀
                    format!("{}/chat/completions", base)
                } else if base.contains("/paas/v4") || base.contains("/api/paas") {
                    // 智谱AI 格式：不需要 /v1 前缀
                    format!("{}/chat/completions", base)
                } else {
                    // 标准 OpenAI 格式
                    format!("{}/v1/chat/completions", base)
                }
            }
            ApiFormat::Claude => {
                if base.ends_with("/v1") {
                    format!("{}/messages", base)
                } else {
                    format!("{}/v1/messages", base)
                }
            }
            ApiFormat::Gemini => {
                let method = if stream { "streamGenerateContent" } else { "generateContent" };
                format!("{}/v1beta/models/{}:{}", base, model, method)
            }
            ApiFormat::Responses => {
                if base.ends_with("/v1") {
                    format!("{}/responses", base)
                } else {
                    format!("{}/v1/responses", base)
                }
            }
        };

        Ok(url)
    }

    /// 构建请求头（带 Provider 自定义配置）
    fn build_headers_with_provider(&self, auth_value: &str, provider: &Provider) -> anyhow::Result<reqwest::header::HeaderMap> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("Content-Type", "application/json".parse()?);

        // 添加认证头
        headers.insert(
            reqwest::header::HeaderName::from_bytes(provider.auth_header.as_bytes())?,
            auth_value.parse()?,
        );

        // 添加 Provider 自定义的额外请求头
        for (key, value) in &provider.headers {
            if let Ok(header_name) = reqwest::header::HeaderName::from_bytes(key.as_bytes()) {
                headers.insert(header_name, value.parse()?);
            }
        }

        // 根据格式添加默认头
        match provider.api_format {
            ApiFormat::Claude => {
                // 如果没有自定义 Anthropic-Version，添加默认值
                if !provider.headers.contains_key("Anthropic-Version") {
                    headers.insert("anthropic-version", "2023-06-01".parse()?);
                }
            }
            ApiFormat::Gemini => {
                // Gemini 通过 URL 参数认证，不需要额外头
            }
            ApiFormat::OpenAI => {
                // OpenAI 格式不需要额外默认头
            }
            ApiFormat::Responses => {
                // Responses API 格式不需要额外默认头
            }
        }

        Ok(headers)
    }

    /// 判断是否应该 fallback
    fn should_fallback(error_str: &str) -> bool {
        let error_lower = error_str.to_lowercase();

        error_lower.contains("timeout")
            || error_lower.contains("connection")
            || error_lower.contains("503")
            || error_lower.contains("502")
            || error_lower.contains("500")
            || error_lower.contains("429")
            || error_lower.contains("rate limit")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Group, GroupModel, GroupStrategy, ModelMapping, Provider, ApiFormat};
    use chrono::Utc;

    fn make_provider(id: &str, name: &str, prefix: &str, format: ApiFormat) -> Provider {
        let now = Utc::now();
        Provider {
            id: id.to_string(),
            name: name.to_string(),
            prefix: prefix.to_string(),
            base_url: "https://api.example.com".to_string(),
            api_key: Some("test-key".to_string()),
            api_format: format,
            models: vec!["test-model".into()],
            enable_cost: false,
            auth_type: crate::models::AuthType::ApiKey,
            oauth: None,
            oauth_tokens: None,
            headers: Default::default(),
            auth_header: "Authorization".to_string(),
            auth_prefix: Some("Bearer".to_string()),
            currency: "CNY".to_string(),
            is_active: true,
            is_builtin: false,
            created_at: now,
            updated_at: now,
        }
    }

    fn make_group(name: &str, models: Vec<&str>, strategy: GroupStrategy) -> Group {
        let now = Utc::now();
        Group {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            description: None,
            models: models.iter().map(|m| GroupModel::new(m.to_string())).collect(),
            strategy,
            config: Default::default(),
            endpoint_type: None,
            is_active: true,
            created_at: now,
            updated_at: now,
        }
    }

    // ============ should_fallback tests ============

    #[test]
    fn test_should_fallback_timeout() {
        assert!(Router::should_fallback("request timeout after 30s"));
        assert!(Router::should_fallback("connection timeout"));
    }

    #[test]
    fn test_should_fallback_connection() {
        assert!(Router::should_fallback("connection refused"));
        assert!(Router::should_fallback("connection reset by peer"));
    }

    #[test]
    fn test_should_fallback_server_errors() {
        assert!(Router::should_fallback("HTTP 500 Internal Server Error"));
        assert!(Router::should_fallback("HTTP 502 Bad Gateway"));
        assert!(Router::should_fallback("HTTP 503 Service Unavailable"));
    }

    #[test]
    fn test_should_fallback_rate_limit() {
        assert!(Router::should_fallback("HTTP 429 Too Many Requests"));
        assert!(Router::should_fallback("rate limit exceeded"));
    }

    #[test]
    fn test_should_not_fallback_auth_error() {
        assert!(!Router::should_fallback("invalid api key"));
        assert!(!Router::should_fallback("authentication failed"));
        assert!(!Router::should_fallback("model not found"));
    }

    #[test]
    fn test_should_not_fallback_client_error() {
        assert!(!Router::should_fallback("HTTP 400 Bad Request"));
        assert!(!Router::should_fallback("HTTP 404 Not Found"));
    }

    // ============ ModelMapping tests ============

    #[test]
    fn test_model_mapping_exact_match() {
        let mapping = ModelMapping::new("gpt-4".to_string(), "group-1".to_string());
        assert!(mapping.matches("gpt-4"));
        assert!(!mapping.matches("gpt-4-turbo"));
    }

    #[test]
    fn test_model_mapping_prefix_wildcard() {
        let mapping = ModelMapping::new("claude-*".to_string(), "group-1".to_string());
        assert!(mapping.matches("claude-3-opus"));
        assert!(mapping.matches("claude-sonnet-4"));
        assert!(!mapping.matches("gpt-4"));
    }

    #[test]
    fn test_model_mapping_suffix_wildcard() {
        let mapping = ModelMapping::new("*-turbo".to_string(), "group-1".to_string());
        assert!(mapping.matches("gpt-4-turbo"));
        assert!(mapping.matches("claude-turbo"));
        assert!(!mapping.matches("gpt-4"));
    }

    #[test]
    fn test_model_mapping_catch_all() {
        let mapping = ModelMapping::new("*".to_string(), "group-1".to_string());
        assert!(mapping.matches("anything"));
        assert!(mapping.matches("gpt-4"));
        assert!(mapping.matches(""));
    }

    // ============ Group strategy tests ============

    #[test]
    fn test_group_priority_ordering() {
        let mut group = make_group("test", vec!["a", "b", "c"], GroupStrategy::Priority);
        group.models[0].priority = 2;
        group.models[1].priority = 0;
        group.models[2].priority = 1;

        let ordered = group.get_ordered_models();
        assert_eq!(ordered[0].model, "b");
        assert_eq!(ordered[1].model, "c");
        assert_eq!(ordered[2].model, "a");
    }

    // ============ Provider infer tests ============

    #[test]
    fn test_infer_provider_claude() {
        assert_eq!(Router::infer_provider("claude-3-opus"), "ant");
        assert_eq!(Router::infer_provider("anthropic-model"), "ant");
    }

    #[test]
    fn test_infer_provider_gemini() {
        assert_eq!(Router::infer_provider("gemini-pro"), "gc");
        assert_eq!(Router::infer_provider("gemma-2"), "gc");
    }

    #[test]
    fn test_infer_provider_openai() {
        assert_eq!(Router::infer_provider("gpt-4"), "oai");
        assert_eq!(Router::infer_provider("gpt-3.5-turbo"), "oai");
        assert_eq!(Router::infer_provider("o1-mini"), "oai");
    }

    #[test]
    fn test_infer_provider_chinese_models() {
        assert_eq!(Router::infer_provider("deepseek-chat"), "ds");
        assert_eq!(Router::infer_provider("moonshot-v1"), "ms");
        assert_eq!(Router::infer_provider("kimi-latest"), "ms");
        assert_eq!(Router::infer_provider("glm-4"), "zp");
        assert_eq!(Router::infer_provider("qwen-turbo"), "qw");
        assert_eq!(Router::infer_provider("ernie-bot"), "bd");
    }

    #[test]
    fn test_infer_provider_default() {
        assert_eq!(Router::infer_provider("unknown-model"), "ds");
    }

    // ============ QuotaStatus tests ============

    #[test]
    fn test_quota_status_within_limit() {
        use crate::models::QuotaLimit;
        let limit = QuotaLimit {
            daily_limit: Some(10.0),
            monthly_limit: Some(100.0),
            warning_threshold: 0.8,
        };
        let status = crate::models::QuotaStatus::compute(5.0, 50.0, &limit);
        assert!(!status.is_exceeded);
        assert!(!status.is_warning);
        assert_eq!(status.daily_remaining, Some(5.0));
        assert_eq!(status.monthly_remaining, Some(50.0));
    }

    #[test]
    fn test_quota_status_exceeded() {
        use crate::models::QuotaLimit;
        let limit = QuotaLimit {
            daily_limit: Some(10.0),
            monthly_limit: Some(100.0),
            warning_threshold: 0.8,
        };
        let status = crate::models::QuotaStatus::compute(12.0, 50.0, &limit);
        assert!(status.is_exceeded);
        assert!(status.daily_remaining == Some(0.0));
    }

    #[test]
    fn test_quota_status_warning() {
        use crate::models::QuotaLimit;
        let limit = QuotaLimit {
            daily_limit: Some(10.0),
            monthly_limit: Some(100.0),
            warning_threshold: 0.8,
        };
        let status = crate::models::QuotaStatus::compute(8.5, 50.0, &limit);
        assert!(!status.is_exceeded);
        assert!(status.is_warning);
    }

    #[test]
    fn test_quota_allow_request() {
        use crate::models::QuotaLimit;
        let limit = QuotaLimit {
            daily_limit: Some(10.0),
            monthly_limit: Some(100.0),
            warning_threshold: 0.8,
        };
        let ok_status = crate::models::QuotaStatus::compute(5.0, 50.0, &limit);
        assert!(ok_status.allow_request().is_ok());

        let fail_status = crate::models::QuotaStatus::compute(12.0, 50.0, &limit);
        assert!(fail_status.allow_request().is_err());
    }

    // ============ ApiFormat tests ============

    #[test]
    fn test_api_format_endpoint() {
        assert_eq!(ApiFormat::OpenAI.endpoint_path(), "/v1/chat/completions");
        assert_eq!(ApiFormat::Claude.endpoint_path(), "/v1/messages");
        assert_eq!(ApiFormat::Gemini.endpoint_path(), "/v1beta/models");
    }

    #[test]
    fn test_api_format_name() {
        assert_eq!(ApiFormat::OpenAI.name(), "OpenAI");
        assert_eq!(ApiFormat::Claude.name(), "Claude");
        assert_eq!(ApiFormat::Gemini.name(), "Gemini");
    }

    // ============ OAuth token tests ============

    #[test]
    fn test_oauth_token_not_expired() {
        use crate::models::OAuthTokens;
        let future = Utc::now() + chrono::Duration::hours(1);
        let tokens = OAuthTokens {
            access_token: "test".to_string(),
            refresh_token: None,
            expires_at: Some(future),
            email: None,
        };
        assert!(!tokens.is_expired());
    }

    #[test]
    fn test_oauth_token_expired() {
        use crate::models::OAuthTokens;
        let past = Utc::now() - chrono::Duration::hours(1);
        let tokens = OAuthTokens {
            access_token: "test".to_string(),
            refresh_token: None,
            expires_at: Some(past),
            email: None,
        };
        assert!(tokens.is_expired());
    }

    #[test]
    fn test_oauth_token_needs_refresh() {
        use crate::models::OAuthTokens;
        let soon = Utc::now() + chrono::Duration::minutes(3);
        let tokens = OAuthTokens {
            access_token: "test".to_string(),
            refresh_token: None,
            expires_at: Some(soon),
            email: None,
        };
        assert!(tokens.needs_refresh());
    }

    #[test]
    fn test_oauth_token_no_expiry() {
        use crate::models::OAuthTokens;
        let tokens = OAuthTokens {
            access_token: "test".to_string(),
            refresh_token: None,
            expires_at: None,
            email: None,
        };
        assert!(!tokens.is_expired());
        assert!(!tokens.needs_refresh());
    }
}


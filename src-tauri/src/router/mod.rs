//! UniRoute 路由器
//!
//! 简化架构：请求模型名 → Group → 模型列表 → 选择模型 → 通过前缀找 Provider → 发送请求

mod circuit_breaker;
mod fallback;
mod rate_limiter;

pub use circuit_breaker::*;
pub use fallback::*;
pub use rate_limiter::*;

use crate::models::{ApiFormat, ChatRequest, EmbeddingRequest, ResponsesRequest, Group, GroupModel, GroupStrategy, ModelMapping, Provider};
use crate::state::AppState;
use crate::translator::Translator;
use rand::Rng;
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

/// 路由器
pub struct Router {
    state: Arc<AppState>,
    translator: Box<dyn Translator>,
    rate_limiter: RateLimiter,
    circuit_breaker: CircuitBreaker,
}

impl Router {
    pub fn new(state: Arc<AppState>) -> Self {
        Self {
            state,
            translator: crate::translator::get_translator(),
            rate_limiter: RateLimiter::new(),
            circuit_breaker: CircuitBreaker::new(),
        }
    }

    /// 路由聊天请求
    pub async fn route_chat(&self, request: ChatRequest) -> RouteResult {
        let requested_model = request.model.clone();
        tracing::info!("收到请求: model='{}'", requested_model);

        // 1. 查找 Group
        let group = self.find_group(&requested_model);

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

        let group = self.find_group(&requested_model);

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
    pub async fn route_responses(&self, request: ResponsesRequest) -> RouteResult {
        let requested_model = request.model.clone();
        tracing::info!("收到 Responses 请求: model='{}'", requested_model);

        let chat_request = responses_to_chat_request(&request);
        self.route_chat(chat_request).await
    }

    /// 查找 Group
    fn find_group(&self, model_name: &str) -> Option<Group> {
        // 1. 精确匹配
        if let Some(group) = self.state.get_group_by_name(model_name) {
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
        let models: Vec<_> = group.models.iter().cloned().collect();

        match group.strategy {
            GroupStrategy::Priority => {
                // 按优先级排序
                let mut sorted = models;
                sorted.sort_by_key(|m| m.priority);
                sorted
            }
            GroupStrategy::RoundRobin => {
                // 轮询：选择下一个模型
                let index = self.state.group_strategy_state
                    .next_round_robin_index(&group.id, models.len());

                // 返回以选定模型为首的列表（后续用于故障转移）
                let mut result = Vec::with_capacity(models.len());
                for i in 0..models.len() {
                    result.push(models[(index + i) % models.len()].clone());
                }
                result
            }
            GroupStrategy::Random => {
                // 随机选择一个模型
                let mut rng = rand::thread_rng();
                let index = rng.gen_range(0..models.len());

                // 返回以随机模型为首的列表
                let mut result = Vec::with_capacity(models.len());
                for i in 0..models.len() {
                    result.push(models[(index + i) % models.len()].clone());
                }
                result
            }
            GroupStrategy::Weighted => {
                // 根据权重随机选择
                let total_weight: u32 = models.iter().map(|m| m.weight).sum();
                if total_weight == 0 {
                    return models;
                }

                let mut rng = rand::thread_rng();
                let mut random = rng.gen_range(0..total_weight);

                let mut selected_index = 0;
                for (i, m) in models.iter().enumerate() {
                    if random < m.weight {
                        selected_index = i;
                        break;
                    }
                    random -= m.weight;
                }

                // 返回以选定模型为首的列表
                let mut result = Vec::with_capacity(models.len());
                for i in 0..models.len() {
                    result.push(models[(selected_index + i) % models.len()].clone());
                }
                result
            }
            GroupStrategy::LeastUsed => {
                let mut models_with_usage: Vec<_> = models
                    .iter()
                    .map(|m| {
                        let usage = self.state.group_strategy_state
                            .get_model_usage(&group.id, &m.model);
                        (m.clone(), usage)
                    })
                    .collect();

                models_with_usage.sort_by_key(|(_, usage)| *usage);

                models_with_usage.into_iter().map(|(m, _)| m).collect()
            }
            GroupStrategy::CostOptimized => {
                let pricing_manager = &self.state.pricing_manager;
                let pm = pricing_manager.read();

                let mut models_with_cost: Vec<_> = models
                    .iter()
                    .map(|m| {
                        let cost = self.estimate_model_cost(&pm, &m.model);
                        (m.clone(), cost)
                    })
                    .collect();

                models_with_cost.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

                models_with_cost.into_iter().map(|(m, _)| m).collect()
            }
        }
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
            // 解析模型：前缀/模型名，返回目标格式
            let (provider, actual_model, target_format) = match self.resolve_model(&group_model.model) {
                Some(result) => result,
                None => {
                    tracing::warn!("无法解析模型: {}", group_model.model);
                    continue;
                }
            };

            tracing::info!(
                "Group 路由: strategy={:?}, group_model='{}' -> provider='{}', actual_model='{}', format={:?}",
                group.strategy, group_model.model, provider.name, actual_model, target_format
            );

            request.model = actual_model.clone();

            let result = self.execute_with_provider(&provider, &actual_model, request.clone(), requested_model.clone(), target_format).await;

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
        let (provider, actual_model, target_format) = match self.resolve_model(model) {
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

        self.execute_with_provider(&provider, &actual_model, request, requested_model, target_format).await
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
                return RouteResult {
                    response: None,
                    error: Some(format!("Provider '{}' 未配置认证信息", provider.name)),
                    info: RouteInfo {
                        provider_name: Some(provider.name.clone()),
                        provider_prefix: Some(provider.prefix.clone()),
                        actual_model: Some(actual_model.to_string()),
                        requested_model,
                        actual_url: None,
                        protocol_transform: None,
                    endpoint_type: None,
                    },
                    actual_request_body: None,
                };
            }
        };

        let url = format!("{}/v1/embeddings", provider.base_url.trim_end_matches('/'));

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

        let client = reqwest::Client::new();
        let response = match client.post(&url).headers(headers).json(&body).send().await {
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
    /// - `prefix/endpoint_type/model` -> 使用指定端点类型
    fn resolve_model_with_endpoint(&self, model: &str) -> Option<(Provider, String, Option<ApiFormat>)> {
        let parts: Vec<&str> = model.splitn(3, '/').collect();

        match parts.len() {
            3 => {
                // 格式: prefix/endpoint_type/model
                let prefix = parts[0];
                let endpoint_type_str = parts[1];
                let model_name = parts[2];

                // 解析端点类型
                let api_format = match endpoint_type_str.to_lowercase().as_str() {
                    "chat" => ApiFormat::OpenAI,
                    "responses" => ApiFormat::OpenAI, // Responses 也用 OpenAI 格式，但端点不同
                    "messages" => ApiFormat::Claude,
                    "gemini" => ApiFormat::Gemini,
                    _ => ApiFormat::OpenAI, // 默认
                };

                // 查找 Provider
                if let Some(provider) = self.state.get_provider_by_prefix(prefix) {
                    tracing::debug!("解析模型: '{}' -> provider='{}', model='{}', format={:?}", model, provider.name, model_name, api_format);
                    return Some((provider, model_name.to_string(), Some(api_format)));
                }

                if let Some(provider) = self.state.get_provider(prefix) {
                    tracing::debug!("解析模型: '{}' -> provider='{}', model='{}', format={:?}", model, provider.name, model_name, api_format);
                    return Some((provider, model_name.to_string(), Some(api_format)));
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

    /// 使用 Provider 执行请求
    async fn execute_with_provider(
        &self,
        provider: &Provider,
        actual_model: &str,
        mut request: ChatRequest,
        requested_model: String,
        target_format: ApiFormat,
    ) -> RouteResult {
        let circuit_key = format!("{}:{}", provider.prefix, actual_model);

        // 检查熔断器
        if !self.circuit_breaker.allow_request(&circuit_key).await {
            tracing::warn!("熔断器阻止请求: provider={}, model={}", provider.name, actual_model);
            return RouteResult {
                response: None,
                error: Some(format!("Provider '{}' 处于熔断状态", provider.name)),
                info: RouteInfo {
                    provider_name: Some(provider.name.clone()),
                    provider_prefix: Some(provider.prefix.clone()),
                    actual_model: Some(actual_model.to_string()),
                    requested_model,
                    actual_url: None,
                    protocol_transform: None,
                    endpoint_type: None,
                },
                actual_request_body: None,
            };
        }

        // 检查速率限制
        if let Err(e) = self.rate_limiter.check_rate_limit(&provider.id).await {
            tracing::warn!("速率限制阻止请求: provider={}, error={:?}", provider.name, e);
            return RouteResult {
                response: None,
                error: Some(format!("Provider '{}' 触发速率限制: {:?}", provider.name, e)),
                info: RouteInfo {
                    provider_name: Some(provider.name.clone()),
                    provider_prefix: Some(provider.prefix.clone()),
                    actual_model: Some(actual_model.to_string()),
                    requested_model,
                    actual_url: None,
                    protocol_transform: None,
                    endpoint_type: None,
                },
                actual_request_body: None,
            };
        }

        // 获取认证凭证
        let auth_value = match provider.get_auth_value() {
            Some(v) => v,
            None => {
                return RouteResult {
                    response: None,
                    error: Some(format!("Provider '{}' 未配置认证信息", provider.name)),
                    info: RouteInfo {
                        provider_name: Some(provider.name.clone()),
                        provider_prefix: Some(provider.prefix.clone()),
                        actual_model: Some(actual_model.to_string()),
                        requested_model,
                        actual_url: None,
                        protocol_transform: None,
                    endpoint_type: None,
                    },
                    actual_request_body: None,
                };
            }
        };

        tracing::info!(
            "发送请求: provider='{}', base_url='{}', model='{}'",
            provider.name, provider.base_url, actual_model
        );

        // 构建 URL（使用传入的 target_format）
        let url = match self.build_url(&provider.base_url, &target_format, actual_model, request.stream) {
            Ok(u) => u,
            Err(e) => {
                self.circuit_breaker.record_failure(&circuit_key).await;
                return RouteResult {
                    response: None,
                    error: Some(e.to_string()),
                    info: RouteInfo {
                        provider_name: Some(provider.name.clone()),
                        provider_prefix: Some(provider.prefix.clone()),
                        actual_model: Some(actual_model.to_string()),
                        requested_model,
                        actual_url: None,
                        protocol_transform: None,
                    endpoint_type: None,
                    },
                    actual_request_body: None,
                };
            }
        };

        // 转换请求格式
        let source_format = ApiFormat::OpenAI;
        // target_format 已作为参数传入

        let transform_label = if source_format != target_format {
            format!("{}->{}", source_format.name(), target_format.name())
        } else {
            "direct".to_string()
        };

        let info = RouteInfo {
            provider_name: Some(provider.name.clone()),
            provider_prefix: Some(provider.prefix.clone()),
            actual_model: Some(actual_model.to_string()),
            requested_model,
            actual_url: Some(url.clone()),
            protocol_transform: Some(transform_label.clone()),
            endpoint_type: None,
        };

        request.model = actual_model.to_string();

        let translated_body = if source_format != target_format {
            match self.translator.translate_request(source_format, target_format, &request) {
                Ok(b) => b,
                Err(e) => {
                    self.circuit_breaker.record_failure(&circuit_key).await;
                    return RouteResult {
                        response: None,
                        error: Some(format!("请求格式转换失败: {}", e)),
                        info,
                        actual_request_body: None,
                    };
                }
            }
        } else {
            match serde_json::to_value(&request) {
                Ok(b) => b,
                Err(e) => {
                    return RouteResult {
                        response: None,
                        error: Some(format!("请求序列化失败: {}", e)),
                        info,
                        actual_request_body: None,
                    };
                }
            }
        };

        // 清理 null 值和空数组
        fn clean_json(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::Object(map) => {
                    // 先递归处理所有子值
                    for v in map.values_mut() {
                        clean_json(v);
                    }
                    // 然后删除 null 和空数组
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
                    // 清理后如果数组中的元素是空对象，也处理
                    arr.retain(|v| !v.is_null());
                }
                _ => {}
            }
        }

        let mut cleaned_body = translated_body;
        clean_json(&mut cleaned_body);

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

        tracing::info!(">>> 协议转换: {}", transform_label);
        tracing::info!(">>> 实际请求 URL: {}", url);
        tracing::info!(
            ">>> 实际请求 Body: {}",
            serde_json::to_string(&cleaned_body).unwrap_or_else(|_| "serialize error".to_string())
        );

        // 发送请求
        let client = reqwest::Client::new();
        let req = client.post(&url).headers(headers).json(&cleaned_body);

        let response = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                self.circuit_breaker.record_failure(&circuit_key).await;
                self.rate_limiter.start_cooldown(&provider.id, std::time::Duration::from_secs(30)).await;
                return RouteResult {
                    response: None,
                    error: Some(format!("请求发送失败: {}", e)),
                    info,
                    actual_request_body: Some(cleaned_body),
                };
            }
        };

        // 检查响应状态码
        let status = response.status();
        if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            self.circuit_breaker.record_failure(&circuit_key).await;
            self.rate_limiter.start_cooldown(&provider.id, std::time::Duration::from_secs(30)).await;
        } else if status.is_success() {
            self.circuit_breaker.record_success(&circuit_key).await;
            self.rate_limiter.clear_cooldown(&provider.id).await;
        }

        RouteResult {
            response: Some(response),
            error: None,
            info,
            actual_request_body: Some(cleaned_body),
        }
    }

    /// 估算模型成本（每百万 token 的输入+输出平均价格）
    fn estimate_model_cost(&self, pm: &crate::pricing::PricingManager, model_key: &str) -> f64 {
        // 尝试从 provider 的模型列表中获取定价
        let providers = self.state.get_providers();
        for provider in &providers {
            if let Some(pricing) = provider.get_model_pricing(model_key) {
                return (pricing.input + pricing.output) / 2.0;
            }
            // 也尝试从全局定价中获取
            if let Some(pricing) = pm.get_pricing(&provider.prefix, model_key) {
                return (pricing.input + pricing.output) / 2.0;
            }
        }
        // 默认返回一个高值，让有定价的模型优先
        100.0
    }

    /// 从模型名推断 Provider
    fn infer_provider(model: &str) -> String {
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
                format!("{}/v1/messages", base)
            }
            ApiFormat::Gemini => {
                let method = if stream { "streamGenerateContent" } else { "generateContent" };
                format!("{}/v1beta/models/{}:{}", base, model, method)
            }
            ApiFormat::Responses => {
                format!("{}/v1/responses", base)
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

/// 将 Responses API 请求转换为 Chat API 请求
/// 支持：
/// 1. 标准 Responses API 格式：{"input": ...}
/// 2. Codex 格式（Chat API 风格）：{"messages": [...]}
pub fn responses_to_chat_request(responses_req: &ResponsesRequest) -> ChatRequest {
    // 首先检查是否是 Codex 格式（extra 中有 messages 字段）
    if let Some(messages_value) = responses_req.extra.get("messages") {
        if let Some(messages_array) = messages_value.as_array() {
            tracing::info!("检测到 Codex 格式的 Responses 请求（使用 messages 字段）");
            let messages: Vec<crate::models::Message> = messages_array
                .iter()
                .filter_map(|msg| {
                    let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
                    let content = msg.get("content");
                    
                    let msg_content = match content {
                        Some(c) if c.is_string() => {
                            crate::models::MessageContent::Text(c.as_str().unwrap_or("").to_string())
                        }
                        Some(c) if c.is_array() => {
                            // 处理 Codex 格式的 content 数组: [{"text": "...", "type": "text"}]
                            let parts: Vec<crate::models::ContentPart> = c
                                .as_array()
                                .unwrap()
                                .iter()
                                .filter_map(|part| {
                                    let part_type = part.get("type").and_then(|t| t.as_str()).unwrap_or("text");
                                    let text = part.get("text").and_then(|t| t.as_str());
                                    
                                    if part_type == "text" {
                                        text.map(|t| crate::models::ContentPart {
                                            content_type: "text".to_string(),
                                            text: Some(t.to_string()),
                                            image_url: None,
                                            extra: serde_json::Map::new(),
                                        })
                                    } else if part_type == "image_url" {
                                        part.get("image_url").map(|img_url| crate::models::ContentPart {
                                            content_type: "image_url".to_string(),
                                            text: None,
                                            image_url: Some(crate::models::ImageUrl {
                                                url: img_url.get("url").and_then(|u| u.as_str()).unwrap_or("").to_string(),
                                                detail: None,
                                            }),
                                            extra: serde_json::Map::new(),
                                        })
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            crate::models::MessageContent::Parts(parts)
                        }
                        _ => crate::models::MessageContent::Text(String::new()),
                    };
                    
                    Some(crate::models::Message {
                        role: role.to_string(),
                        content: msg_content,
                        name: None,
                        tool_calls: Vec::new(),
                        tool_call_id: None,
                    })
                })
                .collect();
            
            let tools: Vec<crate::models::Tool> = responses_req.tools.iter()
                .map(|t| {
                    let normalized = t.normalize();
                    tracing::debug!(
                        "Tool normalize: before tool_type={}, function={:?}, after tool_type={}, function={:?}",
                        t.tool_type, t.function, normalized.tool_type, normalized.function
                    );
                    normalized
                })
                .collect();
            
            tracing::info!("Normalized tools count: {}, first tool: {:?}", tools.len(), tools.first());
            
            return ChatRequest {
                model: responses_req.model.clone(),
                messages,
                stream: responses_req.stream,
                temperature: responses_req.temperature.map(|t| t as f32),
                max_tokens: responses_req.max_output_tokens.map(|t| t as u32),
                tools,
                extra: serde_json::Map::new(),
            };
        }
    }
    
    // 标准 Responses API 格式
    let messages = match &responses_req.input {
        Some(crate::models::ResponsesInput::Text(text)) => {
            vec![crate::models::Message {
                role: "user".to_string(),
                content: crate::models::MessageContent::Text(text.clone()),
                name: None,
                tool_calls: Vec::new(),
                tool_call_id: None,
            }]
        }
        Some(crate::models::ResponsesInput::Items(items)) => {
            items
                .iter()
                .filter_map(|item| {
                    // 获取角色（如果没有角色，根据类型推断）
                    let role = item.role.clone().unwrap_or_else(|| {
                        if item.item_type == "message" {
                            "user".to_string()
                        } else {
                            return String::new();
                        }
                    });

                    // 只处理有效的角色
                    if role != "system" && role != "user" && role != "assistant" {
                        return None;
                    }

                    // 转换内容
                    let content = match &item.content {
                        crate::models::ResponsesContent::Text(text) => {
                            crate::models::MessageContent::Text(text.clone())
                        }
                        crate::models::ResponsesContent::Parts(parts) => {
                            let converted: Vec<crate::models::ContentPart> = parts
                                .iter()
                                .filter_map(|p| {
                                    // 转换 Responses API 类型到标准 OpenAI 类型
                                    let normalized_type = match p.content_type.as_str() {
                                        "input_text" | "output_text" => "text",
                                        "input_image" => "image_url",
                                        "refusal" => return None, // 跳过 refusal 类型
                                        other => other,
                                    };

                                    // 只保留有文本内容的 text 类型，或者有图片的 image_url 类型
                                    if normalized_type == "text" {
                                        if let Some(text_content) = &p.text {
                                            Some(crate::models::ContentPart {
                                                content_type: "text".to_string(),
                                                text: Some(text_content.clone()),
                                                image_url: None,
                                                extra: serde_json::Map::new(),
                                            })
                                        } else {
                                            None
                                        }
                                    } else if normalized_type == "image_url" {
                                        if let Some(img_url) = &p.image_url {
                                            Some(crate::models::ContentPart {
                                                content_type: "image_url".to_string(),
                                                text: None,
                                                image_url: Some(img_url.clone()),
                                                extra: serde_json::Map::new(),
                                            })
                                        } else {
                                            None
                                        }
                                    } else {
                                        // 其他类型，保留但清理 extra
                                        Some(crate::models::ContentPart {
                                            content_type: normalized_type.to_string(),
                                            text: p.text.clone(),
                                            image_url: p.image_url.clone(),
                                            extra: serde_json::Map::new(),
                                        })
                                    }
                                })
                                .collect();
                            crate::models::MessageContent::Parts(converted)
                        }
                    };

                    Some(crate::models::Message {
                        role,
                        content,
                        name: None,
                        tool_calls: Vec::new(),
                        tool_call_id: None,
                    })
                })
                .collect()
        }
        Some(crate::models::ResponsesInput::Raw(value)) => {
            // 尝试从原始值中提取信息
            tracing::warn!("Responses API 收到未知的 input 格式，尝试转换: {:?}", value);
            // 如果是对象，尝试提取内容作为文本
            if let Some(text) = value.as_str() {
                vec![crate::models::Message {
                    role: "user".to_string(),
                    content: crate::models::MessageContent::Text(text.to_string()),
                    name: None,
                    tool_calls: Vec::new(),
                    tool_call_id: None,
                }]
            } else {
                // 无法解析，使用 JSON 字符串
                vec![crate::models::Message {
                    role: "user".to_string(),
                    content: crate::models::MessageContent::Text(value.to_string()),
                    name: None,
                    tool_calls: Vec::new(),
                    tool_call_id: None,
                }]
            }
        }
        None => {
            // 没有 input 字段，也没有 messages 字段，使用空消息
            tracing::warn!("Responses API 请求没有 input 或 messages 字段");
            vec![crate::models::Message {
                role: "user".to_string(),
                content: crate::models::MessageContent::Text(String::new()),
                name: None,
                tool_calls: Vec::new(),
                tool_call_id: None,
            }]
        }
    };

    let tools: Vec<crate::models::Tool> = responses_req.tools.iter()
        .map(|t| t.normalize())
        .collect();

    ChatRequest {
        model: responses_req.model.clone(),
        messages,
        stream: responses_req.stream,
        temperature: responses_req.temperature.map(|t| t as f32),
        max_tokens: responses_req.max_output_tokens.map(|t| t as u32),
        tools,
        extra: serde_json::Map::new(),
    }
}

/// 将 Chat API 响应转换为 Responses API 响应
pub fn chat_to_responses_response(chat_resp: &serde_json::Value, requested_model: &str) -> serde_json::Value {
    // 生成 Responses API 格式的 id
    let original_id = chat_resp.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let response_id = if original_id.starts_with("resp_") {
        original_id.to_string()
    } else {
        format!("resp_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..24].to_string())
    };

    let model = chat_resp.get("model").and_then(|v| v.as_str()).unwrap_or(requested_model);

    // 提取 choices
    let choices = chat_resp.get("choices").and_then(|c| c.as_array());
    let first_choice = choices.and_then(|arr| arr.first());

    // 提取 message
    let message = first_choice.and_then(|c| c.get("message"));

    // 提取 content（可能为 null 或字符串）
    let content_text = message
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("");

    // 提取 tool_calls（如果存在）
    let tool_calls = message.and_then(|m| m.get("tool_calls")).and_then(|tc| tc.as_array());

    // 提取 finish_reason
    let finish_reason = first_choice
        .and_then(|c| c.get("finish_reason"))
        .and_then(|v| v.as_str());

    // 确定 status
    let status = match finish_reason {
        Some("stop") => "completed",
        Some("length") => "incomplete",
        Some("tool_calls") => "completed",
        _ => "completed",
    };

    // 构建 output 数组
    let mut output: Vec<serde_json::Value> = Vec::new();

    // 添加 message 项（如果有内容）
    if !content_text.is_empty() {
        output.push(serde_json::json!({
            "type": "message",
            "id": format!("msg_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..24].to_string()),
            "status": "completed",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": content_text,
                "annotations": []
            }]
        }));
    }

    // 添加 function_call 项（如果有 tool_calls）
    if let Some(calls) = tool_calls {
        for call in calls {
            let call_id = call.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let function = call.get("function");
            let name = function.and_then(|f| f.get("name")).and_then(|v| v.as_str()).unwrap_or("");
            let arguments = function.and_then(|f| f.get("arguments")).and_then(|v| v.as_str()).unwrap_or("{}");

            output.push(serde_json::json!({
                "type": "function_call",
                "id": format!("fc_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..24].to_string()),
                "call_id": call_id,
                "name": name,
                "arguments": arguments,
                "status": "completed"
            }));
        }
    }

    // 如果 output 为空，添加一个空的 message
    if output.is_empty() {
        output.push(serde_json::json!({
            "type": "message",
            "id": format!("msg_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..24].to_string()),
            "status": "completed",
            "role": "assistant",
            "content": []
        }));
    }

    // 处理 usage
    let usage = chat_resp.get("usage").map(|u| {
        let input_tokens = u.get("prompt_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
        let output_tokens = u.get("completion_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
        serde_json::json!({
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
            "total_tokens": u.get("total_tokens").and_then(|v| v.as_i64())
                .unwrap_or(input_tokens + output_tokens),
        })
    });

    serde_json::json!({
        "id": response_id,
        "object": "response",
        "created_at": chrono::Utc::now().timestamp(),
        "status": status,
        "error": null,
        "incomplete_details": if status == "incomplete" {
            serde_json::json!({"reason": "max_output_tokens"})
        } else {
            serde_json::Value::Null
        },
        "model": model,
        "output": output,
        "usage": usage,
    })
}

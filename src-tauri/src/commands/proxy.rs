//! Proxy, benchmark, and diagnostic commands

use serde::Serialize;
use crate::state::{AppState, AppStateContainer};
use std::sync::Arc;
use tauri::State;

/// 获取 AppState，未初始化时返回 None
fn get_state(container: &AppStateContainer) -> Option<Arc<AppState>> {
    container.try_get()
}

// ============ Proxy Commands ============

#[tauri::command]
pub async fn start_proxy(port: u16, state: State<'_, AppStateContainer>) -> Result<(), String> {
    let state = get_state(&state).ok_or("应用正在初始化")?;
    if state.is_proxy_running() {
        return Err("代理服务器已在运行".to_string());
    }

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let state_clone = Arc::clone(&state);

    tokio::spawn(async move {
        if let Err(e) = crate::proxy::start_proxy_server(port, state_clone, shutdown_rx).await {
            tracing::error!("代理服务器错误: {}", e);
        }
    });

    let handle = crate::state::ProxyServerHandle { port, shutdown_tx };
    *state.proxy_server.write() = Some(handle);

    tracing::info!("代理服务器已启动，端口: {}", port);
    Ok(())
}

#[tauri::command]
pub async fn stop_proxy(state: State<'_, AppStateContainer>) -> Result<(), String> {
    let state = get_state(&state).ok_or("应用正在初始化")?;
    let mut proxy = state.proxy_server.write();
    if let Some(handle) = proxy.take() {
        let _ = handle.shutdown_tx.send(());
        tracing::info!("代理服务器已停止");
    }
    Ok(())
}

#[tauri::command]
pub fn get_proxy_status(state: State<'_, AppStateContainer>) -> ProxyStatus {
    let Some(state) = get_state(&state) else {
        return ProxyStatus { is_running: false, port: None };
    };
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
    state: State<'_, AppStateContainer>,
) -> Result<BenchmarkResult, String> {
    let state = get_state(&state).ok_or("应用正在初始化")?;
    let start = std::time::Instant::now();
    let model = request.model.unwrap_or_else(|| "gpt-3.5-turbo".to_string());

    let mut results = Vec::new();

    // 并行测速
    let mut handles = Vec::new();
    for provider_id in &request.provider_ids {
        let provider_id = provider_id.clone();
        let model = model.clone();
        let state = Arc::clone(&state);

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

    let start = std::time::Instant::now();

    let result = state.http_client.post(&url).headers(headers).json(&test_body)
        .timeout(std::time::Duration::from_secs(15))
        .send().await;

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

// ============ Diagnostic Commands ============

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
    state: State<'_, AppStateContainer>,
) -> RouteDiagnostic {
    let Some(state) = get_state(&state) else {
        return RouteDiagnostic {
            requested_model: model,
            group_found: None,
            provider_resolution: None,
            warnings: vec!["应用正在初始化".to_string()],
        };
    };

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
                    let _model_name = parts[1];
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
        let inferred = crate::router::Router::infer_provider(&model);
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
        provider_resolution: None,
        warnings,
    }
}

// ============ Endpoint Test ============

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
    state: State<'_, AppStateContainer>,
) -> Result<EndpointTestResult, String> {
    let state = get_state(&state).ok_or("应用正在初始化")?;
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

    let start = std::time::Instant::now();

    let result = state.http_client.post(&url).headers(headers).json(&body)
        .timeout(std::time::Duration::from_secs(30))
        .send().await;
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
fn build_test_request(
    provider: &crate::models::Provider,
    model: &str,
    endpoint_type: crate::models::EndpointCapability,
) -> (String, serde_json::Value) {
    match endpoint_type {
        crate::models::EndpointCapability::Chat => {
            let url = crate::router::build_api_url(&provider.base_url, "/v1/chat/completions");
            let body = serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": "Hi"}],
                "max_tokens": 5,
            });
            (url, body)
        }
        crate::models::EndpointCapability::Responses => {
            let url = crate::router::build_api_url(&provider.base_url, "/v1/responses");
            let body = serde_json::json!({
                "model": model,
                "input": "Hi",
                "max_output_tokens": 5,
            });
            (url, body)
        }
        crate::models::EndpointCapability::Claude => {
            let url = crate::router::build_api_url(&provider.base_url, "/v1/messages");
            let body = serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": "Hi"}],
                "max_tokens": 5,
            });
            (url, body)
        }
        crate::models::EndpointCapability::Gemini => {
            let url = crate::router::build_api_url(&provider.base_url, &format!("/v1beta/models/{}:generateContent", model));
            let body = serde_json::json!({
                "contents": [{"parts": [{"text": "Hi"}]}],
                "generationConfig": {"maxOutputTokens": 5},
            });
            (url, body)
        }
        crate::models::EndpointCapability::Embeddings => {
            let url = crate::router::build_api_url(&provider.base_url, "/v1/embeddings");
            let body = serde_json::json!({
                "model": model,
                "input": "test",
            });
            (url, body)
        }
        crate::models::EndpointCapability::Images => {
            let url = crate::router::build_api_url(&provider.base_url, "/v1/images/generations");
            let body = serde_json::json!({
                "model": model,
                "prompt": "a white cat",
                "n": 1,
                "size": "256x256",
            });
            (url, body)
        }
        crate::models::EndpointCapability::Videos => {
            let url = crate::router::build_api_url(&provider.base_url, "/v1/videos/generations");
            let body = serde_json::json!({
                "model": model,
                "prompt": "a cat walking",
            });
            (url, body)
        }
        crate::models::EndpointCapability::Music => {
            let url = crate::router::build_api_url(&provider.base_url, "/v1/music/generations");
            let body = serde_json::json!({
                "model": model,
                "prompt": "a happy tune",
            });
            (url, body)
        }
        crate::models::EndpointCapability::Audio => {
            let url = crate::router::build_api_url(&provider.base_url, "/v1/audio/transcriptions");
            let body = serde_json::json!({
                "model": model,
            });
            (url, body)
        }
        crate::models::EndpointCapability::TTS => {
            let url = crate::router::build_api_url(&provider.base_url, "/v1/audio/speech");
            let body = serde_json::json!({
                "model": model,
                "input": "test",
                "voice": "alloy",
            });
            (url, body)
        }
        crate::models::EndpointCapability::Moderation => {
            let url = crate::router::build_api_url(&provider.base_url, "/v1/moderations");
            let body = serde_json::json!({
                "model": model,
                "input": "test",
            });
            (url, body)
        }
        crate::models::EndpointCapability::Rerank => {
            let url = crate::router::build_api_url(&provider.base_url, "/v1/rerank");
            let body = serde_json::json!({
                "model": model,
                "query": "test",
                "documents": ["doc1", "doc2"],
            });
            (url, body)
        }
    }
}

// ============ Fetch Models ============

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
    state: State<'_, AppStateContainer>,
) -> Result<Vec<RemoteModel>, String> {
    let state = get_state(&state).ok_or("应用正在初始化")?;
    let provider = state.get_provider(&provider_id)
        .ok_or_else(|| "Provider 不存在".to_string())?;

    let auth_value = provider.get_auth_value()
        .ok_or_else(|| "未配置认证信息".to_string())?;

    let url = crate::router::build_api_url(&provider.base_url, "/v1/models");

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(reqwest::header::CONTENT_TYPE, reqwest::header::HeaderValue::from_static("application/json"));
    if let (Ok(h), Ok(v)) = (
        reqwest::header::HeaderName::from_bytes(provider.auth_header.as_bytes()),
        auth_value.parse(),
    ) {
        headers.insert(h, v);
    }
    // OpenRouter 需要这些 header
    headers.insert("HTTP-Referer", reqwest::header::HeaderValue::from_static("https://uniroute.app"));
    headers.insert("X-Title", reqwest::header::HeaderValue::from_static("UniRoute"));
    for (key, value) in &provider.headers {
        if let (Ok(k), Ok(v)) = (
            reqwest::header::HeaderName::from_bytes(key.as_bytes()),
            reqwest::header::HeaderValue::from_str(value),
        ) {
            headers.insert(k, v);
        }
    }

    let response = state.http_client.get(&url).headers(headers)
        .timeout(std::time::Duration::from_secs(30))
        .send().await
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

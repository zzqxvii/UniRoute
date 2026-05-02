# UniRoute 优化分析报告 — 基于 adk-rust 项目参考

## 概述

参考 adk-rust（Zavora AI 的生产级 Rust AI Agent 框架，v0.7.0）的架构设计，对照 UniRoute 当前实现，识别出以下可落地的优化方向。

---

## 1. 错误处理：从枚举升级为结构化错误信封

### adk-rust 做法
使用 **结构化错误信封**（struct 而非 enum）：
```rust
pub struct AdkError {
    pub component: ErrorComponent,   // Agent, Model, Tool, Session...
    pub category: ErrorCategory,     // RateLimited, Timeout, Unavailable...
    pub code: &'static str,          // "model.rate_limited"
    pub message: String,
    pub retry: RetryHint,            // should_retry, retry_after_ms, max_attempts
    pub details: Box<ErrorDetails>,  // upstream_status_code, request_id, provider
    source: Option<Box<dyn Error + Send + Sync>>,
}
```
- Builder 模式：`.with_source()`, `.with_retry()`, `.with_upstream_status()`, `.with_provider()`
- `RetryHint::for_category()` 根据错误类别自动推导可重试性
- `to_problem_json()` 输出 RFC 7807 标准错误格式
- 编译期 `Send+Sync` 断言

### UniRoute 现状
简单的 enum：`AppError { Provider, Translation, Routing, Storage, Config, RateLimited, CircuitBreakerOpen, QuotaExceeded }`

### 优化建议
- 将 `AppError` 重构为结构化信封，增加 `code`（静态字符串标识符）、`retry`（RetryHint）、`details`（上游状态码、请求 ID、provider 名称）
- 添加 `with_provider()`, `with_upstream_status()` builder 方法，让错误上下文更丰富
- 对接 RFC 7807 错误格式，方便客户端统一处理
- **优先级：中** — 当前 enum 够用，但结构化错误在调试复杂路由链路时价值很大

---

## 2. HTTP 客户端优化：缓存 Headers

### adk-rust 做法
Anthropic 客户端使用 `Arc<HeaderMap>` 缓存认证 headers，每次请求只需 `Arc::clone()` 而非重建：
```rust
let cached_headers: Arc<HeaderMap> = Arc::new(build_headers(api_key));
// 每次请求
let headers = Arc::clone(&cached_headers);  // 几乎零开销
```

### UniRoute 现状
`build_headers_with_provider()` 每次请求都重新构建 `HeaderMap`，包括字符串拼接 auth header。

### 优化建议
- 对每个 Provider 缓存 `Arc<HeaderMap>`（不含动态部分如 request-id）
- Provider 配置变更时重建缓存
- **优先级：高** — 高频请求场景下减少大量重复分配

---

## 3. 可观测性：结构化计数器

### adk-rust 做法
Anthropic 客户端有独立的 `observability.rs`，定义结构化计数器：
```rust
CLIENT_REQUESTS        // 请求总数
CLIENT_REQUEST_DURATION // 请求耗时直方图
CLIENT_REQUEST_ERRORS   // 错误计数
CLIENT_REQUEST_RETRIES  // 重试次数
STREAM_EVENTS          // SSE 事件数
STREAM_ERRORS          // SSE 解析错误
STREAM_BYTES           // 流式字节数
STREAM_DURATION        // 流式总耗时
STREAM_TTFB            // 首字节时间
```
支持 OpenTelemetry OTLP 导出到 Jaeger/Datadog。

### UniRoute 现状
仅有 `tracing` 日志，无结构化指标。

### 优化建议
- 在 proxy handler 中添加关键指标：请求计数、延迟直方图、错误率、流式 TTFB
- 初期可用 `tracing` 的计数宏，后期接入 OpenTelemetry
- Dashboard 页面可展示实时指标（当前只展示 DB 中的历史统计）
- **优先级：中** — 对生产运维价值大，但需要前端配合

---

## 4. 重试逻辑：配置化 + retry-after 优先级

### adk-rust 做法
```rust
pub struct RetryConfig {
    pub enabled: bool,
    pub max_retries: u32,        // default 3
    pub initial_delay: Duration, // default 250ms
    pub max_delay: Duration,     // default 5s
    pub backoff_multiplier: f32, // default 2.0
}
```
- `is_retryable_status_code()`: 408, 429, 500, 502, 503, 504, 529
- 重试优先级：结构化 `retry_after` > 服务端 hint > 指数退避
- `execute_with_retry_hint()` 泛型重试包装器

### UniRoute 现状
`router/retry.rs` 有指数退避+抖动，但重试配置硬编码在代码中。

### 优化建议
- 将重试参数（max_retries, initial_delay, max_delay, backoff_multiplier）提取到 `GroupConfig` 或全局设置
- 在前端 Settings 页面暴露重试配置
- 统一 `is_retryable` 判断逻辑到一个地方（当前分散在 `should_fallback` 和 `retry.rs` 中）
- **优先级：中** — 已有基础实现，配置化是锦上添花

---

## 5. Axum 中间件栈

### adk-rust 做法
```rust
let app = Router::new()
    .merge(routes)
    .layer(TraceLayer::new_for_http())
    .layer(CorsLayer::permissive())
    .layer(TimeoutLayer::new(Duration::from_secs(30)))
    .layer(SetResponseHeaderLayer::overriding(header::SERVER, "adk-server"))
    .layer(DefaultBodyLimit::max(10 * 1024 * 1024))  // 10MB
    .layer(RequestIdLayer);
```
- 请求体大小限制防止 DoS
- 自动生成 `x-request-id` 便于链路追踪
- Server header 统一标识

### UniRoute 现状
`proxy/mod.rs` 仅有基础 CORS 和路由，无请求体限制、无请求 ID、无超时。

### 优化建议
- 添加 `DefaultBodyLimit`（建议 10MB，防止大请求 OOM）
- 添加请求 ID 中间件（`x-request-id`），贯穿到日志和上游请求
- 添加请求超时层（`TimeoutLayer`）
- **优先级：高** — 安全和稳定性直接相关

---

## 6. SSE 流式解析加固

### adk-rust 做法
SSE 解析器（`adk-anthropic/src/sse.rs`）增加：
- **UTF-8 验证 + 部分字节恢复**：跨 chunk 的多字节 UTF-8 字符正确处理
- **结构化错误事件解析**：识别 SSE `event: error` 类型并转换为 `AdkError`
- **观测计数器**：`STREAM_EVENTS`, `STREAM_ERRORS`, `STREAM_BYTES`, `STREAM_DURATION`, `STREAM_TTFB`

### UniRoute 现状
SSE 解析有 1MB buffer / 64KB event / 60s timeout 限制，但无 UTF-8 验证和结构化错误事件处理。

### 优化建议
- 在 SSE 解析器中添加 UTF-8 部分字节恢复逻辑
- 识别上游 `event: error` 并转换为 `AppError` 而非静默忽略
- 记录 SSE 流的 TTFB（首字节时间）到请求日志
- **优先级：低** — 当前实现在大多数场景下够用

---

## 7. 构建配置优化

### adk-rust 做法
```toml
[profile.dev]
debug = 1           # 最小调试信息
opt-level = 0
incremental = true

[profile.dev.package."*"]  # 依赖包用优化编译
opt-level = 1
debug = false

[profile.release]
strip = true
opt-level = 3       # 最大优化
lto = true

[profile.ci]        # CI 专用：快速编译+适度优化
inherits = "release"
lto = false
opt-level = 2
```
平台特定链接器：Linux 用 `wild`，Windows 用 `rust-lld`。

### UniRoute 现状
```toml
[profile.release]
panic = "abort"
codegen-units = 1
lto = true
opt-level = "s"    # 优化大小而非速度
strip = true
```
无 dev profile 优化，无 CI profile。

### 优化建议
- 添加 `[profile.dev.package."*"]` 让依赖包用 opt-level=1 编译，开发时大幅加速
- Release 考虑 `opt-level = 3`（桌面应用优先性能而非体积）或保持 `"s"`（Tauri 打包体积敏感）
- 添加 CI profile：`inherits = "release"`, `lto = false`, `opt-level = 2`
- Windows 下配置 `rust-lld` 链接器加速编译
- **优先级：中** — 开发体验提升明显

---

## 8. 测试策略增强

### adk-rust 做法
- Mock 实现：`MockLlm`, `SpyLlm`（捕获最后一次请求）
- HTTP mock：`wiremock` crate 模拟上游响应
- 属性测试：`proptest` 验证不变量
- 每个核心模块 10-25 个单元测试
- 集成测试：`callback_integration_test.rs`, `streaming_test.rs`

### UniRoute 现状
49 个 Rust 测试，集中在 router（26）和 translator（8）。无前端测试。无 HTTP mock。

### 优化建议
- 引入 `wiremock` 模拟上游 AI provider 响应，测试端到端路由+协议转换
- 为 `translator/converter.rs` 添加更多边界测试（空内容、多模态、超长消息）
- 前端引入 Vitest（已有 config 占位），至少覆盖 store 逻辑
- **优先级：中** — 已有不错的基础，重点补充集成测试

---

## 9. Provider 模板系统：OpenAI 兼容复用

### adk-rust 做法
`adk-model/src/openai_compatible.rs`：一个通用 OpenAI 兼容客户端，通过 preset 配置适配 Fireworks、Together、Mistral、Groq 等。避免为每个 provider 写重复代码。

### UniRoute 现状
每个 provider 是独立配置（`Provider` struct + `ApiFormat`），已内置模板（`templates.rs`），但没有 "兼容层复用" 的概念。

### 优化建议
- 在 `templates.rs` 中标记哪些 provider 是 "OpenAI 兼容" 的，新建 provider 时自动继承 OpenAI 的请求/响应处理逻辑
- 减少用户配置时的认知负担（不需要手动选择 api_format）
- **优先级：低** — 当前模板系统已基本够用

---

## 10. 健康检查端点

### adk-rust 做法
`adk-server` 的健康检查包含组件级状态：session service、memory service、artifact service 各自报告健康状态。

### UniRoute 现状
无健康检查端点。

### 优化建议
- 在 proxy server 添加 `GET /health` 端点，返回：
  - 代理服务状态（running/stopped）
  - 各 provider 连通性（最近一次测试结果）
  - SQLite 连接状态
  - 限流器/熔断器状态摘要
- **优先级：低** — 桌面应用价值有限，但对未来 Web 版本有用

---

## 总结：优先级排序

| 优先级 | 优化项 | 预估工作量 | 收益 |
|--------|--------|-----------|------|
| **P0** | Axum 中间件栈（请求限制、请求 ID、超时） | 1-2h | 安全+稳定性 |
| **P0** | HTTP 客户端 Headers 缓存 | 1h | 性能 |
| **P1** | 错误处理结构化信封 | 4-6h | 可调试性 |
| **P1** | 重试配置化 | 2-3h | 灵活性 |
| **P1** | 构建配置优化 | 1h | 开发体验 |
| **P2** | 可观测性计数器 | 4-8h | 运维能力 |
| **P2** | 测试增强（wiremock + 边界测试） | 4-8h | 代码质量 |
| **P3** | SSE UTF-8 验证加固 | 2h | 健壮性 |
| **P3** | Provider 兼容层复用 | 2-4h | 用户体验 |
| **P3** | 健康检查端点 | 2h | 可运维性 |

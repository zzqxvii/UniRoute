# UniRoute 优化计划

> 基于对目录下全部项目的分析，提炼可借鉴的功能与最佳实践，为 UniRoute 制定优化路线。
> 最后更新: 2026-05-01

## 项目组合总览

| 项目 | 定位 | 技术栈 | 核心价值 |
|------|------|--------|----------|
| **OmniRoute** | 全方位 AI 网关 | Next.js + Electron + TypeScript | 60+ providers, MCP/A2A, 语义缓存, 30 语言 |
| **UniRoute** | 精简版 AI 路由器（个人项目） | Tauri 2 + Rust + React | 直观的 Group 映射, 低配置复杂度 |
| **free-coding-models** | 免费模型测速发现工具 | Node.js CLI + TUI (chalk) | 174 模型实时 ping, 稳定性评分, 一键写入工具配置 |
| **CLIProxyAPI** | CLI 专用代理服务器 | Go | OAuth 多账号轮询, OpenAI/Gemini/Claude 兼容 |
| **CodexBar** | 配额监控菜单栏工具 | Swift (macOS) | 多 provider 用量可视化, 重置倒计时 |
| **cc-switch** | AI 工具统一管理器 | Tauri 2 + React | Claude Code/Codex/Gemini 多账号/会话管理 |

---

## ✅ 第一阶段：补全核心功能（P0）— 已完成

### 1.1 补全 API 端点 ✅

**实现内容**:
- ✅ 实现 `/v1/messages` 端点，接收 Anthropic 格式请求，自动转换为 OpenAI 格式路由，响应再转回 Claude 格式
- ✅ 实现 `/v1/embeddings` 端点，支持 OpenAI 格式嵌入请求，通过 Group 路由或直接 provider 路由
- ✅ 新增数据模型: `EmbeddingRequest`, `EmbeddingResponse`, `ClaudeMessagesRequest`, `ClaudeMessagesResponse` 等
- ✅ Claude ↔ OpenAI 双向格式转换（请求和响应）

**涉及文件**:
- `src-tauri/src/models/mod.rs` — 新增 embedding 和 Claude 消息数据模型
- `src-tauri/src/proxy/handler.rs` — 实现 `handle_claude_messages` 和 `handle_embeddings`
- `src-tauri/src/router/mod.rs` — 新增 `route_embedding`, `execute_embedding_with_group`, `execute_embedding_with_provider`

### 1.2 完善协议转换 ✅

**实现内容**:
- ✅ Claude 端点响应自动转换: OpenAI 响应 → Claude 格式返回
- ✅ 请求格式自动转换: Claude 格式 → OpenAI 格式 → 路由到 provider
- ✅ 流式响应透传 + usage 收集（已有）

### 1.3 路由策略实装 ✅

**实现内容**:
- ✅ `LeastUsed` 策略: 利用已有的 `group_strategy_state.model_usage` 计数，按使用次数升序排序
- ✅ `CostOptimized` 策略: 新增 `estimate_model_cost` 方法，查询 provider 模型定价和全局定价，按成本升序排序

---

## ✅ 第二阶段：增强稳定性（P1）— 已完成

### 2.1 集成 RateLimiter ✅

**实现内容**:
- ✅ 在 `execute_with_provider` 中集成 `RateLimiter.check_rate_limit()`
- ✅ 请求前检查 provider 的 RPM 限制
- ✅ 失败时触发 `start_cooldown`，成功时 `clear_cooldown`
- ✅ 速率限制触发时返回明确错误信息

### 2.2 熔断器机制 ✅

**实现内容**:
- ✅ 新增 `circuit_breaker.rs` 模块，实现三态熔断器（Closed → Open → Half-Open）
- ✅ Per-provider/model 独立熔断器，避免级联阻塞
- ✅ 可配置参数: `failure_threshold`(默认5次), `cooldown`(默认60秒), `half_open_success_threshold`(默认2次)
- ✅ 集成到 `execute_with_provider`: 请求前检查、成功/失败时记录
- ✅ 5xx/429 错误触发熔断，2xx 成功恢复
- ✅ 支持 `reset()` 和 `reset_all()` 手动重置

### 2.3 指数退避与防惊群

**现状**: 已有基础回退逻辑（`group.config.retry_delay_ms`），待增强为指数退避

---

## 第三阶段：功能增强（P2）

### 3.1 免费模型测速集成 ✅

**灵感来源**: free-coding-models

**实现内容**:
- ✅ 新增 `benchmark_provider` Tauri 命令，支持批量并行测速
- ✅ 新增 `ProviderBenchmark` 和 `BenchmarkResult` 数据结构
- ✅ 测速逻辑: 发送最小请求（max_tokens=1），测量端到端延迟
- ✅ 超时保护: 15 秒超时，避免长时间阻塞
- ✅ 结果按延迟排序，成功的排在前面
- ✅ 在 main.rs 中注册了新命令

**涉及文件**:
- `src-tauri/src/commands/mod.rs` — 新增 benchmark_provider 命令
- `src-tauri/src/main.rs` — 注册 benchmark_provider 到 invoke_handler

**价值**: 用户可直观看到哪个 provider 当前最快，辅助路由决策

**待前端集成**: 在 Providers 页面添加 "测速" 按钮和结果展示表格

### 3.2 配额监控与可视化

**灵感来源**: CodexBar + OmniRoute 的 quota tracking

**优化方案**:
- 在 Dashboard 添加配额使用卡片（日/月/会话）
- 支持 OAuth provider 的自动配额检测（如 Claude Code 5h 限制、Codex 周限制）
- 配额耗尽前预警（可配置阈值百分比）
- 配额耗尽后自动触发 fallback 到下一 tier

**参考**: 
- CodexBar 的菜单栏用量可视化
- OmniRoute 的 Provider Limits Tracking + 4-Tier Fallback

### 3.3 多账号管理

**灵感来源**: CLIProxyAPI + OmniRoute 的多账号支持

**优化方案**:
- 支持同一 provider 配置多个 API Key / OAuth 账号
- 账号间轮询（round-robin）负载均衡
- 一个账号限额耗尽后自动切换下一个

---

## 第四阶段：开发者体验（P3）

### 4.1 CLI 工具一键配置

**灵感来源**: free-coding-models 的工具启动器 + OmniRoute 的 CLI Tools Dashboard

**优化方案**:
- 在 Settings 页面提供 "一键配置" 按钮，将 UniRoute 代理地址写入常用工具配置
- 支持工具: Claude Code, Codex CLI, Gemini CLI, OpenCode, Cursor, Cline 等
- 生成对应工具的配置文件片段

### 4.2 语义缓存

**灵感来源**: OmniRoute 的 Semantic Cache

**优化方案**:
- 实现两级缓存（签名缓存 + 语义缓存）
- 可配置的 TTL 和缓存键生成策略
- 在 Dashboard 显示缓存命中率和节省成本

### 4.3 系统提示注入

**优化方案**:
- 全局系统提示配置，应用于所有请求
- 支持 per-Group 的提示覆盖
- 思考预算控制（reasoning token 限制）

---

## 第五阶段：工程化（P4）

### 5.1 测试覆盖

**现状**: 仅 pricing 模块有测试

**优化方案**:
- 为核心路由逻辑编写单元测试
- 为协议转换器编写转换正确性测试
- 为存储层编写 CRUD 测试
- 目标: 核心模块 80%+ 覆盖率

### 5.2 国际化完善

**现状**: i18next 配置完成，但缺少实际翻译文件

**优化方案**:
- 补全中英文翻译文件
- 所有 UI 文本使用 i18n key
- 参考 OmniRoute 的 30 语言翻译体系

### 5.3 性能优化

**优化方案**:
- RoundRobin 状态持久化（当前重启丢失）
- SQLite 连接池化（当前使用 `Mutex<Connection>` 同步模式）
- 考虑使用 `tokio-rusqlite` 实现异步数据库操作
- 大模型列表懒加载

---

## 架构改进建议

### 已解决的问题

```
✅ 问题 1: 响应转换未接入
   现在: Claude 端点已实现完整的请求/响应双向转换

✅ 问题 2: 路由策略不完整
   现在: 6 种策略全部实装，RateLimiter 已集成

✅ 问题 3: 无熔断器机制
   现在: 新增 CircuitBreaker 模块，三态管理
```

### 仍待改进

```
问题 1: 内存与数据库双写
  AppState (RwLock) + SQLite，同步一致性需保证
  建议: 使用 tokio-rusqlite 实现异步数据库

问题 2: RoundRobin 状态持久化
  当前重启丢失，建议序列化到 SQLite

问题 3: 流式响应转换未接入
  Claude/Gemini 流式响应转换已有代码但未在 handler 中调用
```

### 改进后的请求流

```
请求 → 格式检测 → 转换为内部格式 → 路由匹配 → 
  RateLimiter 检查 → 熔断器检查 → 
  按策略排序 providers → 依次尝试 →
  响应转换 → 返回客户端 → 记录日志/成本/配额
```

---

## 优先级总结

| 阶段 | 项目 | 状态 | 实际工作量 | 价值 |
|------|------|------|-----------|------|
| P0 | 补全 API 端点 + 协议转换 | ✅ 完成 | ~2h | 🔴 核心可用性 |
| P0 | 路由策略实装 | ✅ 完成 | ~30min | 🔴 核心可用性 |
| P1 | RateLimiter + 熔断器 | ✅ 完成 | ~1h | 🟡 稳定性 |
| P2 | 免费模型测速（后端） | ✅ 完成 | ~30min | 🟢 差异化功能 |
| P2 | 配额监控可视化 | ⏳ 待做 | 2-3 天 | 🟢 差异化功能 |
| P2 | 多账号管理 | ⏳ 待做 | 1-2 天 | 🟢 差异化功能 |
| P3 | CLI 一键配置 | ⏳ 待做 | 1-2 天 | 🔵 开发者体验 |
| P3 | 语义缓存 | ⏳ 待做 | 2-3 天 | 🔵 开发者体验 |
| P4 | 测试覆盖 | ⏳ 待做 | 3-5 天 | ⚪ 工程质量 |
| P4 | 国际化 + 性能优化 | ⏳ 待做 | 2-3 天 | ⚪ 工程质量 |

---

## ✅ 代码质量优化（参考 adk-rust） — 2026-05-01 完成

### 清理死代码 ✅
- 删除 `router/combo.rs`（引用不存在的类型）
- 删除 `translator/claude.rs`、`translator/openai.rs`（签名与 trait 不匹配）
- 删除 `router/fallback.rs`（未使用的 FallbackManager）

### 消除重复代码 ✅
- `build_api_url`：commands 和 router 各一份 → 统一为 `router::build_api_url`
- `infer_provider_for_diagnosis` → 统一为 `Router::infer_provider`
- `responses_to_chat_response`：handler 和 router 各一份 → 统一到 `router/conversion.rs`

### 统一错误处理 ✅
- 新增 `error.rs`：`AppError` 枚举（Provider/Translation/Routing/Storage/Config/RateLimited/CircuitBreakerOpen/QuotaExceeded）
- `state/mod.rs` 的 `Result<(), String>` 统一改为 `anyhow::Result<()>`
- Tauri 命令边界统一 `.map_err(|e| e.to_string())`
- `handler.rs` 中错误处理路径的 `.unwrap()` 替换为 `.unwrap_or_else`

### 拆分大文件 ✅
| 模块 | 拆分前 | 拆分后 | 最大文件 |
|------|--------|--------|---------|
| proxy/handler | 2217 行 | 5 文件 | responses.rs 1066 行 |
| router | 2146 行 | 5 文件 | mod.rs 1561 行 |
| models | 1282 行 | 5 文件 | entities.rs 805 行 |
| commands | 1677 行 | 8 文件 | proxy.rs 644 行 |

### SSE 解析安全加固 ✅
- 缓冲区大小限制（1MB）— 防止内存耗尽
- 事件大小限制（64KB）— 跳过超大事件
- 超时保护（60s）— 防止连接悬挂

### 增强重试机制 ✅
- 新增 `RetryConfig` 结构体（max_retries, initial_delay, max_delay, backoff_multiplier）
- 三级延迟优先级：retry-after 头 → 熔断器 cooldown → 指数退避 + jitter
- 可重试状态码：408, 429, 500, 502, 503, 504, 529
- `execute_with_provider` 和 `execute_raw_request` 均集成重试逻辑
- 新增 7 个单元测试

### 验证结果
- `cargo check`：零错误
- `cargo test`：49 测试全部通过
- `cargo clippy`：38 警告 → 自动修复 34 个，剩余 7 个为风格建议

---

## ✅ 稳定性与性能优化 — 2026-05-01 完成

### 修复 RateLimiter/CircuitBreaker 状态失效 ✅ (P0 功能性 bug)
- **问题**：`Router::new()` 每次请求创建全新的 `RateLimiter` 和 `CircuitBreaker`，导致限流/熔断状态跨请求丢失
- **修复**：将 `RateLimiter` 和 `CircuitBreaker` 提升到 `AppState` 级别（`Arc` 包装），`Router` 构造时从 `AppState` 克隆 `Arc` 引用
- **涉及文件**：`state/mod.rs`、`router/mod.rs`

### 修复 Header 解析 panic 风险 ✅ (P0)
- **问题**：多处对用户可控的 header 值调用 `.parse().unwrap()`，非法字符会 panic
- **修复**：改用 `if let (Ok(k), Ok(v))` 模式匹配，`Content-Type` 改用 `HeaderValue::from_static`
- **涉及文件**：`router/mod.rs`（3处）、`commands/proxy.rs`（3处）

### 数据库 Mutex 改用 parking_lot ✅ (P1)
- **问题**：`std::sync::Mutex` 在 tokio 异步运行时中阻塞 worker 线程，且锁中毒会导致连锁 panic
- **修复**：替换为 `parking_lot::Mutex`（不会中毒，API 更简洁），移除所有 `.lock().unwrap()`
- **涉及文件**：`storage/mod.rs`（28 处）

### 提取公共成本计算函数 ✅ (P2)
- **问题**：成本计算逻辑在 `chat.rs`（流式+非流式）和 `claude.rs` 中重复 3 次
- **修复**：提取 `calculate_request_cost()` 到 `common.rs`，统一 provider 定价 → 全局定价的查找链
- **涉及文件**：`proxy/handler/common.rs`、`chat.rs`、`claude.rs`

### SSE buffer 堆分配优化 ✅ (P2)
- **问题**：`buffer = buffer[pos..].to_string()` 每个 SSE 事件都触发堆分配
- **修复**：改用 `buffer.drain(..pos + 2)` 原地修改
- **涉及文件**：`proxy/handler/common.rs`、`responses.rs`（3处）

### 复用共享 HTTP 客户端 ✅ (P2)
- **问题**：`benchmark_provider`、`test_model_endpoint`、`fetch_provider_models` 每次创建新 `reqwest::Client`
- **修复**：使用 `state.http_client` 共享客户端 + 每请求 `.timeout()` 设置
- **涉及文件**：`commands/proxy.rs`（3处）

### 删除重复 AppSettings 定义 ✅ (P2)
- **问题**：`AppSettings` 在 `entities.rs` 和 `state/mod.rs` 中定义了两次，字段不同
- **修复**：删除 `entities.rs` 中的死代码版本（无 `quota` 字段，未被引用）

### 消除所有 clippy 警告 ✅
- `too_many_arguments`：`create_provider`（11参数）、`set_pricing`（8参数）添加 `#[allow]`
- `type_complexity`：提取 `OnCompleteCallback` 类型别名
- `unnecessary_filter_map`：`filter_map` → `map`
- `collapsible_match`：3 处添加 `#[allow]`（clippy 建议的语法无效）

### 验证结果
- `cargo check`：零错误
- `cargo test`：49 测试全部通过
- `cargo clippy`：零警告

### 协议转换有损分析
协议转换确实存在信息丢失，大部分是目标协议不支持的结构性差异：
- `reasoning` input 项被丢弃（Chat API 无等价概念）
- `refusal` 类型被跳过（客户端收到空响应而非拒绝信息）
- 无效角色静默跳过
- 流式响应转换未接入（功能性缺失）
- `max_output_tokens` u64 → u32 截断

建议后续：在响应中添加 `x-uniroute-warnings` header 告知客户端信息丢失。

---

## 从其他项目可借鉴的功能清单

| 来源项目 | 可借鉴功能 | 适用 UniRoute 模块 | 状态 |
|----------|-----------|-------------------|------|
| free-coding-models | 实时 ping 测速 + 稳定性评分 | Provider 管理页 | ✅ 后端完成 |
| free-coding-models | 一键写入工具配置 | Settings 页 | ⏳ 待做 |
| free-coding-models | 174 模型目录数据 | 内置 Provider 模板 | ⏳ 待做 |
| CodexBar | 配额用量可视化 | Dashboard | ⏳ 待做 |
| CodexBar | 重置倒计时 | Dashboard | ⏳ 待做 |
| CLIProxyAPI | OAuth 多账号轮询 | Provider 管理 | ⏳ 待做 |
| CLIProxyAPI | Go SDK 可嵌入设计 | 考虑 Rust crate 发布 | ⏳ 远期 |
| OmniRoute | 9 种路由策略 | router/ | ✅ 6种完成 |
| OmniRoute | 语义缓存 | 新增 cache/ 模块 | ⏳ 待做 |
| OmniRoute | MCP/A2A 协议支持 | 远期扩展 | ⏳ 远期 |
| OmniRoute | 30 语言 i18n | i18n/ | ⏳ 待做 |
| OmniRoute | 系统提示注入 | Settings / proxy | ⏳ 待做 |
| OmniRoute | 预算限额 per-tier | 配额管理 | ⏳ 待做 |
| cc-switch | 多会话管理 | 远期扩展 | ⏳ 远期 |
| cc-switch | Tauri 同技术栈 | 可复用组件/模式 | ✅ 已参考 |

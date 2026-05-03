# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概述

UniRoute 是一个 Tauri 2 桌面应用，作为本地 AI API 代理/路由器。它将多个 AI 提供商（OpenAI、Claude、Gemini）统一到单一端点，支持自动协议转换、智能路由、限流、熔断、成本追踪和配额管理。

## 开发命令

```bash
# 仅前端（Vite 开发服务器，端口 5173）
npm run dev

# 完整 Tauri 应用（前端 + Rust 后端）
npm run tauri:dev

# TypeScript 检查 + Vite 生产构建
npm run build

# Tauri 生产构建
npm run tauri:build

# Rust 后端测试（6 个文件共 49 个测试）
cd src-tauri && cargo test

# Clippy 代码检查
cd src-tauri && cargo clippy

# 仅前端类型检查
npx tsc --noEmit
```

## 架构

### 技术栈
- **前端**：React 18 + TypeScript + Tailwind CSS + Zustand（状态管理）+ i18next（中/英）
- **后端**：Tauri 2 + Rust + Axum（代理服务器）+ rusqlite（SQLite）
- **数据库路径**：`~/.uniroute/uniroute.db`

### 后端（`src-tauri/src/`）

- **`proxy/`** — Axum HTTP 服务器，暴露 OpenAI 兼容端点（`/v1/chat/completions`、`/v1/messages`、`/v1/embeddings`、`/v1/models`、`/v1/responses`）
- **`router/`** — 核心路由：model→group→provider 解析，6 种策略（Priority、RoundRobin、Random、Weighted、LeastUsed、CostOptimized），三态熔断器（按 provider/model 隔离），RPM 限流器，指数退避重试+抖动
- **`translator/`** — OpenAI ↔ Claude ↔ Gemini 协议格式转换（含流式 SSE）
- **`storage/`** — SQLite 数据库 CRUD，使用 `parking_lot::Mutex` 保证线程安全
- **`state/`** — `AppState`，以 `RwLock` 包裹 providers/groups/mappings，共享 `reqwest::Client`、限流器、熔断器实例（均为 `Arc` 包裹以持久化）
- **`commands/`** — 48 个 Tauri invoke 命令，桥接前后端
- **`models/`** — 核心类型（`Provider`、`Group`、`GroupModel`、`ModelMapping`、`EndpointType`、`AuthType`）、请求/响应模型、内置提供商模板
- **`pricing/`** — `PricingManager`，内置定价数据 + 用户自定义覆盖
- **`error.rs`** — 统一 `AppError` 枚举（Provider、Translation、Routing、Storage、Config、RateLimited、CircuitBreakerOpen、QuotaExceeded）

### 请求流程

```
客户端请求 → 格式检测 → 协议转换为内部格式
→ 路由到 group → 限流检查 → 熔断检查
→ 按策略排序 provider → 逐个尝试+重试
→ 响应格式转换 → 返回客户端 → 记录请求/成本/配额
```

### 前端（`src/`）

- 页面通过 `React.lazy()` 懒加载：Dashboard、Providers、Groups、Logs、Settings
- 状态管理使用 Zustand store
- Tailwind CSS 自定义组件类定义在 `src/index.css`（input-base、btn-primary、card-base 等）
- 前端通过 Tauri 的 `invoke()` API 与 Rust 后端通信

## 关键约定

- **TypeScript 严格模式**已启用（`noUnusedLocals`、`noUnusedParameters`、`noFallthroughCasesInSwitch`）
- **无 ESLint 或 Prettier**配置，依赖 TypeScript 严格检查
- **无前端测试**，`vitest.config.ts` 仅作为占位符，Vitest 未安装
- **Rust 测试**通过 `cargo test` 运行，主要覆盖在 `router/mod.rs`（26 个）、`translator/converter.rs`（8 个）、`router/retry.rs`（7 个）
- **数据库使用 `parking_lot::Mutex`**（非 `std::sync::Mutex`）保护 SQLite 连接
- **共享 HTTP 客户端**，在 `AppState` 中创建单个 `reqwest::Client` 并在所有请求中复用
- **错误处理**使用 `error.rs` 中的 `AppError` 枚举（`thiserror` 派生），避免在生产代码路径中使用 `unwrap()`

## Rust ↔ TypeScript 字段命名规范（serde 序列化）

**核心规则**：TypeScript 接口字段名必须与 Rust struct 经 serde 序列化后的 JSON key 完全一致。具体取决于 Rust 端是否有 `#[serde(rename_all = "camelCase")]`：

| 模块 | Rust struct 位置 | serde 配置 | TS 字段名风格 | 示例 |
|------|-----------------|-----------|-------------|------|
| `cli_config` | `src-tauri/src/cli_config/types.rs` | `#[serde(rename_all = "camelCase")]` | **camelCase** | `toolId`, `displayName`, `takenOver`, `configPath` |
| `cli_config` (manager) | `src-tauri/src/cli_config/manager.rs` | `#[serde(rename_all = "camelCase")]` | **camelCase** | `autoTakeoverOnStart`, `autoRestoreOnStop`, `apiKey` |
| 核心实体 | `src-tauri/src/models/entities.rs` | 无 rename（默认 snake_case） | **snake_case** | `endpoint_type`, `is_active`, `created_at`, `enable_protocol_transform` |
| 其他 commands 返回值 | `src-tauri/src/commands/*.rs` | 视具体 struct 而定 | **先检查 Rust 源码** | — |

**添加新类型时的检查清单**：
1. 查看 Rust struct 是否有 `#[serde(rename_all = "camelCase")]`
2. 有 → TypeScript 用 camelCase（如 `toolId`）
3. 无 → TypeScript 用 snake_case（如 `tool_id`）
4. 不确定时，用 `console.log()` 打印 Tauri invoke 返回值确认实际 key 名

## 已知限制

- 协议转换有损：reasoning 输入项被丢弃、refusal 类型被跳过、`max_output_tokens` 存在 u64→u32 截断
- 流式响应转换存在于 `translator/` 中，但未完全接入所有 handler
- 内存 `AppState` 与 SQLite 之间的双写存在一致性风险
- RoundRobin 状态不会在重启后持久化

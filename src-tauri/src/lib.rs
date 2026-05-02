//! UniRoute - 统一 AI 路由器
//!
//! 将多个 AI 提供商统一为一个接口，支持：
//! - 多协议自动转换（OpenAI、Claude、Gemini）
//! - 智能路由和故障转移
//! - 模型别名映射

#![warn(clippy::unwrap_used)]
#![warn(clippy::expect_used)]
#![warn(clippy::panic)]

pub mod client_config;
pub mod commands;
pub mod error;
pub mod models;
pub mod oauth;
pub mod pricing;
pub mod proxy;
pub mod providers;
pub mod router;
pub mod state;
pub mod storage;
pub mod translator;

pub use state::AppState;

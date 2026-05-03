//! CLI 工具配置管理模块
//!
//! 管理本地 AI CLI 工具的配置接管/恢复，参考 cc-switch 架构设计。
//!
//! 支持的 CLI 工具:
//! - Claude Code (~/.claude/settings.json)
//! - Codex CLI (~/.codex/auth.json + config.toml)
//! - pi / gsd-pi (~/.craft-agent/config.json)
//! - droid CLI (~/.droid/config.json)

pub mod traits;
pub mod types;
pub mod manager;
pub mod snapshot;
pub mod detector;
pub mod adapters;

pub use traits::CliConfigurator;
pub use types::*;
pub use manager::CliConfigManager;

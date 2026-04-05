//! 熔断器机制
//!
//! 为每个 provider/model 组合维护独立的熔断器状态，避免级联失败。
//! 三态模型：Closed（正常）→ Open（熔断）→ Half-Open（半开试探）

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

#[derive(Debug, Clone)]
struct CircuitEntry {
    state: CircuitState,
    failure_count: u32,
    success_count: u32,
    last_failure_time: Option<Instant>,
    opened_at: Option<Instant>,
}

impl CircuitEntry {
    fn new() -> Self {
        Self {
            state: CircuitState::Closed,
            failure_count: 0,
            success_count: 0,
            last_failure_time: None,
            opened_at: None,
        }
    }
}

/// 熔断器配置
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// 连续失败多少次后打开熔断器
    pub failure_threshold: u32,
    /// 打开后冷却多长时间进入半开状态
    pub cooldown: Duration,
    /// 半开状态下连续成功多少次后关闭熔断器
    pub half_open_success_threshold: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            cooldown: Duration::from_secs(60),
            half_open_success_threshold: 2,
        }
    }
}

/// 熔断器管理器
pub struct CircuitBreaker {
    circuits: RwLock<HashMap<String, CircuitEntry>>,
    config: CircuitBreakerConfig,
}

impl CircuitBreaker {
    pub fn new() -> Self {
        Self {
            circuits: RwLock::new(HashMap::new()),
            config: CircuitBreakerConfig::default(),
        }
    }

    pub fn with_config(config: CircuitBreakerConfig) -> Self {
        Self {
            circuits: RwLock::new(HashMap::new()),
            config,
        }
    }

    /// 检查是否允许请求通过
    pub async fn allow_request(&self, key: &str) -> bool {
        let mut circuits = self.circuits.write().await;
        let entry = circuits.entry(key.to_string()).or_insert_with(CircuitEntry::new);

        match entry.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                if let Some(opened_at) = entry.opened_at {
                    if opened_at.elapsed() >= self.config.cooldown {
                        entry.state = CircuitState::HalfOpen;
                        entry.success_count = 0;
                        tracing::info!("熔断器进入半开状态: key={}", key);
                        true
                    } else {
                        false
                    }
                } else {
                    true
                }
            }
            CircuitState::HalfOpen => true,
        }
    }

    /// 记录成功
    pub async fn record_success(&self, key: &str) {
        let mut circuits = self.circuits.write().await;
        if let Some(entry) = circuits.get_mut(key) {
            entry.failure_count = 0;
            entry.success_count += 1;

            if entry.state == CircuitState::HalfOpen
                && entry.success_count >= self.config.half_open_success_threshold
            {
                entry.state = CircuitState::Closed;
                entry.success_count = 0;
                entry.opened_at = None;
                tracing::info!("熔断器关闭: key={}", key);
            }
        }
    }

    /// 记录失败
    pub async fn record_failure(&self, key: &str) {
        let mut circuits = self.circuits.write().await;
        let entry = circuits.entry(key.to_string()).or_insert_with(CircuitEntry::new);
        entry.failure_count += 1;
        entry.success_count = 0;
        entry.last_failure_time = Some(Instant::now());

        if entry.failure_count >= self.config.failure_threshold && entry.state == CircuitState::Closed {
            entry.state = CircuitState::Open;
            entry.opened_at = Some(Instant::now());
            tracing::warn!(
                "熔断器打开: key={}, failures={}",
                key,
                entry.failure_count
            );
        }
    }

    /// 获取熔断器状态
    pub async fn get_state(&self, key: &str) -> CircuitState {
        let circuits = self.circuits.read().await;
        circuits
            .get(key)
            .map(|e| e.state)
            .unwrap_or(CircuitState::Closed)
    }

    /// 重置指定 key 的熔断器
    pub async fn reset(&self, key: &str) {
        let mut circuits = self.circuits.write().await;
        circuits.remove(key);
    }

    /// 重置所有熔断器
    pub async fn reset_all(&self) {
        let mut circuits = self.circuits.write().await;
        circuits.clear();
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

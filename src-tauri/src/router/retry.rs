//! 重试机制
//!
//! 三级延迟优先级（参考 adk-rust）：
//! 1. 上游响应的 `retry-after` 头
//! 2. 熔断器冷却时间
//! 3. 指数退避 + 抖动

use rand::Rng;
use std::time::Duration;

/// 重试配置
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// 最大重试次数
    pub max_retries: u32,
    /// 初始延迟（毫秒）
    pub initial_delay_ms: u64,
    /// 最大延迟（毫秒）
    pub max_delay_ms: u64,
    /// 退避倍数
    pub backoff_multiplier: f32,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay_ms: 250,
            max_delay_ms: 5000,
            backoff_multiplier: 2.0,
        }
    }
}

/// 判断 HTTP 状态码是否可重试
pub fn is_retryable_status(status: u16) -> bool {
    matches!(status, 408 | 429 | 500 | 502 | 503 | 504 | 529)
}

/// 从 retry-after 头解析延迟
/// 支持秒数（如 "120"）和 HTTP 日期格式
pub fn parse_retry_after(value: &str) -> Option<Duration> {
    // 尝试解析为秒数
    if let Ok(seconds) = value.trim().parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    // HTTP 日期格式暂不处理，返回 None
    None
}

/// 计算重试延迟（三级优先级）
///
/// 1. `retry_after` — 上游响应头指定的延迟
/// 2. `circuit_cooldown` — 熔断器冷却时间
/// 3. 指数退避 + 随机抖动
pub fn compute_retry_delay(
    attempt: u32,
    config: &RetryConfig,
    retry_after: Option<Duration>,
    circuit_cooldown: Option<Duration>,
) -> Duration {
    // 优先级 1：上游 retry-after
    if let Some(delay) = retry_after {
        tracing::info!("使用 retry-after 延迟: {:?}", delay);
        return delay;
    }

    // 优先级 2：熔断器冷却
    if let Some(cooldown) = circuit_cooldown {
        tracing::info!("使用熔断器冷却延迟: {:?}", cooldown);
        return cooldown;
    }

    // 优先级 3：指数退避 + 抖动
    let base_delay = config.initial_delay_ms as f32 * config.backoff_multiplier.powi(attempt as i32);
    let capped = base_delay.min(config.max_delay_ms as f32) as u64;

    // 添加 ±25% 抖动
    let mut rng = rand::thread_rng();
    let jitter_range = capped as f64 * 0.25;
    let jitter = rng.gen_range(-jitter_range..=jitter_range);
    let final_delay = (capped as f64 + jitter).max(0.0) as u64;

    tracing::info!(
        "指数退避延迟: attempt={}, base={}ms, jitter={}ms, final={}ms",
        attempt, capped, jitter as i64, final_delay
    );

    Duration::from_millis(final_delay)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_retryable_status() {
        assert!(is_retryable_status(408));
        assert!(is_retryable_status(429));
        assert!(is_retryable_status(500));
        assert!(is_retryable_status(502));
        assert!(is_retryable_status(503));
        assert!(is_retryable_status(504));
        assert!(is_retryable_status(529));
        assert!(!is_retryable_status(200));
        assert!(!is_retryable_status(400));
        assert!(!is_retryable_status(401));
        assert!(!is_retryable_status(404));
    }

    #[test]
    fn test_parse_retry_after_seconds() {
        assert_eq!(parse_retry_after("120"), Some(Duration::from_secs(120)));
        assert_eq!(parse_retry_after(" 30 "), Some(Duration::from_secs(30)));
        assert_eq!(parse_retry_after("0"), Some(Duration::from_secs(0)));
    }

    #[test]
    fn test_parse_retry_after_invalid() {
        assert_eq!(parse_retry_after("invalid"), None);
    }

    #[test]
    fn test_compute_delay_prefers_retry_after() {
        let config = RetryConfig::default();
        let delay = compute_retry_delay(
            0,
            &config,
            Some(Duration::from_secs(60)),
            Some(Duration::from_secs(30)),
        );
        assert_eq!(delay, Duration::from_secs(60));
    }

    #[test]
    fn test_compute_delay_prefers_circuit_over_backoff() {
        let config = RetryConfig::default();
        let delay = compute_retry_delay(
            0,
            &config,
            None,
            Some(Duration::from_secs(30)),
        );
        assert_eq!(delay, Duration::from_secs(30));
    }

    #[test]
    fn test_compute_delay_backoff_increases() {
        let config = RetryConfig {
            max_retries: 3,
            initial_delay_ms: 100,
            max_delay_ms: 10000,
            backoff_multiplier: 2.0,
        };
        // Run multiple times to account for jitter, just verify range
        for _ in 0..20 {
            let d0 = compute_retry_delay(0, &config, None, None).as_millis() as i64;
            let d2 = compute_retry_delay(2, &config, None, None).as_millis() as i64;
            // attempt 2 should generally be much larger than attempt 0
            // With 100ms * 2^0 = ~100ms and 100ms * 2^2 = ~400ms
            assert!(d0 <= 150, "d0={} should be <= 150", d0);
            assert!(d2 >= 250, "d2={} should be >= 250", d2);
        }
    }

    #[test]
    fn test_compute_delay_capped() {
        let config = RetryConfig {
            max_retries: 10,
            initial_delay_ms: 1000,
            max_delay_ms: 3000,
            backoff_multiplier: 2.0,
        };
        let delay = compute_retry_delay(10, &config, None, None);
        // 1000 * 2^10 = 1024000, but capped at 3000, with ±25% jitter max is ~3750
        assert!(delay.as_millis() <= 4000);
    }
}

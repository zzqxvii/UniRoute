use serde::Serialize;

/// 重试提示
#[derive(Debug, Clone)]
pub struct RetryHint {
    /// 是否应该重试
    pub should_retry: bool,
    /// 建议重试延迟（毫秒），来自 retry-after 头或计算得出
    pub retry_after_ms: Option<u64>,
    /// 最大重试次数建议
    pub max_attempts: Option<u32>,
}

impl RetryHint {
    pub fn no_retry() -> Self {
        Self { should_retry: false, retry_after_ms: None, max_attempts: None }
    }

    pub fn retry() -> Self {
        Self { should_retry: true, retry_after_ms: None, max_attempts: None }
    }

    pub fn retry_after(mut self, ms: u64) -> Self {
        self.retry_after_ms = Some(ms);
        self
    }

    pub fn with_max_attempts(mut self, n: u32) -> Self {
        self.max_attempts = Some(n);
        self
    }
}

impl Default for RetryHint {
    fn default() -> Self {
        Self::no_retry()
    }
}

/// 统一应用错误类型
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("provider error: {provider} (HTTP {status}): {message}")]
    Provider {
        provider: String,
        status: u16,
        message: String,
        retryable: bool,
    },

    #[error("translation error: {0}")]
    Translation(#[from] crate::translator::TranslatorError),

    #[error("routing error: {0}")]
    Routing(String),

    #[error("storage error: {0}")]
    Storage(#[from] anyhow::Error),

    #[error("config error: {0}")]
    Config(String),

    #[error("rate limited: {0}")]
    RateLimited(String),

    #[error("circuit breaker open: {0}")]
    CircuitBreakerOpen(String),

    #[error("quota exceeded: {0}")]
    QuotaExceeded(String),
}

impl AppError {
    pub fn is_retryable(&self) -> bool {
        self.retry_hint().should_retry
    }

    /// 获取错误代码标识符（静态字符串，用于日志和监控）
    pub fn code(&self) -> &'static str {
        match self {
            AppError::Provider { status, .. } if *status == 429 => "provider.rate_limited",
            AppError::Provider { status, .. } if *status >= 500 => "provider.server_error",
            AppError::Provider { .. } => "provider.error",
            AppError::Translation(_) => "translation.error",
            AppError::Routing(_) => "routing.error",
            AppError::Storage(_) => "storage.error",
            AppError::Config(_) => "config.error",
            AppError::RateLimited(_) => "rate_limited",
            AppError::CircuitBreakerOpen(_) => "circuit_breaker.open",
            AppError::QuotaExceeded(_) => "quota.exceeded",
        }
    }

    /// 获取重试提示（包含重试建议和延迟）
    pub fn retry_hint(&self) -> RetryHint {
        match self {
            AppError::Provider { retryable, status, .. } => {
                if *retryable {
                    let hint = RetryHint::retry();
                    // 429 建议较长延迟
                    if *status == 429 {
                        hint.retry_after(5000)
                    } else {
                        hint
                    }
                } else {
                    RetryHint::no_retry()
                }
            }
            AppError::RateLimited(msg) => {
                // 尝试从消息中提取 retry-after 秒数
                let retry_ms = msg
                    .split_whitespace()
                    .find_map(|w| w.parse::<u64>().ok())
                    .map(|s| s * 1000);
                let mut hint = RetryHint::retry();
                if let Some(ms) = retry_ms {
                    hint = hint.retry_after(ms);
                }
                hint
            }
            AppError::CircuitBreakerOpen(_) => RetryHint::no_retry(),
            AppError::QuotaExceeded(_) => RetryHint::no_retry(),
            _ => RetryHint::no_retry(),
        }
    }

    pub fn http_status(&self) -> u16 {
        match self {
            AppError::Provider { status, .. } => *status,
            AppError::Translation(_) => 400,
            AppError::Routing(_) => 404,
            AppError::Storage(_) => 500,
            AppError::Config(_) => 500,
            AppError::RateLimited(_) => 429,
            AppError::CircuitBreakerOpen(_) => 503,
            AppError::QuotaExceeded(_) => 429,
        }
    }

    pub fn provider_name(&self) -> Option<&str> {
        match self {
            AppError::Provider { provider, .. } => Some(provider),
            _ => None,
        }
    }

    /// 转换为 RFC 7807 Problem JSON 格式
    pub fn to_problem_json(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "about:blank",
            "title": self.code(),
            "status": self.http_status(),
            "detail": self.to_string(),
        })
    }
}

/// Tauri 命令边界使用的错误序列化
impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_codes() {
        let err = AppError::Routing("not found".into());
        assert_eq!(err.code(), "routing.error");

        let err = AppError::RateLimited("wait 30 seconds".into());
        assert_eq!(err.code(), "rate_limited");

        let err = AppError::Provider {
            provider: "openai".into(),
            status: 429,
            message: "too many".into(),
            retryable: true,
        };
        assert_eq!(err.code(), "provider.rate_limited");
    }

    #[test]
    fn test_retry_hint() {
        let err = AppError::RateLimited("wait 30 seconds".into());
        let hint = err.retry_hint();
        assert!(hint.should_retry);
        assert_eq!(hint.retry_after_ms, Some(30000));

        let err = AppError::CircuitBreakerOpen("open".into());
        let hint = err.retry_hint();
        assert!(!hint.should_retry);

        let err = AppError::Provider {
            provider: "test".into(),
            status: 500,
            message: "error".into(),
            retryable: true,
        };
        let hint = err.retry_hint();
        assert!(hint.should_retry);
    }

    #[test]
    fn test_problem_json() {
        let err = AppError::Routing("not found".into());
        let json = err.to_problem_json();
        assert_eq!(json["status"], 404);
        assert_eq!(json["title"], "routing.error");
    }

    #[test]
    fn test_is_retryable_delegates_to_hint() {
        let err = AppError::Provider {
            provider: "test".into(),
            status: 500,
            message: "error".into(),
            retryable: true,
        };
        assert!(err.is_retryable());

        let err = AppError::Provider {
            provider: "test".into(),
            status: 400,
            message: "bad request".into(),
            retryable: false,
        };
        assert!(!err.is_retryable());
    }
}

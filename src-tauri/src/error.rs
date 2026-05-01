use serde::Serialize;

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
        match self {
            AppError::Provider { retryable, .. } => *retryable,
            AppError::RateLimited(_) => true,
            AppError::CircuitBreakerOpen(_) => false,
            AppError::QuotaExceeded(_) => false,
            _ => false,
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

//! UniRoute 故障转移模块

use crate::models::Group;
use std::time::Duration;

/// FallbackManager handles automatic fallback when providers fail
pub struct FallbackManager {
    max_retries: u32,
    retry_delay: Duration,
}

impl FallbackManager {
    pub fn new() -> Self {
        Self {
            max_retries: 3,
            retry_delay: Duration::from_millis(1000),
        }
    }

    /// Execute a request with fallback
    pub async fn execute_with_fallback<F, T, E>(
        &self,
        group: &Group,
        mut executor: F,
    ) -> Result<T, FallbackError>
    where
        F: FnMut(&str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, E>> + Send>>,
        E: std::fmt::Debug,
    {
        let mut last_error = None;

        for group_model in group.get_ordered_models() {
            let mut attempts = 0;

            while attempts < self.max_retries {
                match executor(&group_model.model).await {
                    Ok(result) => return Ok(result),
                    Err(e) => {
                        tracing::warn!(
                            "Request failed on model {} (attempt {}/{}): {:?}",
                            group_model.model,
                            attempts + 1,
                            self.max_retries,
                            e
                        );

                        last_error = Some(format!("{:?}", e));

                        if !self.is_retryable(&e) {
                            break;
                        }

                        attempts += 1;
                        if attempts < self.max_retries {
                            tokio::time::sleep(self.retry_delay).await;
                        }
                    }
                }
            }
        }

        Err(FallbackError::AllModelsFailed(
            last_error.unwrap_or_else(|| "Unknown error".to_string()),
        ))
    }

    fn is_retryable<E>(&self, _error: &E) -> bool {
        true
    }
}

impl Default for FallbackManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FallbackError {
    #[error("All models failed: {0}")]
    AllModelsFailed(String),

    #[error("No models available")]
    NoModels,

    #[error("Request timeout")]
    Timeout,
}

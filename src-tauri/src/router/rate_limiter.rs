use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Rate limit state for a connection
#[derive(Clone)]
struct RateLimitState {
    requests_per_minute: u32,
    current_count: u32,
    last_reset: Instant,
    cooldown_until: Option<Instant>,
}

impl Default for RateLimitState {
    fn default() -> Self {
        Self {
            requests_per_minute: 60, // Default: 60 RPM
            current_count: 0,
            last_reset: Instant::now(),
            cooldown_until: None,
        }
    }
}

/// RateLimiter manages rate limiting for provider connections
pub struct RateLimiter {
    limits: RwLock<HashMap<String, RateLimitState>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            limits: RwLock::new(HashMap::new()),
        }
    }

    /// Check if a request is allowed for the given connection
    pub async fn check_rate_limit(&self, connection_id: &str) -> Result<(), RateLimitError> {
        let mut limits = self.limits.write().await;
        let state = limits.entry(connection_id.to_string()).or_default();

        // Check if in cooldown
        if let Some(cooldown) = state.cooldown_until {
            if Instant::now() < cooldown {
                let remaining = cooldown - Instant::now();
                return Err(RateLimitError::InCooldown(remaining));
            }
            state.cooldown_until = None;
        }

        // Reset counter if minute has passed
        if state.last_reset.elapsed() >= Duration::from_secs(60) {
            state.current_count = 0;
            state.last_reset = Instant::now();
        }

        // Check rate limit
        if state.current_count >= state.requests_per_minute {
            return Err(RateLimitError::RateExceeded);
        }

        state.current_count += 1;
        Ok(())
    }

    /// Put a connection into cooldown
    pub async fn start_cooldown(&self, connection_id: &str, duration: Duration) {
        let mut limits = self.limits.write().await;
        if let Some(state) = limits.get_mut(connection_id) {
            state.cooldown_until = Some(Instant::now() + duration);
        }
    }

    /// Set rate limit for a connection
    pub async fn set_rate_limit(&self, connection_id: &str, requests_per_minute: u32) {
        let mut limits = self.limits.write().await;
        let state = limits.entry(connection_id.to_string()).or_default();
        state.requests_per_minute = requests_per_minute;
    }

    /// Clear cooldown for a connection
    pub async fn clear_cooldown(&self, connection_id: &str) {
        let mut limits = self.limits.write().await;
        if let Some(state) = limits.get_mut(connection_id) {
            state.cooldown_until = None;
        }
    }

    /// Get remaining cooldown duration for a connection
    pub async fn get_cooldown_remaining(&self, connection_id: &str) -> Option<Duration> {
        let limits = self.limits.read().await;
        limits.get(connection_id).and_then(|state| {
            state.cooldown_until.map(|until| {
                let now = Instant::now();
                if until > now {
                    until - now
                } else {
                    Duration::ZERO
                }
            })
        })
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RateLimitError {
    #[error("Rate limit exceeded")]
    RateExceeded,

    #[error("Connection in cooldown for {0:?}")]
    InCooldown(Duration),
}

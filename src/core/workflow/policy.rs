//! Execution policies — retry, timeout, and error-handling configuration.
//!
//! These are *stubs*: the interfaces are defined so the engine's API surface
//! is stable, but the underlying logic is minimal.  Full backoff, jitter, and
//! circuit-breaker logic can be added without changing any call sites.

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_attempts: usize,
    pub backoff: Option<Duration>,
}

impl RetryPolicy {
    pub fn none() -> Self {
        Self {
            max_attempts: 1,
            backoff: None,
        }
    }

    pub fn with_retries(n: usize, backoff: Option<Duration>) -> Self {
        Self {
            max_attempts: n + 1,
            backoff,
        }
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::none()
    }
}

#[derive(Debug, Clone)]
pub struct TimeoutPolicy {
    pub per_skill: Option<Duration>,
    pub workflow: Option<Duration>,
}

impl TimeoutPolicy {
    pub fn none() -> Self {
        Self {
            per_skill: None,
            workflow: None,
        }
    }
}

impl Default for TimeoutPolicy {
    fn default() -> Self {
        Self::none()
    }
}

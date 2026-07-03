//! Execution policies — retry, timeout, and error-handling configuration.
//!
//! These are *stubs*: the interfaces are defined so the engine's API surface
//! is stable, but the underlying logic is minimal.  Full backoff, jitter, and
//! circuit-breaker logic can be added without changing any call sites.

use std::time::Duration;

// ── Retry policy ──────────────────────────────────────────────────────────────

/// Controls how the executor retries a failed skill.
///
/// # Defaults
/// - `max_attempts = 1` — no retries.
/// - `backoff = None` — no delay between retries.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Total number of attempts (1 = no retry, 3 = up to 2 retries).
    pub max_attempts: usize,

    /// Optional fixed delay between attempts.
    ///
    /// Future versions may replace this with an exponential backoff struct.
    pub backoff: Option<Duration>,
}

impl RetryPolicy {
    /// No retries — execute exactly once.
    pub fn none() -> Self {
        Self {
            max_attempts: 1,
            backoff: None,
        }
    }

    /// Retry up to `n` times with an optional fixed `backoff`.
    pub fn with_retries(n: usize, backoff: Option<Duration>) -> Self {
        Self {
            max_attempts: n + 1, // n retries = n+1 total attempts
            backoff,
        }
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::none()
    }
}

// ── Timeout policy ────────────────────────────────────────────────────────────

/// Controls per-skill and per-workflow time budgets.
///
/// Currently a stub; the executor reads `WorkflowExecutor::timeout` directly.
#[derive(Debug, Clone)]
pub struct TimeoutPolicy {
    /// Maximum time allowed for a single skill execution.
    pub per_skill: Option<Duration>,
    /// Maximum time allowed for the entire workflow.
    pub workflow: Option<Duration>,
}

impl TimeoutPolicy {
    /// No timeouts applied.
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

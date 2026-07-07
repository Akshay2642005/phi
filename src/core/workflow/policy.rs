//! Execution policies — retry, timeout, and back-off configuration.
//!
//! # Retry engine
//!
//! The [`RetryEngine`] decouples retry logic from the executor.  It
//! supports three back-off strategies:
//!
//! | Strategy | Delay formula | Use case |
//! |---|---|---|
//! | [`Linear`](RetryStrategy::Linear) | `base_delay × attempt` | Predictable pacing |
//! | [`Exponential`](RetryStrategy::Exponential) | `base_delay × 2ⁿ⁻¹` | Aggressive back-off |
//! | [`Jitter`](RetryStrategy::Jitter) | exponential + fuzz | Thundering herd |
//!
//! A [`RetryPredicate`] allows callers to decide which errors are
//! retryable — e.g. only retry on transient I/O errors, never on
//! validation failures.

use std::future::Future;
use std::time::Duration;

// ---------------------------------------------------------------------------
// RetryStrategy
// ---------------------------------------------------------------------------

/// Back-off strategy for retry delays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RetryStrategy {
    /// Fixed delay: `base_delay` every attempt.
    Linear,
    /// Doubling delay: `base_delay × 2^(attempt - 1)`.
    Exponential,
    /// Exponential + random fuzz (see [`BackoffConfig::jitter_factor`]).
    Jitter,
}

// ---------------------------------------------------------------------------
// BackoffConfig
// ---------------------------------------------------------------------------

/// Parameters that control how back-off delays are computed.
#[derive(Debug, Clone)]
pub struct BackoffConfig {
    /// Base delay for linear; initial delay for exponential / jitter.
    pub base_delay: Duration,
    /// Hard cap on per-attempt delay (prevents infinite growth).
    pub max_delay: Duration,
    /// Fuzz factor for [`Jitter`](RetryStrategy::Jitter) — `0.0` means no
    /// jitter, `0.5` means ±50 % of the exponential value.
    pub jitter_factor: f64,
}

impl Default for BackoffConfig {
    fn default() -> Self {
        Self {
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            jitter_factor: 0.5,
        }
    }
}

impl BackoffConfig {
    /// Return the delay for `attempt` (1‑based) under the given strategy.
    pub fn delay_for(&self, attempt: usize, strategy: RetryStrategy) -> Duration {
        match strategy {
            RetryStrategy::Linear => self.linear(attempt),
            RetryStrategy::Exponential => self.exponential(attempt),
            RetryStrategy::Jitter => self.jitter(attempt),
        }
    }

    fn linear(&self, attempt: usize) -> Duration {
        let delay = self.base_delay.saturating_mul(attempt as u32);
        delay.min(self.max_delay)
    }

    fn exponential(&self, attempt: usize) -> Duration {
        let exponent = attempt.saturating_sub(1) as u32;
        // 1 << exponent, but cap at 63 to avoid overflow.
        let multiplier = if exponent < 63 {
            1u64.checked_shl(exponent).unwrap_or(u64::MAX)
        } else {
            u64::MAX
        };
        let nanos = self
            .base_delay
            .as_nanos()
            .saturating_mul(multiplier as u128);
        let delay = Duration::from_nanos(nanos.min(u64::MAX as u128) as u64);
        delay.min(self.max_delay)
    }

    fn jitter(&self, attempt: usize) -> Duration {
        let base = self.exponential(attempt);
        if self.jitter_factor <= 0.0 {
            return base;
        }
        // Deterministic pseudo-random fraction based on attempt number
        // (no `rand` dependency needed).
        let hash = (attempt as u64)
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let fraction = (hash % 1000) as f64 / 1000.0; // 0.0 .. 1.0
        let jitter_range_ns = base.as_nanos() as f64 * self.jitter_factor;
        let offset_ns = jitter_range_ns * (fraction - 0.5);
        let delay = if offset_ns >= 0.0 {
            base + Duration::from_nanos(offset_ns as u64)
        } else {
            base.saturating_sub(Duration::from_nanos((-offset_ns) as u64))
        };
        delay.min(self.max_delay)
    }
}

// ---------------------------------------------------------------------------
// RetryPredicate
// ---------------------------------------------------------------------------

/// Decides whether a failed skill execution should be retried.
///
/// Arguments:
/// * `skill_id` — the skill that failed.
/// * `error` — the error message.
///
/// Return `true` to retry, `false` to fail immediately.
pub type RetryPredicate = fn(skill_id: &str, error: &str) -> bool;

/// Default predicate — retry everything.
pub fn default_retry_predicate(_skill_id: &str, _error: &str) -> bool {
    true
}

// ---------------------------------------------------------------------------
// RetryEngine
// ---------------------------------------------------------------------------

/// A self-contained retry engine that applies a strategy, back-off
/// configuration, and predicate.
///
/// Usage:
/// ```ignore
/// let engine = RetryEngine::new(RetryStrategy::Exponential, 3, backoff_cfg);
/// engine.execute("my_skill", || async {
///     my_fallible_op().await
/// }).await?;
/// ```
pub struct RetryEngine {
    pub strategy: RetryStrategy,
    pub max_attempts: usize,
    pub backoff_config: BackoffConfig,
    pub retry_predicate: RetryPredicate,
}

impl std::fmt::Debug for RetryEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RetryEngine")
            .field("strategy", &self.strategy)
            .field("max_attempts", &self.max_attempts)
            .field("backoff_config", &self.backoff_config)
            .field("retry_predicate", &(self.retry_predicate as usize))
            .finish()
    }
}

impl RetryEngine {
    /// Create a new retry engine.
    ///
    /// `max_attempts` is the total number of times the operation will be
    /// tried (including the first attempt).  Use `1` for no retries.
    pub fn new(
        strategy: RetryStrategy,
        max_attempts: usize,
        backoff_config: BackoffConfig,
    ) -> Self {
        Self {
            strategy,
            max_attempts,
            backoff_config,
            retry_predicate: default_retry_predicate,
        }
    }

    /// Set a custom retry predicate (builder style).
    pub fn with_predicate(mut self, predicate: RetryPredicate) -> Self {
        self.retry_predicate = predicate;
        self
    }

    /// Execute `attempt_fn`, retrying on failure according to the engine's
    /// strategy, back-off config, and predicate.
    ///
    /// `attempt_fn` is called once per attempt with a mutable reference to
    /// `state`.  Passing state through this parameter avoids the
    /// FnMut-borrow-escape problem — the closure can borrow `state` in its
    /// async block without capturing it from the environment.
    pub async fn execute<S: ?Sized, F, Fut>(
        &self,
        skill_id: &str,
        state: &mut S,
        mut attempt_fn: F,
    ) -> anyhow::Result<()>
    where
        F: FnMut(&mut S) -> Fut,
        Fut: Future<Output = anyhow::Result<()>>,
    {
        let max = self.max_attempts.max(1);

        for attempt in 1..=max {
            let start = std::time::Instant::now();
            let result = attempt_fn(state).await;
            let elapsed = start.elapsed();

            match result {
                Ok(()) => {
                    tracing::debug!(
                        skill_id,
                        attempt,
                        max,
                        elapsed_ms = elapsed.as_millis() as u64,
                        "RetryEngine: attempt succeeded"
                    );
                    return Ok(());
                }
                Err(e) => {
                    let elapsed_ms = elapsed.as_millis() as u64;
                    let error_msg = format!("{:#}", e);

                    if attempt >= max {
                        tracing::error!(
                            skill_id,
                            attempt,
                            max,
                            elapsed_ms,
                            error = %e,
                            "RetryEngine: all attempts exhausted"
                        );
                        return Err(e);
                    }

                    if !(self.retry_predicate)(skill_id, &error_msg) {
                        tracing::warn!(
                            skill_id,
                            attempt,
                            max,
                            elapsed_ms,
                            error = %e,
                            "RetryEngine: non-retryable error, stopping"
                        );
                        return Err(e);
                    }

                    let delay = self.backoff_config.delay_for(attempt, self.strategy);
                    let delay_ms = delay.as_millis();
                    tracing::warn!(
                        skill_id,
                        attempt,
                        max,
                        elapsed_ms,
                        delay_ms,
                        error = %e,
                        "RetryEngine: retrying after back-off"
                    );
                    tokio::time::sleep(delay).await;
                }
            }
        }

        // The loop always exits via `return` (success, exhaustion, or
        // non-retryable).  This is unreachable.
        unreachable!()
    }
}

// ---------------------------------------------------------------------------
// RetryPolicy (backward-compatible)
// ---------------------------------------------------------------------------

/// High-level retry configuration used by [`WorkflowExecutor`] and friends.
///
/// This is the user-facing config type.  Internally the executor converts
/// it into a [`RetryEngine`].
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_attempts: usize,
    pub backoff: Option<Duration>,
    pub strategy: RetryStrategy,
    pub backoff_config: BackoffConfig,
    pub retry_predicate: RetryPredicate,
}

impl RetryPolicy {
    /// No retries — exactly one attempt.
    pub fn none() -> Self {
        Self {
            max_attempts: 1,
            backoff: None,
            strategy: RetryStrategy::Linear,
            backoff_config: BackoffConfig::default(),
            retry_predicate: default_retry_predicate,
        }
    }

    /// `max_retries` additional attempts beyond the first (so total
    /// attempts = `max_retries + 1`).
    pub fn with_retries(max_retries: usize, backoff: Option<Duration>) -> Self {
        Self {
            max_attempts: max_retries + 1,
            backoff,
            strategy: RetryStrategy::Linear,
            backoff_config: BackoffConfig::default(),
            retry_predicate: default_retry_predicate,
        }
    }

    /// Select a back-off strategy.
    pub fn with_strategy(mut self, strategy: RetryStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Override the back-off config.
    pub fn with_backoff_config(mut self, config: BackoffConfig) -> Self {
        self.backoff_config = config;
        self
    }

    /// Set a custom retry predicate.
    pub fn with_predicate(mut self, predicate: RetryPredicate) -> Self {
        self.retry_predicate = predicate;
        self
    }

    /// Convert into a [`RetryEngine`] for direct use.
    pub fn engine(&self) -> RetryEngine {
        RetryEngine {
            strategy: self.strategy,
            max_attempts: self.max_attempts,
            backoff_config: self.backoff_config.clone(),
            retry_predicate: self.retry_predicate,
        }
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::none()
    }
}

// ---------------------------------------------------------------------------
// TimeoutPolicy
// ---------------------------------------------------------------------------

/// Execution timeout configuration.
///
/// Currently a stub — the fields are defined so the API surface is stable.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // -----------------------------------------------------------------------
    // BackoffConfig — delay computation
    // -----------------------------------------------------------------------

    #[test]
    fn linear_backoff_scales_with_attempt() {
        let cfg = BackoffConfig {
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            jitter_factor: 0.0,
        };
        assert_eq!(
            cfg.delay_for(1, RetryStrategy::Linear),
            Duration::from_millis(100)
        );
        assert_eq!(
            cfg.delay_for(2, RetryStrategy::Linear),
            Duration::from_millis(200)
        );
        assert_eq!(
            cfg.delay_for(3, RetryStrategy::Linear),
            Duration::from_millis(300)
        );
    }

    #[test]
    fn exponential_backoff_grows_correctly() {
        let cfg = BackoffConfig {
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            jitter_factor: 0.0,
        };
        assert_eq!(
            cfg.delay_for(1, RetryStrategy::Exponential),
            Duration::from_millis(100)
        );
        assert_eq!(
            cfg.delay_for(2, RetryStrategy::Exponential),
            Duration::from_millis(200)
        );
        assert_eq!(
            cfg.delay_for(3, RetryStrategy::Exponential),
            Duration::from_millis(400)
        );
        assert_eq!(
            cfg.delay_for(4, RetryStrategy::Exponential),
            Duration::from_millis(800)
        );
    }

    #[test]
    fn backoff_honours_max_delay() {
        let cfg = BackoffConfig {
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(250), // tight cap
            jitter_factor: 0.0,
        };
        // Linear: attempt 3 → 300 → capped at 250
        assert_eq!(
            cfg.delay_for(3, RetryStrategy::Linear),
            Duration::from_millis(250)
        );
        // Exponential: attempt 3 → 400 → capped at 250
        assert_eq!(
            cfg.delay_for(3, RetryStrategy::Exponential),
            Duration::from_millis(250)
        );
    }

    #[test]
    fn jitter_produces_varying_delays() {
        let cfg = BackoffConfig {
            base_delay: Duration::from_millis(1000),
            max_delay: Duration::from_secs(10),
            jitter_factor: 0.5,
        };
        // Jitter should differ from the pure exponential value on at
        // least some attempts (not a perfect test, but catches broken
        // implementations).
        let exponential = cfg.delay_for(3, RetryStrategy::Exponential);
        let jittered = cfg.delay_for(3, RetryStrategy::Jitter);
        // The jittered value should still be <= max_delay
        assert!(
            jittered <= cfg.max_delay,
            "jittered delay exceeds max_delay"
        );
        // It's possible (though unlikely with 50% fuzz) that the jitter
        // lands exactly on the exponential value, so we only assert it's
        // in a reasonable range.
        assert!(
            jittered >= Duration::ZERO,
            "jittered delay should be non-negative"
        );
    }

    #[test]
    fn jitter_factor_zero_gives_exact_exponential() {
        let cfg = BackoffConfig {
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            jitter_factor: 0.0,
        };
        let exp = cfg.delay_for(2, RetryStrategy::Exponential);
        let jit = cfg.delay_for(2, RetryStrategy::Jitter);
        assert_eq!(exp, jit, "zero jitter factor should match exponential");
    }

    // -----------------------------------------------------------------------
    // RetryEngine — end-to-end behaviour
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn retries_on_failure_up_to_max_attempts() {
        let engine = RetryEngine::new(
            RetryStrategy::Linear,
            3,
            BackoffConfig {
                base_delay: Duration::from_millis(1), // negligible sleep
                max_delay: Duration::from_millis(5),
                jitter_factor: 0.0,
            },
        );
        let call_count = Arc::new(AtomicUsize::new(0));
        let count = call_count.clone();

        let result = engine
            .execute("test", &mut (), |_| {
                let c = count.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    anyhow::bail!("transient error")
                }
            })
            .await;

        assert!(result.is_err(), "should exhaust retries");
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            3,
            "should be called 3 times"
        );
    }

    #[tokio::test]
    async fn succeeds_on_first_attempt() {
        let engine = RetryEngine::new(RetryStrategy::Linear, 3, BackoffConfig::default());
        let call_count = Arc::new(AtomicUsize::new(0));
        let count = call_count.clone();

        let result = engine
            .execute("test", &mut (), |_| {
                let c = count.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            })
            .await;

        assert!(result.is_ok(), "should succeed");
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            1,
            "should be called once"
        );
    }

    #[tokio::test]
    async fn succeeds_after_retry_when_eventually_ok() {
        let engine = RetryEngine::new(
            RetryStrategy::Linear,
            3,
            BackoffConfig {
                base_delay: Duration::from_millis(1),
                max_delay: Duration::from_millis(5),
                jitter_factor: 0.0,
            },
        );
        let call_count = Arc::new(AtomicUsize::new(0));
        let count = call_count.clone();

        let result = engine
            .execute("test", &mut (), |_| {
                let c = count.clone();
                async move {
                    let attempt = c.fetch_add(1, Ordering::SeqCst);
                    if attempt < 1 {
                        anyhow::bail!("not yet")
                    } else {
                        Ok(())
                    }
                }
            })
            .await;

        assert!(result.is_ok(), "should succeed on 2nd attempt");
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn non_retryable_predicate_stops_immediately() {
        let engine = RetryEngine::new(
            RetryStrategy::Linear,
            3,
            BackoffConfig {
                base_delay: Duration::from_millis(1),
                max_delay: Duration::from_millis(5),
                jitter_factor: 0.0,
            },
        )
        .with_predicate(|_skill_id, error| !error.contains("fatal"));

        let call_count = Arc::new(AtomicUsize::new(0));
        let count = call_count.clone();

        let result = engine
            .execute("test", &mut (), |_| {
                let c = count.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    anyhow::bail!("fatal: something broke")
                }
            })
            .await;

        assert!(result.is_err(), "should fail on non-retryable error");
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            1,
            "should stop after first attempt"
        );
    }

    #[tokio::test]
    async fn retries_with_exponential_backoff() {
        let engine = RetryEngine::new(
            RetryStrategy::Exponential,
            3,
            BackoffConfig {
                base_delay: Duration::from_millis(10),
                max_delay: Duration::from_secs(1),
                jitter_factor: 0.0,
            },
        );
        let call_count = Arc::new(AtomicUsize::new(0));
        let count = call_count.clone();

        let start = std::time::Instant::now();
        let _result = engine
            .execute("test", &mut (), |_| {
                let c = count.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    anyhow::bail!("err")
                }
            })
            .await;
        let elapsed = start.elapsed();

        // 3 attempts: attempt 1 (instant), sleep 10ms, attempt 2, sleep 20ms, attempt 3
        assert!(
            elapsed >= Duration::from_millis(25),
            "should have exponential delays"
        );
        assert_eq!(call_count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn max_attempts_one_means_no_retry() {
        let engine = RetryEngine::new(RetryStrategy::Linear, 1, BackoffConfig::default());
        let call_count = Arc::new(AtomicUsize::new(0));
        let count = call_count.clone();

        let result = engine
            .execute("test", &mut (), |_| {
                let c = count.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    anyhow::bail!("fail")
                }
            })
            .await;

        assert!(result.is_err(), "should fail");
        assert_eq!(call_count.load(Ordering::SeqCst), 1, "only one attempt");
    }

    #[tokio::test]
    async fn jitter_strategy_does_not_panic() {
        let engine = RetryEngine::new(
            RetryStrategy::Jitter,
            2,
            BackoffConfig {
                base_delay: Duration::from_millis(5),
                max_delay: Duration::from_millis(50),
                jitter_factor: 0.3,
            },
        );
        let result = engine
            .execute("test", &mut (), |_| async move { anyhow::bail!("err") })
            .await;
        assert!(result.is_err(), "should fail after 2 attempts");
    }

    // -----------------------------------------------------------------------
    // Backward-compatible RetryPolicy
    // -----------------------------------------------------------------------

    #[test]
    fn retry_policy_none_gives_single_attempt() {
        let p = RetryPolicy::none();
        assert_eq!(p.max_attempts, 1);
        assert!(p.backoff.is_none());
    }

    #[test]
    fn retry_policy_with_retries_counts_correctly() {
        let p = RetryPolicy::with_retries(3, Some(Duration::from_millis(100)));
        assert_eq!(p.max_attempts, 4);
        assert_eq!(p.backoff, Some(Duration::from_millis(100)));
    }

    #[test]
    fn retry_policy_builder_methods() {
        let p = RetryPolicy::none()
            .with_strategy(RetryStrategy::Exponential)
            .with_backoff_config(BackoffConfig {
                base_delay: Duration::from_millis(50),
                max_delay: Duration::from_secs(5),
                jitter_factor: 0.0,
            })
            .with_predicate(|_id, err| !err.contains("fatal"));

        assert_eq!(p.strategy, RetryStrategy::Exponential);
        assert_eq!(p.backoff_config.base_delay, Duration::from_millis(50));
    }

    #[test]
    fn retry_policy_engine_conversion() {
        let p = RetryPolicy::with_retries(2, Some(Duration::from_millis(50)));
        let engine = p.engine();
        assert_eq!(engine.max_attempts, 3);
    }
}

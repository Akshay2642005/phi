//! Workflow executor — runs individual skill nodes.
//!
//! The executor is the *data plane* for a workflow run.  It takes a skill
//! from the scheduler's ready queue, drives it through its lifecycle, and
//! writes the result back into the [`WorkflowContext`] and the
//! [`WorkflowGraph`].
//!
//! # Separation of concerns
//! - The **scheduler** decides *which* nodes to run and *when*.
//! - The **executor** decides *how* to run a single node (timeout, retries,
//!   error propagation, metrics).
//!
//! # Future parallel execution
//! The current implementation runs one skill at a time.  Upgrading to
//! concurrent execution requires calling [`WorkflowExecutor::execute_node`]
//! inside `tokio::spawn` / `JoinSet` tasks with an `Arc<Mutex<…>>` or
//! channel-based result delivery — no redesign of the executor is needed.

use std::time::Duration;

use anyhow::Result;

use crate::core::{
    context::WorkflowContext,
    errors::{SkillError, WorkflowError},
    skill::SkillRef,
    workflow::{
        graph::WorkflowGraph,
        node::NodeState,
        policy::RetryPolicy,
    },
};

// ── Executor ──────────────────────────────────────────────────────────────────

/// Runs individual skills and records outcomes in the graph and context.
pub struct WorkflowExecutor {
    /// Optional per-skill execution timeout.
    ///
    /// `None` means no timeout is applied.
    pub timeout: Option<Duration>,

    /// Retry policy applied on transient skill failures.
    ///
    /// See [`RetryPolicy`] for details.
    pub retry_policy: RetryPolicy,
}

impl WorkflowExecutor {
    /// Create an executor with no timeout and the default retry policy.
    pub fn new() -> Self {
        Self {
            timeout: None,
            retry_policy: RetryPolicy::default(),
        }
    }

    /// Create an executor with a specific timeout.
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            timeout: Some(timeout),
            retry_policy: RetryPolicy::default(),
        }
    }

    /// Execute a single skill node.
    ///
    /// # Flow
    /// 1. Transition the node to `Running`.
    /// 2. Call `skill.execute(ctx)`, optionally wrapped in a timeout.
    /// 3. On success: transition to `Completed`, mark in context.
    /// 4. On failure: transition to `Failed`, mark in context, wrap error.
    ///
    /// # Cancellation
    /// Before executing, the executor checks `ctx.is_cancelled()`.  If the
    /// workflow has been cancelled the node is transitioned to `Skipped` and
    /// [`WorkflowError::Cancelled`] is returned.
    pub async fn execute_node<I>(
        &self,
        skill: &SkillRef<I>,
        graph: &mut WorkflowGraph<I>,
        ctx: &mut WorkflowContext<I>,
    ) -> Result<(), WorkflowError>
    where
        I: Send + Sync + std::fmt::Debug + 'static,
    {
        let def = skill.definition();

        // ── Cancellation check ────────────────────────────────────────────────
        if ctx.is_cancelled() {
            graph.transition(def.id, NodeState::Skipped);
            tracing::info!(skill_id = def.id, "Skill skipped — workflow cancelled");
            return Err(WorkflowError::Cancelled);
        }

        // ── Transition to Running ─────────────────────────────────────────────
        graph.transition(def.id, NodeState::Running);

        tracing::info!(
            skill_id = def.id,
            skill_name = def.name,
            skill_version = def.version,
            "Executing skill"
        );

        // ── Execute (with optional timeout) ───────────────────────────────────
        let result = self.run_with_policy(skill, ctx).await;

        // ── Record outcome ────────────────────────────────────────────────────
        match result {
            Ok(()) => {
                graph.transition(def.id, NodeState::Completed);
                ctx.mark_completed(def.id);

                tracing::info!(
                    skill_id = def.id,
                    elapsed_ms = ctx.elapsed().as_millis() as u64,
                    "Skill completed successfully"
                );

                Ok(())
            }
            Err(e) => {
                let reason = e.to_string();
                graph.transition(def.id, NodeState::Failed { reason: reason.clone() });
                ctx.mark_failed(def.id);
                ctx.error(def.id, format!("Skill failed: {}", reason));

                tracing::error!(
                    skill_id = def.id,
                    error = %e,
                    "Skill execution failed"
                );

                Err(WorkflowError::SkillFailed {
                    skill_id: def.id.to_string(),
                    source: SkillError::Unexpected(e),
                })
            }
        }
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    /// Run the skill, applying retry and (stub) timeout policy.
    ///
    /// The current implementation only applies the retry count; timeout
    /// wrapping (`tokio::time::timeout`) is left as a TODO stub to avoid
    /// introducing a `Future` lifetime complexity before parallel execution is
    /// needed.
    async fn run_with_policy<I>(
        &self,
        skill: &SkillRef<I>,
        ctx: &mut WorkflowContext<I>,
    ) -> anyhow::Result<()>
    where
        I: Send + Sync + std::fmt::Debug + 'static,
    {
        let max_attempts = self.retry_policy.max_attempts.max(1);

        for attempt in 1..=max_attempts {
            match skill.execute(ctx).await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    if attempt < max_attempts {
                        tracing::warn!(
                            skill_id = skill.definition().id,
                            attempt,
                            max_attempts,
                            error = %e,
                            "Skill failed; retrying"
                        );
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        unreachable!("loop exits via return in all branches")
    }
}

impl Default for WorkflowExecutor {
    fn default() -> Self {
        Self::new()
    }
}

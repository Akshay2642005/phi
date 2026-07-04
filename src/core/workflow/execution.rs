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
    workflow::{graph::WorkflowGraph, policy::RetryPolicy, state::NodeState},
};

pub struct WorkflowExecutor {
    pub timeout: Option<Duration>,
    pub retry_policy: RetryPolicy,
}

impl WorkflowExecutor {
    pub fn new() -> Self {
        Self {
            timeout: None,
            retry_policy: RetryPolicy::default(),
        }
    }

    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            timeout: Some(timeout),
            retry_policy: RetryPolicy::default(),
        }
    }

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
        let skill_id = def.id;
        let execution_id = ctx.execution_id;

        tracing::trace!(
            execution_id = %execution_id,
            skill_id = skill_id,
            "Executor: execute_node called"
        );

        if ctx.is_cancelled() {
            graph.transition(def.id, NodeState::Skipped);
            tracing::info!(
                execution_id = %execution_id,
                skill_id = skill_id,
                "Skill skipped — workflow cancelled"
            );
            return Err(WorkflowError::Cancelled);
        }

        graph.transition(def.id, NodeState::Running);

        tracing::info!(
            execution_id = %execution_id,
            skill_id = skill_id,
            skill_name = def.name,
            skill_version = def.version,
            timeout_ms = self.timeout.map(|d| d.as_millis() as u64),
            max_attempts = self.retry_policy.max_attempts,
            "Executing skill"
        );

        let start = std::time::Instant::now();
        let result = self.run_with_policy(skill, ctx).await;
        let elapsed_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(()) => {
                graph.transition(def.id, NodeState::Completed);
                ctx.mark_completed(def.id);

                tracing::info!(
                    execution_id = %execution_id,
                    skill_id = skill_id,
                    elapsed_ms,
                    "Skill completed successfully"
                );

                Ok(())
            }
            Err(e) => {
                let reason = e.to_string();
                graph.transition(
                    def.id,
                    NodeState::Failed {
                        reason: reason.clone(),
                    },
                );
                ctx.mark_failed(def.id);
                ctx.error(def.id, format!("Skill failed: {}", reason));

                tracing::error!(
                    execution_id = %execution_id,
                    skill_id = skill_id,
                    elapsed_ms,
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

    async fn run_with_policy<I>(
        &self,
        skill: &SkillRef<I>,
        ctx: &mut WorkflowContext<I>,
    ) -> anyhow::Result<()>
    where
        I: Send + Sync + std::fmt::Debug + 'static,
    {
        let max_attempts = self.retry_policy.max_attempts.max(1);
        let skill_id = skill.definition().id;

        for attempt in 1..=max_attempts {
            let start = std::time::Instant::now();

            match skill.execute(ctx).await {
                Ok(()) => {
                    let elapsed_ms = start.elapsed().as_millis() as u64;
                    tracing::debug!(
                        skill_id = skill_id,
                        attempt,
                        elapsed_ms,
                        "Skill execution attempt succeeded"
                    );
                    return Ok(());
                }
                Err(e) => {
                    let elapsed_ms = start.elapsed().as_millis() as u64;

                    if attempt < max_attempts {
                        let backoff_ms = self
                            .retry_policy
                            .backoff
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0);

                        tracing::warn!(
                            skill_id = skill_id,
                            attempt,
                            max_attempts,
                            elapsed_ms,
                            backoff_ms,
                            error = %e,
                            "Skill failed; retrying"
                        );

                        if backoff_ms > 0 {
                            tokio::time::sleep(self.retry_policy.backoff.unwrap()).await;
                        }
                    } else {
                        tracing::error!(
                            skill_id = skill_id,
                            attempt,
                            max_attempts,
                            elapsed_ms,
                            error = %e,
                            "Skill failed; all attempts exhausted"
                        );
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

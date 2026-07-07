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
//! # Timeout hierarchy
//! - **Node timeout** (`node_timeout`): per-skill deadline enforced via
//!   `tokio::time::timeout`.  Retries respect this bound (each attempt gets
//!   its own timeout).
//! - **Workflow timeout** (`workflow_timeout`): total execution budget for
//!   the entire workflow, enforced at the engine loop level.

use std::time::Duration;

use anyhow::Result;
use tokio::time::timeout;

use crate::core::{
    context::WorkflowContext,
    errors::{SkillError, WorkflowError},
    skill::SkillRef,
    workflow::{graph::WorkflowGraph, policy::RetryPolicy, state::NodeState},
};

pub struct WorkflowExecutor {
    /// Per-node execution timeout (each attempt).
    pub node_timeout: Option<Duration>,
    /// Total workflow execution timeout.
    pub workflow_timeout: Option<Duration>,
    pub retry_policy: RetryPolicy,
}

impl WorkflowExecutor {
    pub fn new() -> Self {
        Self {
            node_timeout: None,
            workflow_timeout: None,
            retry_policy: RetryPolicy::default(),
        }
    }

    /// Create an executor with a per-node timeout.
    pub fn with_node_timeout(timeout: Duration) -> Self {
        Self {
            node_timeout: Some(timeout),
            workflow_timeout: None,
            retry_policy: RetryPolicy::default(),
        }
    }

    pub async fn execute_node<I>(
        &self,
        skill: &SkillRef<I>,
        graph: &mut WorkflowGraph<I>,
        ctx: &mut WorkflowContext<I>,
    ) -> std::result::Result<(), WorkflowError>
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
            node_timeout_ms = self.node_timeout.map(|d| d.as_millis() as u64),
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

    /// Run a skill with retry policy and optional node timeout.
    ///
    /// Each attempt is bounded by [`node_timeout`](Self::node_timeout) when set.
    /// The timeout is applied per-attempt, so retries get a fresh deadline each time.
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

            let skill_future = skill.execute(ctx);

            let result = if let Some(dur) = self.node_timeout {
                match timeout(dur, skill_future).await {
                    Ok(r) => r,
                    Err(_elapsed) => {
                        anyhow::bail!(
                            "skill '{}' timed out after {} ms (attempt {}/{})",
                            skill_id,
                            dur.as_millis(),
                            attempt,
                            max_attempts,
                        );
                    }
                }
            } else {
                skill_future.await
            };

            match result {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        context::WorkflowContext,
        skill::{Skill, SkillDefinition, SkillRef},
    };
    use async_trait::async_trait;
    use std::sync::Arc;

    #[derive(Clone)]
    struct TestSkill {
        def: &'static SkillDefinition,
        delay_ms: u64,
    }

    #[async_trait]
    impl Skill<()> for TestSkill {
        fn definition(&self) -> &'static SkillDefinition {
            self.def
        }

        async fn execute(&self, _ctx: &mut WorkflowContext<()>) -> anyhow::Result<()> {
            if self.delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
            }
            Ok(())
        }
    }

    fn make_skill(def: &'static SkillDefinition, delay_ms: u64) -> SkillRef<()> {
        Arc::new(TestSkill { def, delay_ms })
    }

    const SKILL_A: SkillDefinition = SkillDefinition {
        id: "a",
        name: "A",
        description: "",
        version: "1.0.0",
        dependencies: &[],
    };

    #[tokio::test]
    async fn no_timeout_completes_normally() {
        let skill = make_skill(&SKILL_A, 0);
        let executor = WorkflowExecutor::new();
        let (mut ctx, _) = WorkflowContext::new(());
        let mut graph = WorkflowGraph::new();
        graph.add_node(skill.clone());

        let result = executor.run_with_policy(&skill, &mut ctx).await;
        assert!(result.is_ok(), "no timeout should complete ok");
    }

    #[tokio::test]
    async fn node_timeout_fires_when_exceeded() {
        let skill = make_skill(&SKILL_A, 100);
        let executor = WorkflowExecutor::with_node_timeout(Duration::from_millis(20));
        let (mut ctx, _) = WorkflowContext::new(());

        let result = executor.run_with_policy(&skill, &mut ctx).await;
        assert!(result.is_err(), "timeout should produce error");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("timed out"),
            "error should mention timeout, got: {err}"
        );
    }

    #[tokio::test]
    async fn node_timeout_within_budget_succeeds() {
        let skill = make_skill(&SKILL_A, 10);
        let executor = WorkflowExecutor::with_node_timeout(Duration::from_millis(100));
        let (mut ctx, _) = WorkflowContext::new(());

        let result = executor.run_with_policy(&skill, &mut ctx).await;
        assert!(result.is_ok(), "skill within timeout should succeed");
    }
}

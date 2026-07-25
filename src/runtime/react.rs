//! ReAct runtime — multi-step reasoning loop.
//!
//! The [`ReActRuntime`] wraps the core engine and orchestrates the
//! Plan → Act → Observe loop:
//!
//! 1. Run the planner skill → get `PlanOutput` with `skills_to_run`
//! 2. Build a sub-registry containing only those skills
//! 3. Run the sub-registry → tool skills execute, results stored in context
//! 4. Loop back to step 1 with updated context
//! 5. When `PlanOutput.goal_complete = true`, return the final answer

use std::fmt::Debug;

use anyhow::Result;
use serde::de::DeserializeOwned;

use crate::core::{
    context::WorkflowContext,
    events::bus::EventBus,
    skill::{SkillRef, SkillRegistry},
    workflow::{engine::WorkflowEngine, state::WorkflowState},
};
use crate::runtime::planner::PlanOutput;

/// Result of a multi-step ReAct run.
#[derive(Debug)]
pub struct ReActResult {
    pub final_answer: String,
    pub iterations: usize,
}

/// Multi-step ReAct loop orchestrator.
///
/// This struct wraps [`WorkflowEngine`] and drives the iterative
/// Plan → Act → Observe loop required for true ReAct reasoning.
/// Each iteration builds a fresh DAG from the planner's chosen skills.
pub struct ReActRuntime<I>
where
    I: Send + Sync + Debug + DeserializeOwned + 'static,
{
    engine: WorkflowEngine<I>,
}

impl<I> ReActRuntime<I>
where
    I: Send + Sync + Debug + DeserializeOwned + 'static,
{
    pub fn new() -> Self {
        Self {
            engine: WorkflowEngine::new(),
        }
    }

    /// Attach an event bus to the inner engine.
    pub fn with_event_bus(mut self, bus: EventBus) -> Self {
        self.engine = self.engine.with_event_bus(bus);
        self
    }

    /// Run the ReAct loop until the planner signals completion.
    ///
    /// `registry` must contain at least a planner skill (id `"plan"`) and
    /// all tool skills the planner might invoke.
    ///
    /// Each iteration:
    /// 1. Runs only the planner → reads `PlanOutput`
    /// 2. If `goal_complete` → return with `final_answer`
    /// 3. Builds a sub-registry with only `skills_to_run`
    /// 4. Runs the sub-registry → tool results accumulate in `ctx`
    pub async fn run(
        &self,
        registry: &SkillRegistry<I>,
        ctx: &mut WorkflowContext<I>,
    ) -> Result<ReActResult> {
        let mut iterations = 0usize;

        loop {
            iterations += 1;

            // ── Step 1: Run only the planner ─────────────────────────
            let planner_registry = registry.filter_to_ids(&["plan"]);
            if planner_registry.skills().is_empty() {
                anyhow::bail!("ReActRuntime: no planner skill ('plan') in registry");
            }

            let result = self.engine.run(&planner_registry, ctx).await?;
            if !result.is_success() {
                anyhow::bail!(
                    "ReActRuntime: planner iteration {} failed: {:?}",
                    iterations,
                    result.errors()
                );
            }

            // ── Step 2: Read PlanOutput from context ─────────────────
            let plan = ctx.get::<PlanOutput>("plan").ok_or_else(|| {
                anyhow::anyhow!(
                    "ReActRuntime: iteration {} finished but no PlanOutput in context",
                    iterations
                )
            })?;

            if plan.goal_complete {
                let answer = plan
                    .final_answer
                    .clone()
                    .unwrap_or_else(|| "Goal complete (no final answer)".to_string());
                tracing::info!(
                    iterations,
                    answer = %answer,
                    "ReAct loop complete"
                );
                return Ok(ReActResult {
                    final_answer: answer,
                    iterations,
                });
            }

            if plan.skills_to_run.is_empty() {
                // Planner says not complete but no skills to run — treat as done.
                tracing::warn!(
                    iterations,
                    "Planner set goal_complete=false but skills_to_run is empty"
                );
                return Ok(ReActResult {
                    final_answer: "No skills to run; workflow incomplete.".to_string(),
                    iterations,
                });
            }

            // ── Step 3: Build sub-registry for tool skills ───────────
            let skill_ids: Vec<&str> = plan.skills_to_run.iter().map(String::as_str).collect();
            let sub_registry = registry.filter_to_ids(&skill_ids);

            if sub_registry.skills().is_empty() {
                tracing::warn!(
                    iterations,
                    skills = ?plan.skills_to_run,
                    "None of the requested skills are registered"
                );
                continue;
            }

            // ── Step 4: Run the tools ────────────────────────────────
            tracing::info!(
                iterations,
                skills = ?plan.skills_to_run,
                reasoning = %plan.reasoning,
                "Running tool skills"
            );

            let tool_result = self.engine.run(&sub_registry, ctx).await?;
            if !tool_result.is_success() {
                anyhow::bail!(
                    "ReActRuntime: tool iteration {} failed: {:?}",
                    iterations,
                    tool_result.errors()
                );
            }

            // Loop back to step 1.
        }
    }
}

impl<I> Default for ReActRuntime<I>
where
    I: Send + Sync + Debug + DeserializeOwned + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

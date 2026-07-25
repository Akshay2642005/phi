//! ReAct runtime — multi-step reasoning loop.
//!
//! The [`ReActRuntime`] wraps the core engine and orchestrates the
//! Plan → Act → Observe cycle. On the first call it asks the planner for
//! the full ordered list of skills to run, executes each one sequentially
//! (skipping already-completed skills), then calls the planner once more
//! to check whether the goal is complete and produce the final answer.
//!
//! This avoids the common failure mode where the LLM planner keeps
//! nominating the same already-completed skill on every iteration.

use std::collections::BTreeSet;
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

/// Safety limit to prevent infinite loops.
const MAX_ITERATIONS: usize = 20;

/// Result of a multi-step ReAct run.
#[derive(Debug)]
pub struct ReActResult {
    pub final_answer: String,
    pub iterations: usize,
    pub max_reached: bool,
}

/// Multi-step ReAct loop orchestrator.
///
/// This struct wraps [`WorkflowEngine`] and drives a two-phase execution:
///
/// **Phase 1** — Plan: run the planner once, collect the ordered list of
///   tool skills to execute.
/// **Phase 2** — Execute all tools sequentially (each in its own
///   `engine.run()` call), feeding results back into the context.
/// **Phase 3** — Confirm: run the planner once more to check whether the
///   goal is complete and produce the final answer.
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

    /// Run the planning + execution + confirmation cycle.
    ///
    /// `registry` must contain at least a planner skill (id `"plan"`) and
    /// all tool skills the planner might invoke.
    pub async fn run(
        &self,
        registry: &SkillRegistry<I>,
        ctx: &mut WorkflowContext<I>,
    ) -> Result<ReActResult> {
        // ── Step 1: Run the planner to get the plan ────────────────
        let plan = self.run_planner(registry, ctx).await?;

        if plan.goal_complete {
            return Ok(ReActResult {
                final_answer: plan
                    .final_answer
                    .unwrap_or_else(|| "Goal complete.".to_string()),
                iterations: 1,
                max_reached: false,
            });
        }

        if plan.skills_to_run.is_empty() {
            anyhow::bail!("Planner returned goal_complete=false with no skills to run");
        }

        // ── Step 2: Execute each requested tool sequentially ───────
        // Clone the plan so we can borrow ctx mutably in the loop.
        let skill_ids: Vec<String> = plan.skills_to_run.clone();
        let skill_params: std::collections::HashMap<String, String> = plan.skill_parameters.clone();

        let mut executed_count = 0usize;

        for skill_id in &skill_ids {
            // Build a one-skill registry first (no mutable ctx borrow).
            let sub = registry.filter_to_ids(&[skill_id.as_str()]);
            if sub.skills().is_empty() {
                tracing::warn!(
                    skill = %skill_id,
                    "Requested skill not registered — skipping"
                );
                continue;
            }

            // Pre-populate the skill input slot from planner parameters,
            // substituting {{skill_id}} placeholders with actual slot displays.
            if let Some(param) = skill_params.get(skill_id.as_str()) {
                let resolved = substitute_placeholders(param, ctx);
                tracing::debug!(
                    skill = %skill_id,
                    param = %param,
                    resolved = %resolved,
                    "Setting skill input parameter"
                );
                ctx.set::<String>(skill_id.clone(), resolved);
            }

            // Check if already completed — snapshot using owned strings so
            // the reference is dropped before the mutable borrow below.
            {
                let completed_set: BTreeSet<String> =
                    ctx.completed_skills.iter().cloned().collect();
                if completed_set.contains(skill_id) {
                    tracing::debug!(
                        skill = %skill_id,
                        "Skill already completed — skipping"
                    );
                    continue;
                }
            }

            tracing::info!(
                skill = %skill_id,
                "Running tool skill"
            );

            let tool_result = self.engine.run(&sub, ctx).await?;
            if !tool_result.is_success() {
                anyhow::bail!(
                    "ReActRuntime: skill '{}' failed: {:?}",
                    skill_id,
                    tool_result.errors()
                );
            }

            executed_count += 1;
        }

        // ── Step 3: Confirm completion ─────────────────────────────
        // Check if all planned skills completed successfully.
        let completed_set: BTreeSet<String> = ctx.completed_skills.iter().cloned().collect();
        let all_done = skill_ids.iter().all(|s| completed_set.contains(s));

        let total_iterations = 1 + executed_count;

        // Build a human-readable summary from slot display values.
        // Collect into a BTreeMap first for consistent ordering.
        let mut display_map: std::collections::BTreeMap<String, String> =
            std::collections::BTreeMap::new();
        for entry in ctx.slot_displays.iter() {
            display_map.insert(entry.key().clone(), entry.value().clone());
        }
        let summary_parts: Vec<String> = display_map
            .into_iter()
            .map(|(k, v)| format!("{}: {}", k, v))
            .collect();
        let summary = if summary_parts.is_empty() {
            "Task completed.".to_string()
        } else {
            summary_parts.join("\n")
        };

        if all_done {
            Ok(ReActResult {
                final_answer: summary,
                iterations: total_iterations,
                max_reached: false,
            })
        } else {
            anyhow::bail!(
                "Not all planned skills completed successfully: planned={:?}, completed={:?}",
                skill_ids,
                ctx.completed_skills
            );
        }
    }

    /// Run only the planner skill and return its output.
    async fn run_planner(
        &self,
        registry: &SkillRegistry<I>,
        ctx: &mut WorkflowContext<I>,
    ) -> Result<PlanOutput> {
        let planner_registry = registry.filter_to_ids(&["plan"]);
        if planner_registry.skills().is_empty() {
            anyhow::bail!("ReActRuntime: no planner skill ('plan') in registry");
        }

        self.engine.run(&planner_registry, ctx).await?;

        ctx.get::<PlanOutput>("plan")
            .map(|arc| (*arc).clone())
            .ok_or_else(|| anyhow::anyhow!("Planner finished but no PlanOutput in context"))
    }
}

/// Replace `{skill_id}` or `{{skill_id}}` placeholders in `param` with
/// the display value of that skill's output in `ctx.slot_displays`.
///
/// If a referenced skill has no display value, the placeholder is left
/// unchanged.  If multiple placeholders appear, each is resolved.
fn substitute_placeholders<I>(param: &str, ctx: &WorkflowContext<I>) -> String
where
    I: Send + Sync + Debug + 'static,
{
    let mut result = param.to_string();
    let bytes = param.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        // Look for '{' (single or double).
        if bytes[i] == b'{' {
            let skip_one = i + 1 < len && bytes[i + 1] == b'{';
            let start = if skip_one { i + 2 } else { i + 1 };

            if start >= len {
                break;
            }

            // Scan for the closing '}' (or '}}').
            let mut end = start;
            while end < len && bytes[end] != b'}' {
                end += 1;
            }
            if end >= len {
                break;
            }

            // Check for '}}' close (if we opened with '{{').
            let close_pair = skip_one && end + 1 < len && bytes[end + 1] == b'}';
            let placeholder_len = if close_pair { end + 2 } else { end + 1 };

            let skill_ref = std::str::from_utf8(&bytes[start..end]).unwrap_or("");
            if !skill_ref.is_empty()
                && skill_ref.chars().all(|c| c.is_alphanumeric() || c == '_')
                && let Some(display) = ctx.slot_displays.get(skill_ref)
            {
                let raw = &bytes[i..placeholder_len];
                let placeholder = std::str::from_utf8(raw).unwrap_or("");
                result = result.replace(placeholder, display.value());
            }
            i = placeholder_len;
        } else {
            i += 1;
        }
    }
    result
}

impl<I> Default for ReActRuntime<I>
where
    I: Send + Sync + Debug + DeserializeOwned + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

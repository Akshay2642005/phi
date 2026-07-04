//! Planner skill — the "brain" of the ReAct runtime.
//!
//! Given the runtime's goal and the list of available skills, the LLM decides:
//! 1. Which skills are needed to accomplish the goal.
//! 2. Whether the goal is already complete (based on prior step outputs).
//!
//! # ReAct pattern
//! This skill implements the **Reason** half of ReAct:
//!   Reason → Act (run chosen skills) → Observe (read slot outputs) → repeat
//!
//! Slot written: `"plan"` → [`PlanOutput`]
//! Dependencies: none (always runs first each step)

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::core::{
    context::WorkflowContext,
    llm::{LlmServiceExt as _, LlmServiceRef},
    skill::{Skill, SkillDefinition},
};
use crate::runtime::RuntimeInput;

// ── Output type ───────────────────────────────────────────────────────────────

/// The planner's decision for one ReAct step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanOutput {
    /// One-sentence reasoning explaining the plan.
    pub reasoning: String,
    /// Ordered list of skill IDs to run in this step.
    ///
    /// Skills are run in dependency order by the engine; the planner only
    /// needs to name them, not order them.
    pub skills_to_run: Vec<String>,
    /// `true` when the planner believes the goal is fully accomplished.
    pub goal_complete: bool,
    /// Final answer to present to the user when `goal_complete` is true.
    pub final_answer: Option<String>,
}

// ── Skill definition ──────────────────────────────────────────────────────────

pub const DEFINITION: SkillDefinition = SkillDefinition {
    id: "plan",
    name: "Planner",
    description: "Uses the LLM to decide which skills to run to accomplish the goal.",
    version: "1.0.0",
    dependencies: &[],
};

// ── Skill implementation ──────────────────────────────────────────────────────

pub struct PlannerSkill {
    pub llm: LlmServiceRef,
    /// Descriptions of every available tool skill, injected at construction.
    pub available_skills: Vec<SkillSummary>,
}

/// A brief description of a skill shown to the planner LLM.
#[derive(Debug, Clone)]
pub struct SkillSummary {
    pub id: &'static str,
    pub description: &'static str,
}

impl PlannerSkill {
    pub fn new(llm: LlmServiceRef, available_skills: Vec<SkillSummary>) -> Arc<Self> {
        Arc::new(Self {
            llm,
            available_skills,
        })
    }
}

#[async_trait]
impl Skill<RuntimeInput> for PlannerSkill {
    fn definition(&self) -> &'static SkillDefinition {
        &DEFINITION
    }

    async fn execute(&self, ctx: &mut WorkflowContext<RuntimeInput>) -> Result<()> {
        let goal = &ctx.input.goal;
        let extra_context = ctx.input.context.as_deref().unwrap_or("none");

        // Summarise what has been done so far (if anything)
        let prior_results = build_prior_results_summary(ctx);

        // List available skills for the LLM
        let skills_list: String = self
            .available_skills
            .iter()
            .map(|s| format!("  - \"{}\" : {}", s.id, s.description))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            r#"You are the planner for a digital worker. Your job is to decide which skills
to run next to accomplish the goal below.

GOAL: {goal}

EXTRA CONTEXT PROVIDED BY USER: {extra_context}

AVAILABLE SKILLS:
{skills_list}

RESULTS FROM PREVIOUS STEPS:
{prior_results}

INSTRUCTIONS:
- Choose only the skills needed for the NEXT step (don't re-run already-completed skills).
- If the goal is already fully accomplished based on prior results, set goal_complete to true
  and write a clear final_answer.
- Keep skills_to_run as an empty list when goal_complete is true.
- Return only valid JSON, no markdown, no extra text.

Respond with this exact JSON shape:
{{
  "reasoning": "<one sentence explaining your plan>",
  "skills_to_run": ["<skill_id>", ...],
  "goal_complete": <true|false>,
  "final_answer": "<answer string, or null if not complete>"
}}"#
        );

        tracing::debug!(skill = DEFINITION.id, "Calling LLM for plan");

        let output: PlanOutput = self.llm.prompt_typed(&prompt).await?;

        tracing::info!(
            skill = DEFINITION.id,
            reasoning = %output.reasoning,
            skills = ?output.skills_to_run,
            complete = output.goal_complete,
            "Plan generated"
        );

        ctx.set(DEFINITION.id, output);
        Ok(())
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn build_prior_results_summary<I>(ctx: &WorkflowContext<I>) -> String
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    if ctx.completed_skills.is_empty() {
        return "None — this is the first step.".to_string();
    }

    let mut lines = Vec::new();

    if !ctx.completed_skills.is_empty() {
        lines.push(format!(
            "Completed skills: {}",
            ctx.completed_skills.join(", ")
        ));
    }
    if !ctx.failed_skills.is_empty() {
        lines.push(format!("Failed skills: {}", ctx.failed_skills.join(", ")));
    }

    for d in ctx.diagnostics.iter().take(10) {
        if let Some(ref sid) = d.skill_id {
            lines.push(format!("[{}] {}", sid, d.message));
        } else {
            lines.push(d.message.clone());
        }
    }

    lines.join("\n")
}

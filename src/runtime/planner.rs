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

use std::collections::HashMap;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanOutput {
    pub reasoning: String,
    pub skills_to_run: Vec<String>,
    /// Per-skill input extracted from the goal by the LLM.
    /// Keyed by skill id — the runtime will pre-populate each
    /// skill's context slot so it receives the right input.
    pub skill_parameters: HashMap<String, String>,
    pub goal_complete: bool,
    pub final_answer: Option<String>,
}

pub const DEFINITION: SkillDefinition = SkillDefinition {
    id: "plan",
    name: "Planner",
    description: "Uses the LLM to decide which skills to run to accomplish the goal.",
    version: "1.0.0",
    dependencies: &[],
};

pub struct PlannerSkill {
    pub llm: LlmServiceRef,
    pub available_skills: Vec<SkillSummary>,
}

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

        let prior_results = build_prior_results_summary(ctx);

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

INSTRUCTIONS (CRITICAL):
- List ALL skills needed to accomplish the goal, in execution order.
  Include every step from start to finish — do not skip intermediate steps.
- You MUST NOT re-run a skill that has ALREADY BEEN COMPLETED.
  The completed-skills list tells you what has already been done.
- For each skill in skills_to_run, extract the exact input that skill needs from the goal
  into skill_parameters. The key is the skill id, the value is the input string.
- If a later skill needs the result from an earlier skill, write "{{<skill_id>}}" as a
  placeholder in its parameter. Example: "interesting fact about {{calculator}}".
  The runtime will fill in the actual value.
- WEB_SEARCH QUERY TIP: The web_search skill uses DuckDuckGo Instant Answer API which
  only works with SHORT keyword queries (e.g. "42" or "Eiffel Tower height").
  Do NOT write conversational phrases like "interesting fact about the number 42" —
  they will return zero results. Write 2-5 keyword queries.
- If the goal is already fully accomplished based on prior results, set goal_complete to true
  and write a clear final_answer.
- Keep skills_to_run as an empty list when goal_complete is true.
- Return only valid JSON, no markdown, no extra text.

Respond with this exact JSON shape:
{{
  "reasoning": "<one sentence explaining your plan>",
  "skills_to_run": ["<skill_id>", ...],
  "skill_parameters": {{
    "<skill_id>": "<exact input for that skill extracted from the goal>"
  }},
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

fn build_prior_results_summary<I>(ctx: &WorkflowContext<I>) -> String
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    if ctx.completed_skills.is_empty() && ctx.failed_skills.is_empty() {
        return "None — this is the first step.".to_string();
    }

    let mut lines = Vec::new();

    if !ctx.completed_skills.is_empty() {
        // Deduplicate while preserving insertion order.
        let mut seen = std::collections::HashSet::new();
        let deduped: Vec<&String> = ctx
            .completed_skills
            .iter()
            .filter(|s| seen.insert(s.as_str()))
            .collect();
        if !deduped.is_empty() {
            lines.push(format!(
                "Completed skills (deduplicated): {}",
                deduped
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    if !ctx.failed_skills.is_empty() {
        let mut seen = std::collections::HashSet::new();
        let deduped: Vec<&String> = ctx
            .failed_skills
            .iter()
            .filter(|s| seen.insert(s.as_str()))
            .collect();
        if !deduped.is_empty() {
            lines.push(format!(
                "Failed skills: {}",
                deduped
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }

    // Show populated slot keys so the LLM knows what data is available.
    let slot_keys: Vec<String> = ctx.slots.keys();
    if !slot_keys.is_empty() {
        lines.push(format!(
            "Available data in context slots: {}",
            slot_keys.join(", ")
        ));
        // Show human-readable display values for slots that have them.
        let mut display_lines: Vec<String> = Vec::new();
        for entry in ctx.slot_displays.iter() {
            display_lines.push(format!("  {}: {}", entry.key(), entry.value()));
        }
        if !display_lines.is_empty() {
            lines.push("Slot values:".to_string());
            lines.extend(display_lines);
        }
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

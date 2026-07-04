//! ReAct loop — the top-level orchestrator for the runtime agent.
//!
//! # What is ReAct?
//! ReAct (Reason + Act) is a pattern where an LLM alternates between:
//! - **Reasoning**: deciding what to do next (PlannerSkill)
//! - **Acting**: executing chosen tools (WebSearch, Sentiment, Summary, …)
//! - **Observing**: reading tool outputs and looping back to reason again
//!
//! # Loop structure
//! ```text
//! Step 1:  run PlannerSkill  →  PlanOutput { skills_to_run, goal_complete }
//!          if goal_complete  →  print final_answer, stop
//!          else              →  build sub-registry of chosen skills + deps
//!
//! Step 2:  run sub-registry  →  each chosen skill writes to ctx.slots
//!
//! Step 3:  inject slot summaries into ctx diagnostics so planner sees them
//!          on the next step
//!
//! Repeat until goal_complete == true or max_steps reached.
//! ```

use std::sync::Arc;

use anyhow::{Result, anyhow};

use crate::core::{
    context::WorkflowContext,
    plugin::{Plugin, PluginContext},
    skill::{SkillRegistry, SkillRef},
    workflow::WorkflowEngine,
};
use crate::runtime::{
    RuntimeInput,
    planner::{PlanOutput, DEFINITION as PLAN_DEF},
    plugin::RuntimePlugin,
};

// ── ReAct loop ────────────────────────────────────────────────────────────────

/// Configuration for the ReAct loop.
pub struct ReactConfig {
    /// Maximum number of Reason-Act cycles before aborting.
    pub max_steps: usize,
}

impl Default for ReactConfig {
    fn default() -> Self {
        Self { max_steps: 5 }
    }
}

/// Final output produced by the loop.
pub struct ReactResult {
    /// The planner's stated final answer (if goal was marked complete).
    pub final_answer: Option<String>,
    /// Total number of steps taken.
    pub steps_taken: usize,
    /// Whether the goal was declared complete by the planner.
    pub goal_complete: bool,
}

/// Run the ReAct loop for the given `RuntimeInput`.
pub async fn run_react_loop(
    plugin: &RuntimePlugin,
    input: RuntimeInput,
    config: ReactConfig,
) -> Result<ReactResult> {
    // ── Build the full registry once (all skills available to the planner) ──
    let mut plugin_ctx = PluginContext::<RuntimeInput>::new();
    plugin.register(&mut plugin_ctx);
    let full_registry = plugin_ctx.into_registry();

    // ── Create a single shared context for all steps ──────────────────────
    let (mut ctx, _cancel) = WorkflowContext::new(input);

    let engine = WorkflowEngine::<RuntimeInput>::new();

    for step in 1..=config.max_steps {
        tracing::info!(step, max = config.max_steps, "ReAct step starting");
        println!("\n── Step {step}/{} ─────────────────────────────────────", config.max_steps);

        // ── Phase 1: Reason — run the planner only ────────────────────────
        let planner_registry = single_skill_registry(&full_registry, PLAN_DEF.id)?;
        engine
            .run(&planner_registry, &mut ctx)
            .await
            .map_err(|e| anyhow!("planner failed: {e}"))?;

        // Read the plan output
        let plan = ctx
            .get::<PlanOutput>(PLAN_DEF.id)
            .ok_or_else(|| anyhow!("planner did not write output to context slot"))?;

        println!("🤔 Reasoning: {}", plan.reasoning);
        println!("🛠  Skills chosen: {:?}", plan.skills_to_run);

        // ── Check for completion ──────────────────────────────────────────
        if plan.goal_complete {
            println!("\n✅ Goal complete!");
            return Ok(ReactResult {
                final_answer: plan.final_answer.clone(),
                steps_taken: step,
                goal_complete: true,
            });
        }

        if plan.skills_to_run.is_empty() {
            tracing::warn!("Planner chose no skills and goal is not complete — stopping.");
            break;
        }

        // ── Phase 2: Act — run only the chosen skills ─────────────────────
        let act_registry = build_sub_registry(&full_registry, &plan.skills_to_run)?;

        match engine.run(&act_registry, &mut ctx).await {
            Ok(result) => {
                if result.is_success() {
                    println!("✔  Skills completed: {:?}", result.completed());
                } else {
                    println!("⚠  Some skills failed: {:?}", result.failed());
                    for err in result.errors() {
                        println!("   Error: {err}");
                    }
                }
            }
            Err(e) => {
                tracing::error!(step, error = %e, "Skill execution error in ReAct loop");
                println!("❌ Step {step} error: {e}");
            }
        }

        // ── Phase 3: Observe ──────────────────────────────────────────────
        observe_web_search(&ctx);
        observe_summary(&ctx);
    }

    Ok(ReactResult {
        final_answer: None,
        steps_taken: config.max_steps,
        goal_complete: false,
    })
}

// ── Sub-registry helpers ──────────────────────────────────────────────────────

fn single_skill_registry(
    full: &SkillRegistry<RuntimeInput>,
    id: &str,
) -> Result<SkillRegistry<RuntimeInput>> {
    let mut reg = SkillRegistry::new();
    let skill = full
        .skills()
        .get(id)
        .ok_or_else(|| anyhow!("skill '{id}' not found in registry"))?;
    reg.register(Arc::clone(skill));
    Ok(reg)
}

fn build_sub_registry(
    full: &SkillRegistry<RuntimeInput>,
    skill_ids: &[String],
) -> Result<SkillRegistry<RuntimeInput>> {
    let mut reg = SkillRegistry::new();
    for id in skill_ids {
        add_with_deps(full, &mut reg, id)?;
    }
    Ok(reg)
}

fn add_with_deps(
    full: &SkillRegistry<RuntimeInput>,
    target: &mut SkillRegistry<RuntimeInput>,
    id: &str,
) -> Result<()> {
    if target.contains(id) {
        return Ok(());
    }

    let skill: SkillRef<RuntimeInput> = full
        .skills()
        .get(id)
        .ok_or_else(|| anyhow!("skill '{id}' requested by planner is not registered"))?
        .clone();

    for dep_id in skill.definition().dependencies {
        add_with_deps(full, target, dep_id)?;
    }

    target.register(skill);
    Ok(())
}

// ── Observation helpers ───────────────────────────────────────────────────────

fn observe_web_search(ctx: &WorkflowContext<RuntimeInput>) {
    if let Some(ws) = ctx.get::<crate::tools::web_search::WebSearchOutput>("web_search") {
        if !ws.abstract_text.is_empty() {
            println!("\n🔍 Search result ({}):", ws.abstract_source);
            let preview: String = ws.abstract_text.chars().take(300).collect();
            let suffix = if ws.abstract_text.len() > 300 { "…" } else { "" };
            println!("   {}{}", preview, suffix);
            println!("   Source: {}", ws.abstract_url);
        } else {
            println!("\n🔍 Web search returned no abstract.");
            for r in ws.related.iter().take(3) {
                println!("   - {}", r.text.chars().take(120).collect::<String>());
            }
        }
    }
}

fn observe_summary(ctx: &WorkflowContext<RuntimeInput>) {
    use crate::runtime::analysis::SummaryOutput;
    if let Some(sum) = ctx.get::<SummaryOutput>("summary") {
        println!("\n📝 Summary: {}", sum.summary);
        println!("   Topics: {}", sum.key_topics.join(", "));
    }
}

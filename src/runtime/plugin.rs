//! Runtime plugin — registers all skills for the runtime ReAct agent.
//!
//! Skills registered:
//! - `plan`        — PlannerSkill (always first in every ReAct step)
//! - `web_search`  — WebSearchSkill
//! - `word_count`  — word count analysis (adapted for RuntimeInput)
//! - `sentiment`   — sentiment analysis (adapted for RuntimeInput)
//! - `summary`     — text summary (adapted for RuntimeInput)
//!
//! The ReAct loop does NOT run all skills every step. It runs the planner
//! first, reads `PlanOutput.skills_to_run`, then builds a sub-registry
//! containing only those skills + their transitive dependencies.

use std::sync::Arc;

use crate::core::{
    llm::LlmServiceRef,
    plugin::{Plugin, PluginContext, PluginMetadata},
    skill::SkillDefinition,
};
use crate::runtime::{
    RuntimeInput,
    planner::{PlannerSkill, SkillSummary},
};
use crate::tools::web_search::WebSearchSkill;

// ── Metadata ──────────────────────────────────────────────────────────────────

pub static METADATA: PluginMetadata = PluginMetadata {
    id: "runtime",
    name: "Runtime Agent",
    description: "ReAct planner + web search + text analysis tools.",
    version: "1.0.0",
};

// ── Backwards compat alias ────────────────────────────────────────────────────

/// Alias kept so existing `WorkerPlugin` references still compile.
pub type WorkerPlugin = RuntimePlugin;

// ── Skill catalogue ───────────────────────────────────────────────────────────

/// Ordered list of all non-planner skills + their descriptions.
/// This is shown verbatim to the LLM planner.
pub fn tool_skill_catalogue() -> Vec<SkillSummary> {
    vec![
        SkillSummary {
            id: crate::tools::web_search::DEFINITION.id,
            description: crate::tools::web_search::DEFINITION.description,
        },
        SkillSummary {
            id: crate::runtime::analysis::WORD_COUNT_DEF.id,
            description: crate::runtime::analysis::WORD_COUNT_DEF.description,
        },
        SkillSummary {
            id: crate::runtime::analysis::SENTIMENT_DEF.id,
            description: crate::runtime::analysis::SENTIMENT_DEF.description,
        },
        SkillSummary {
            id: crate::runtime::analysis::SUMMARY_DEF.id,
            description: crate::runtime::analysis::SUMMARY_DEF.description,
        },
    ]
}

// ── Plugin ────────────────────────────────────────────────────────────────────

/// Registers the planner + all tool skills for `RuntimeInput`.
pub struct RuntimePlugin {
    llm: LlmServiceRef,
}

impl RuntimePlugin {
    pub fn new(llm: LlmServiceRef) -> Self {
        Self { llm }
    }
}

impl Plugin<RuntimeInput> for RuntimePlugin {
    fn metadata(&self) -> &'static PluginMetadata {
        &METADATA
    }

    fn register(&self, ctx: &mut PluginContext<RuntimeInput>) {
        let catalogue = tool_skill_catalogue();

        // Planner — always registered
        ctx.register_skill(PlannerSkill::new(self.llm.clone(), catalogue));

        // Tool skills
        ctx.register_skill(WebSearchSkill::new());

        // Analysis skills (adapted for RuntimeInput)
        ctx.register_skill(crate::runtime::analysis::word_count_skill());
        ctx.register_skill(crate::runtime::analysis::sentiment_skill(self.llm.clone()));
        ctx.register_skill(crate::runtime::analysis::summary_skill(self.llm.clone()));
    }
}

//! Runtime plugin — registers all skills for the runtime ReAct agent.
//!
//! Skills registered:
//! - `plan`        — PlannerSkill (always first in every ReAct step)
//! - `web_search`  — WebSearchSkill
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

pub static METADATA: PluginMetadata = PluginMetadata {
    id: "runtime",
    name: "Runtime Agent",
    description: "ReAct planner + web search + text analysis tools.",
    version: "1.0.0",
};

pub type WorkerPlugin = RuntimePlugin;

pub fn tool_skill_catalogue() -> Vec<SkillSummary> {
    vec![SkillSummary {
        id: crate::tools::web_search::DEFINITION.id,
        description: crate::tools::web_search::DEFINITION.description,
    }]
}

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
        ctx.register_skill(PlannerSkill::new(self.llm.clone(), catalogue));
        ctx.register_skill(WebSearchSkill::new());
    }
}

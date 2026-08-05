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
use crate::tools::{
    calculator::CalculatorSkill, file_read::FileReadSkill, file_write::FileWriteSkill,
    shell_command::ShellCommandSkill, web_search::WebSearchSkill,
};

pub static METADATA: PluginMetadata = PluginMetadata {
    id: "runtime",
    name: "Runtime Agent",
    description: "ReAct planner + web search + text analysis tools.",
    version: "1.0.0",
};

pub type WorkerPlugin = RuntimePlugin;

pub fn tool_skill_catalogue() -> Vec<SkillSummary> {
    vec![
        SkillSummary {
            id: crate::tools::calculator::DEFINITION.id,
            description: crate::tools::calculator::DEFINITION.description,
        },
        SkillSummary {
            id: crate::tools::file_read::DEFINITION.id,
            description: crate::tools::file_read::DEFINITION.description,
        },
        SkillSummary {
            id: crate::tools::file_write::DEFINITION.id,
            description: crate::tools::file_write::DEFINITION.description,
        },
        SkillSummary {
            id: crate::tools::shell_command::DEFINITION.id,
            description: crate::tools::shell_command::DEFINITION.description,
        },
        SkillSummary {
            id: crate::tools::web_search::DEFINITION.id,
            description: crate::tools::web_search::DEFINITION.description,
        },
    ]
}

pub struct RuntimePlugin {
    llm: LlmServiceRef,
    tavily_api_key: String,
}

impl RuntimePlugin {
    pub fn new(llm: LlmServiceRef, tavily_api_key: String) -> Self {
        Self {
            llm,
            tavily_api_key,
        }
    }
}

impl Plugin<RuntimeInput> for RuntimePlugin {
    fn metadata(&self) -> &'static PluginMetadata {
        &METADATA
    }

    fn register(&self, ctx: &mut PluginContext<RuntimeInput>) {
        let catalogue = tool_skill_catalogue();
        ctx.register_skill(PlannerSkill::new(self.llm.clone(), catalogue));
        ctx.register_skill(CalculatorSkill::new());
        ctx.register_skill(FileReadSkill::new());
        ctx.register_skill(FileWriteSkill::new());
        ctx.register_skill(ShellCommandSkill::new());
        ctx.register_skill(WebSearchSkill::new(self.tavily_api_key.clone()));
    }
}

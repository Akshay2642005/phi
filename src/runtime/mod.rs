//! Runtime layer — wraps the workflow engine in a ReAct reasoning loop.
//!
//! The runtime owns:
//! - Workflow lifecycle (start, step, cancel)
//! - The ReAct reasoning loop
//! - Execution coordination between the planner and tool skills
//! - Cancellation, progress tracking
//!
//! # Modules
//! - [`RuntimeInput`]  — the domain input type shared across all runtime skills.
//! - [`planner`]       — `PlannerSkill`: LLM selects which tool skills to run.
//! - [`plugin`]        — `RuntimePlugin`: registers planner + all tool skills.

pub mod planner;
pub mod plugin;

#[derive(Debug, Clone)]
pub struct RuntimeInput {
    pub goal: String,
    pub context: Option<String>,
}

pub type WorkerInput = RuntimeInput;

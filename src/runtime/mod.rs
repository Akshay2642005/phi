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
//! - [`react_loop`]    — `run_react_loop`: orchestrates multi-step execution.
//! - [`analysis`]      — analysis skill adapters for `RuntimeInput`.

pub mod analysis;
pub mod planner;
pub mod plugin;
pub mod react_loop;

// ── Domain input ──────────────────────────────────────────────────────────────

/// The immutable input passed to every skill in a runtime workflow run.
#[derive(Debug, Clone)]
pub struct RuntimeInput {
    /// The high-level goal the runtime should accomplish.
    ///
    /// Example: `"Summarise the latest Rust blog post"`
    pub goal: String,

    /// Optional extra context the caller wants to provide upfront.
    ///
    /// Example: a URL, a snippet of text, a previous result.
    pub context: Option<String>,
}

// ── Backwards compatibility alias ─────────────────────────────────────────────

/// Alias for [`RuntimeInput`] kept for any code that still imports `WorkerInput`.
pub type WorkerInput = RuntimeInput;

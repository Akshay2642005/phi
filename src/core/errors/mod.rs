//! Domain error types for the AI Workflow Engine.
//!
//! All structured errors are defined here with [`thiserror`] so that callers
//! can pattern-match on specific failure modes rather than parsing strings.
//!
//! # Hierarchy
//! - [`WorkflowError`] — errors at the workflow-orchestration level
//! - [`SkillError`] — errors produced during skill execution
//! - [`EngineError`] — top-level engine / bootstrap errors

use thiserror::Error;

// ── Skill errors ─────────────────────────────────────────────────────────────

/// Errors that originate inside a skill's `execute` method.
#[derive(Debug, Error)]
pub enum SkillError {
    /// A required predecessor output is absent from the context.
    ///
    /// This usually means the dependency graph is wrong or the predecessor
    /// skill failed without propagating the error.
    #[error("prerequisite '{0}' is missing from the workflow context")]
    MissingPrerequisite(&'static str),

    /// The LLM returned a response that could not be parsed into the
    /// expected output type.
    #[error("failed to deserialize LLM response: {0}")]
    OutputDeserializationFailed(#[source] serde_json::Error),

    /// The LLM call itself failed (network, timeout, quota, etc.).
    #[error("LLM call failed: {0}")]
    LlmCallFailed(#[source] anyhow::Error),

    /// A field required to build the skill prompt is absent.
    #[error("required field '{0}' is missing for prompt construction")]
    MissingPromptInput(&'static str),

    /// Catch-all for unexpected skill failures that do not fit any of
    /// the variants above.
    #[error("skill execution failed: {0}")]
    Unexpected(#[source] anyhow::Error),
}

// ── Workflow errors ───────────────────────────────────────────────────────────

/// Errors raised by the workflow orchestration layer (planner, scheduler,
/// executor, graph, engine).
#[derive(Debug, Error)]
pub enum WorkflowError {
    /// A cycle was found in the dependency graph of skills.
    ///
    /// The engine cannot execute a cyclic graph; the skill definitions must
    /// be fixed before the workflow can run.
    #[error("dependency cycle detected involving skill '{0}'")]
    DependencyCycle(String),

    /// The dependency graph references a skill that was never registered.
    #[error("skill '{required}' is required by '{dependent}' but is not registered")]
    MissingDependency {
        /// The skill that has the unsatisfied requirement.
        dependent: String,
        /// The skill ID that is missing from the registry.
        required: String,
    },

    /// No skills were registered or the plan produced an empty execution list.
    #[error("workflow execution plan is empty — no skills to execute")]
    EmptyPlan,

    /// A skill in the plan failed during execution.
    ///
    /// `skill_id` names the failing skill; the `source` is the underlying
    /// [`SkillError`].
    #[error("skill '{skill_id}' failed during execution")]
    SkillFailed {
        /// Identifier of the skill that failed.
        skill_id: String,
        /// The underlying skill error.
        #[source]
        source: SkillError,
    },

    /// The workflow was cancelled before completion (e.g., via a
    /// cancellation token).
    #[error("workflow was cancelled")]
    Cancelled,

    /// The workflow exceeded its allocated time budget.
    #[error("workflow timed out after {elapsed_ms} ms")]
    Timeout {
        /// How long the workflow ran before the timeout was triggered.
        elapsed_ms: u64,
    },

    /// The scheduler detected a deadlock: nodes remain but none are runnable.
    ///
    /// This should not happen if cycle detection passes; it is a safety-net
    /// error that indicates an engine bug.
    #[error(
        "scheduler deadlock: {pending} node(s) pending but none are ready to execute"
    )]
    SchedulerDeadlock {
        /// How many nodes are still waiting.
        pending: usize,
    },

    /// A context read/write error (e.g., type mismatch for a named slot).
    #[error("context error: {0}")]
    ContextError(String),
}

// ── Engine errors ─────────────────────────────────────────────────────────────

/// Top-level errors produced during engine construction or startup.
#[derive(Debug, Error)]
pub enum EngineError {
    /// A required environment variable or configuration key is missing or
    /// has an invalid value.
    #[error("configuration error: {0}")]
    ConfigurationError(String),

    /// The LLM backend could not be initialised (bad URL, missing key, etc.).
    #[error("failed to initialise LLM service: {0}")]
    LlmInitFailed(#[source] anyhow::Error),

    /// The plugin failed to register its contributions with the engine.
    #[error("plugin '{name}' failed to load: {source}")]
    PluginLoadFailed {
        /// Human-readable plugin name.
        name: String,
        /// Underlying cause.
        #[source]
        source: anyhow::Error,
    },

    /// An internal engine invariant was violated (bug, not user error).
    #[error("internal engine error: {0}")]
    Internal(String),
}

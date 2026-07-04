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

#[derive(Debug, Error)]
pub enum SkillError {
    #[error("prerequisite '{0}' is missing from the workflow context")]
    MissingPrerequisite(&'static str),

    #[error("failed to deserialize LLM response: {0}")]
    OutputDeserializationFailed(#[source] serde_json::Error),

    #[error("LLM call failed: {0}")]
    LlmCallFailed(#[source] anyhow::Error),

    #[error("required field '{0}' is missing for prompt construction")]
    MissingPromptInput(&'static str),

    #[error("skill execution failed: {0}")]
    Unexpected(#[source] anyhow::Error),
}

#[derive(Debug, Error)]
pub enum WorkflowError {
    #[error("dependency cycle detected involving skill '{0}'")]
    DependencyCycle(String),

    #[error("skill '{required}' is required by '{dependent}' but is not registered")]
    MissingDependency {
        dependent: String,
        required: String,
    },

    #[error("workflow execution plan is empty — no skills to execute")]
    EmptyPlan,

    #[error("skill '{skill_id}' failed during execution")]
    SkillFailed {
        skill_id: String,
        #[source]
        source: SkillError,
    },

    #[error("workflow was cancelled")]
    Cancelled,

    #[error("workflow timed out after {elapsed_ms} ms")]
    Timeout {
        elapsed_ms: u64,
    },

    #[error("scheduler deadlock: {pending} node(s) pending but none are ready to execute")]
    SchedulerDeadlock {
        pending: usize,
    },

    #[error("context error: {0}")]
    ContextError(String),
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("configuration error: {0}")]
    ConfigurationError(String),

    #[error("failed to initialise LLM service: {0}")]
    LlmInitFailed(#[source] anyhow::Error),

    #[error("plugin '{name}' failed to load: {source}")]
    PluginLoadFailed {
        name: String,
        #[source]
        source: anyhow::Error,
    },

    #[error("internal engine error: {0}")]
    Internal(String),
}

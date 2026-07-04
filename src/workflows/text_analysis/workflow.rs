//! Text analysis workflow — top-level workflow definition.
//!
//! This file defines the workflow as a portable, self-contained unit.
//! The engine interacts with the workflow only through its input type
//! ([`TextAnalysisInput`]) and the plugin ([`TextAnalysisPlugin`]).
//!
//! # Package layout (portable workflow)
//!
//! ```text
//! text_analysis/
//! ├── workflow.rs          — Workflow struct + metadata (this file)
//! ├── mod.rs               — TextAnalysisInput + re-exports
//! ├── config.rs            — TextAnalysisConfig (tunable parameters)
//! ├── plugin.rs            — TextAnalysisPlugin (registers skills)
//! ├── prompts/             — Prompt templates (future)
//! │   └── ...              — (one file per LLM skill)
//! ├── skills/              — Skill implementations
//! │   ├── mod.rs
//! │   ├── word_count.rs
//! │   ├── sentiment.rs
//! │   └── summary.rs
//! ├── artifacts/           — Artifact type definitions (future)
//! │   └── ...              — (one file per artifact type)
//! └── README.md            — Workflow documentation
//! ```
//!
//! # Portability
//! This entire directory can be copied into another project and registered
//! with the engine there.  The engine knows nothing about the workflow's
//! internals — it only sees a [`Plugin<TextAnalysisInput>`].

use std::sync::Arc;

use crate::core::{
    context::WorkflowContext,
    errors::WorkflowError,
    plugin::Plugin,
    skill::SkillRegistry,
    workflow::WorkflowEngine,
};

use super::{TextAnalysisInput, config::TextAnalysisConfig, plugin::TextAnalysisPlugin};

// ── Workflow metadata ─────────────────────────────────────────────────────────

/// Static metadata describing the text analysis workflow.
///
/// This mirrors the pattern used by skills and plugins for consistency.
#[derive(Debug, Clone)]
pub struct WorkflowMetadata {
    /// Unique, stable identifier for this workflow.
    pub id: &'static str,
    /// Human-readable workflow name.
    pub name: &'static str,
    /// One-line description of what the workflow does.
    pub description: &'static str,
    /// Semver-style version string.
    pub version: &'static str,
}

/// Canonical metadata for the text analysis workflow.
pub static WORKFLOW_METADATA: WorkflowMetadata = WorkflowMetadata {
    id: "text_analysis",
    name: "Text Analysis",
    description: "Analyses a piece of text: word count → sentiment → summary.",
    version: "1.0.0",
};

// ── Workflow runner ───────────────────────────────────────────────────────────

/// A ready-to-run text analysis workflow.
///
/// Owns the plugin, registry, and engine.  Call [`TextAnalysisWorkflow::run`]
/// with an input to execute the full pipeline.
///
/// # Example
/// ```ignore
/// let workflow = TextAnalysisWorkflow::new(llm);
/// let result = workflow.run(input).await?;
/// ```
pub struct TextAnalysisWorkflow {
    plugin: TextAnalysisPlugin,
    engine: WorkflowEngine<TextAnalysisInput>,
}

impl TextAnalysisWorkflow {
    /// Create a new workflow runner.
    ///
    /// Accepts the LLM service so each skill can request completions,
    /// and an optional config for tuning behaviour.
    pub fn new(
        llm: crate::core::llm::LlmServiceRef,
        _config: Option<TextAnalysisConfig>,
    ) -> Self {
        Self {
            plugin: TextAnalysisPlugin::new(llm),
            engine: WorkflowEngine::new(),
        }
    }

    /// Run the workflow with the given input.
    ///
    /// Returns a [`WorkflowResult`] on success.
    pub async fn run(
        &self,
        input: TextAnalysisInput,
    ) -> Result<super::skills::word_count::WordCountOutput, WorkflowError> {
        // Build the registry from the plugin.
        let mut plugin_ctx = crate::core::plugin::PluginContext::new();
        self.plugin.register(&mut plugin_ctx);
        let registry: SkillRegistry<TextAnalysisInput> = plugin_ctx.into_registry();

        // Create a fresh context for this run.
        let (mut ctx, _cancel) = WorkflowContext::new(input);

        // Execute.
        let result = self.engine.run(&registry, &mut ctx).await?;

        if !result.is_success() {
            return Err(WorkflowError::ContextError(
                result
                    .errors()
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "workflow failed with no specific error".to_string()),
            ));
        }

        // Return the output of the first skill for convenience.
        ctx.get::<super::skills::word_count::WordCountOutput>("word_count")
            .map(|arc| (*arc).clone())
            .ok_or_else(|| {
                WorkflowError::ContextError("word_count output not found".to_string())
            })
    }

    /// Return metadata about this workflow.
    pub fn metadata(&self) -> &'static WorkflowMetadata {
        &WORKFLOW_METADATA
    }
}

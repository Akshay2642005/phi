//! Text-analysis plugin — registers all three skills with the engine.
//!
//! # Usage
//! ```ignore
//! let plugin = TextAnalysisPlugin::new(container.llm.clone());
//! let mut ctx = PluginContext::new();
//! plugin.register(&mut ctx);
//! let registry = ctx.into_registry();
//! ```

use std::sync::Arc;

use crate::core::{
    llm::LlmServiceRef,
    plugin::{Plugin, PluginContext, PluginMetadata},
};
use crate::workflows::text_analysis::{
    TextAnalysisInput,
    skills::{sentiment::SentimentSkill, summary::SummarySkill, word_count::WordCountSkill},
};

// ── Metadata ──────────────────────────────────────────────────────────────────

pub static METADATA: PluginMetadata = PluginMetadata {
    id: "text_analysis",
    name: "Text Analysis",
    description: "Word count → sentiment → summary pipeline.",
    version: "1.0.0",
};

// ── Plugin ────────────────────────────────────────────────────────────────────

/// Plugin that registers the 3-skill text-analysis pipeline.
pub struct TextAnalysisPlugin {
    llm: LlmServiceRef,
}

impl TextAnalysisPlugin {
    pub fn new(llm: LlmServiceRef) -> Self {
        Self { llm }
    }
}

impl Plugin<TextAnalysisInput> for TextAnalysisPlugin {
    fn metadata(&self) -> &'static PluginMetadata {
        &METADATA
    }

    fn register(&self, ctx: &mut PluginContext<TextAnalysisInput>) {
        ctx.register_skill(Arc::new(WordCountSkill));
        ctx.register_skill(SentimentSkill::new(self.llm.clone()));
        ctx.register_skill(SummarySkill::new(self.llm.clone()));
    }
}

//! Skill 3 — Summary
//!
//! Calls the LLM to produce a concise one-sentence summary of the input.
//! Uses both word-count stats and sentiment as additional context.
//!
//! Slot read:    `"word_count"` → [`WordCountOutput`]
//! Slot read:    `"sentiment"`  → [`SentimentOutput`]
//! Slot written: `"summary"`   → [`SummaryOutput`]
//! Dependencies: `["sentiment"]`  (implicitly requires word_count too)

use std::sync::Arc;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::core::{
    context::WorkflowContext,
    llm::{LlmServiceExt as _, LlmServiceRef},
    skill::{Skill, SkillDefinition},
};
use crate::workflows::text_analysis::{
    TextAnalysisInput,
    skills::{sentiment::SentimentOutput, word_count::WordCountOutput},
};

// ── Output type ───────────────────────────────────────────────────────────────

/// Summary produced by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryOutput {
    /// One-sentence summary of the input text.
    pub summary: String,
    /// Key topics identified in the text.
    pub key_topics: Vec<String>,
}

// ── Skill definition ──────────────────────────────────────────────────────────

pub const DEFINITION: SkillDefinition = SkillDefinition {
    id: "summary",
    name: "Text Summary",
    description: "Produces a concise summary and key topics using an LLM.",
    version: "1.0.0",
    dependencies: &["sentiment"], // sentiment already depends on word_count
};

// ── Skill implementation ──────────────────────────────────────────────────────

pub struct SummarySkill {
    pub llm: LlmServiceRef,
}

impl SummarySkill {
    pub fn new(llm: LlmServiceRef) -> Arc<Self> {
        Arc::new(Self { llm })
    }
}

#[async_trait]
impl Skill<TextAnalysisInput> for SummarySkill {
    fn definition(&self) -> &'static SkillDefinition {
        &DEFINITION
    }

    async fn execute(&self, ctx: &mut WorkflowContext<TextAnalysisInput>) -> Result<()> {
        // Read both prerequisite slots
        let wc: Arc<WordCountOutput> = ctx
            .get::<WordCountOutput>("word_count")
            .ok_or_else(|| anyhow!("word_count slot missing"))?;

        let sentiment: Arc<SentimentOutput> = ctx
            .get::<SentimentOutput>("sentiment")
            .ok_or_else(|| anyhow!("sentiment slot missing"))?;

        let text = &ctx.input.text;

        let prompt = format!(
            r#"Summarise the following text in one sentence and identify its key topics.

Context:
- Word count: {}
- Sentiment: {} (confidence: {:.0}%)
- Sentiment reason: {}

Text:
"{}"

Respond with a single JSON object — no markdown, no extra text:
{{
  "summary": "<one sentence summary>",
  "key_topics": ["<topic1>", "<topic2>", "<topic3>"]
}}"#,
            wc.word_count,
            sentiment.sentiment,
            sentiment.confidence * 100.0,
            sentiment.reason,
            text
        );

        tracing::debug!(skill = DEFINITION.id, "Calling LLM for summary");

        let output: SummaryOutput = self.llm.prompt_typed(&prompt).await?;

        tracing::info!(
            skill = DEFINITION.id,
            topics = ?output.key_topics,
            "Summary complete"
        );

        ctx.set(DEFINITION.id, output);
        Ok(())
    }
}

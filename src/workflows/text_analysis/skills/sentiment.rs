//! Skill 2 — Sentiment Analysis
//!
//! Calls the LLM to classify the tone of the input text.  Uses the word-count
//! output as context so the model can calibrate its response.
//!
//! Slot read:    `"word_count"` → [`WordCountOutput`]
//! Slot written: `"sentiment"` → [`SentimentOutput`]
//! Dependencies: `["word_count"]`

use std::sync::Arc;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::core::{
    context::WorkflowContext,
    llm::{LlmServiceExt as _, LlmServiceRef},
    skill::{Skill, SkillDefinition},
};
use crate::workflows::text_analysis::{TextAnalysisInput, skills::word_count::WordCountOutput};

// ── Output type ───────────────────────────────────────────────────────────────

/// Sentiment classification produced by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentimentOutput {
    /// Overall sentiment: "positive", "negative", or "neutral".
    pub sentiment: String,
    /// Confidence score between 0.0 and 1.0.
    pub confidence: f32,
    /// One-sentence reason for the classification.
    pub reason: String,
}

// ── Skill definition ──────────────────────────────────────────────────────────

pub const DEFINITION: SkillDefinition = SkillDefinition {
    id: "sentiment",
    name: "Sentiment Analysis",
    description: "Classifies the emotional tone of the text using an LLM.",
    version: "1.0.0",
    dependencies: &["word_count"],
};

// ── Skill implementation ──────────────────────────────────────────────────────

pub struct SentimentSkill {
    pub llm: LlmServiceRef,
}

impl SentimentSkill {
    pub fn new(llm: LlmServiceRef) -> Arc<Self> {
        Arc::new(Self { llm })
    }
}

#[async_trait]
impl Skill<TextAnalysisInput> for SentimentSkill {
    fn definition(&self) -> &'static SkillDefinition {
        &DEFINITION
    }

    async fn execute(&self, ctx: &mut WorkflowContext<TextAnalysisInput>) -> Result<()> {
        // Read prerequisite output from slot map
        let wc: Arc<WordCountOutput> = ctx
            .get::<WordCountOutput>("word_count")
            .ok_or_else(|| anyhow!("word_count slot missing — dependency not satisfied"))?;

        let text = &ctx.input.text;

        let prompt = format!(
            r#"Analyse the sentiment of the following text.

Text ({} words, {} sentences):
"{}"

Respond with a single JSON object — no markdown, no extra text:
{{
  "sentiment": "<positive|negative|neutral>",
  "confidence": <0.0-1.0>,
  "reason": "<one sentence explanation>"
}}"#,
            wc.word_count, wc.sentence_count, text
        );

        tracing::debug!(skill = DEFINITION.id, "Calling LLM for sentiment");

        let output: SentimentOutput = self.llm.prompt_typed(&prompt).await?;

        tracing::info!(
            skill = DEFINITION.id,
            sentiment = %output.sentiment,
            confidence = output.confidence,
            "Sentiment analysis complete"
        );

        ctx.set(DEFINITION.id, output);
        Ok(())
    }
}

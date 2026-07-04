//! Analysis skill adapters for `RuntimeInput`.
//!
//! The text-analysis skills (word_count, sentiment, summary) were written for
//! `TextAnalysisInput`. The runtime's domain input is `RuntimeInput`. Rather
//! than duplicating the logic, we implement slim wrapper skills that pull the
//! text from whichever slot has content (web_search result or the goal itself).

use std::sync::Arc;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::core::{
    context::WorkflowContext,
    llm::{LlmServiceExt as _, LlmServiceRef},
    skill::{Skill, SkillDefinition, SkillRef},
};
use crate::runtime::RuntimeInput;

// ── Re-export output types so plugin/react_loop can reference them ────────────
pub use crate::workflows::text_analysis::skills::{
    sentiment::SentimentOutput, summary::SummaryOutput, word_count::WordCountOutput,
};

// ── Shared definitions ────────────────────────────────────────────────────────

pub const WORD_COUNT_DEF: SkillDefinition = SkillDefinition {
    id: "word_count",
    name: "Word Count",
    description: "Counts words, sentences, and characters in the fetched or input text.",
    version: "1.0.0",
    dependencies: &[],
};

pub const SENTIMENT_DEF: SkillDefinition = SkillDefinition {
    id: "sentiment",
    name: "Sentiment Analysis",
    description: "Classifies the emotional tone of the text using an LLM.",
    version: "1.0.0",
    dependencies: &["word_count"],
};

pub const SUMMARY_DEF: SkillDefinition = SkillDefinition {
    id: "summary",
    name: "Text Summary",
    description: "Produces a concise one-sentence summary and key topics using an LLM.",
    version: "1.0.0",
    dependencies: &["sentiment"],
};

// ── Helper: pick the best text to analyse ────────────────────────────────────

fn pick_text(ctx: &WorkflowContext<RuntimeInput>) -> String {
    if let Some(ws) = ctx.get::<crate::tools::web_search::WebSearchOutput>("web_search")
        && !ws.abstract_text.is_empty()
    {
        return ws.as_context();
    }
    ctx.input.goal.clone()
}

// ── Word Count ────────────────────────────────────────────────────────────────

pub struct RuntimeWordCountSkill;

#[async_trait]
impl Skill<RuntimeInput> for RuntimeWordCountSkill {
    fn definition(&self) -> &'static SkillDefinition {
        &WORD_COUNT_DEF
    }

    async fn execute(&self, ctx: &mut WorkflowContext<RuntimeInput>) -> Result<()> {
        let text = pick_text(ctx);

        let word_count = text.split_whitespace().count();
        let sentence_count = text
            .split(['.', '!', '?'])
            .filter(|s| !s.trim().is_empty())
            .count()
            .max(1);
        let char_count = text.chars().filter(|c| !c.is_whitespace()).count();
        let total_len: usize = text
            .split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphabetic()).len())
            .sum();
        let avg_word_length = if word_count > 0 {
            total_len as f32 / word_count as f32
        } else {
            0.0
        };

        let output = WordCountOutput {
            word_count,
            sentence_count,
            char_count,
            avg_word_length,
        };

        tracing::info!(
            skill = WORD_COUNT_DEF.id,
            words = output.word_count,
            "Word count complete"
        );

        ctx.set(WORD_COUNT_DEF.id, output);
        Ok(())
    }
}

pub fn word_count_skill() -> SkillRef<RuntimeInput> {
    Arc::new(RuntimeWordCountSkill)
}

// ── Sentiment ─────────────────────────────────────────────────────────────────

pub struct RuntimeSentimentSkill {
    llm: LlmServiceRef,
}

#[async_trait]
impl Skill<RuntimeInput> for RuntimeSentimentSkill {
    fn definition(&self) -> &'static SkillDefinition {
        &SENTIMENT_DEF
    }

    async fn execute(&self, ctx: &mut WorkflowContext<RuntimeInput>) -> Result<()> {
        let wc: Arc<WordCountOutput> = ctx
            .get::<WordCountOutput>("word_count")
            .ok_or_else(|| anyhow!("word_count slot missing"))?;

        let text = pick_text(ctx);

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

        let output: SentimentOutput = self.llm.prompt_typed(&prompt).await?;

        tracing::info!(
            skill = SENTIMENT_DEF.id,
            sentiment = %output.sentiment,
            "Sentiment complete"
        );

        ctx.set(SENTIMENT_DEF.id, output);
        Ok(())
    }
}

pub fn sentiment_skill(llm: LlmServiceRef) -> SkillRef<RuntimeInput> {
    Arc::new(RuntimeSentimentSkill { llm })
}

// ── Summary ───────────────────────────────────────────────────────────────────

pub struct RuntimeSummarySkill {
    llm: LlmServiceRef,
}

#[async_trait]
impl Skill<RuntimeInput> for RuntimeSummarySkill {
    fn definition(&self) -> &'static SkillDefinition {
        &SUMMARY_DEF
    }

    async fn execute(&self, ctx: &mut WorkflowContext<RuntimeInput>) -> Result<()> {
        let wc: Arc<WordCountOutput> = ctx
            .get::<WordCountOutput>("word_count")
            .ok_or_else(|| anyhow!("word_count slot missing"))?;
        let sentiment: Arc<SentimentOutput> = ctx
            .get::<SentimentOutput>("sentiment")
            .ok_or_else(|| anyhow!("sentiment slot missing"))?;

        let text = pick_text(ctx);

        let prompt = format!(
            r#"Summarise the following text in one sentence and identify its key topics.

Context: {} words, sentiment: {} (confidence {:.0}%).
Reason: {}

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

        let output: SummaryOutput = self.llm.prompt_typed(&prompt).await?;

        tracing::info!(
            skill = SUMMARY_DEF.id,
            topics = ?output.key_topics,
            "Summary complete"
        );

        ctx.set(SUMMARY_DEF.id, output);
        Ok(())
    }
}

pub fn summary_skill(llm: LlmServiceRef) -> SkillRef<RuntimeInput> {
    Arc::new(RuntimeSummarySkill { llm })
}

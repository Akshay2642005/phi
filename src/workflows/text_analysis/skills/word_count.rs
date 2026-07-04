//! Skill 1 — Word Count
//!
//! Pure-Rust skill: no LLM call.  Reads `ctx.input.text`, counts words /
//! sentences / characters, and writes a [`WordCountOutput`] to the slot map.
//!
//! Slot written: `"word_count"` → [`WordCountOutput`]
//! Dependencies: none (runs first)

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::core::{
    context::WorkflowContext,
    skill::{Skill, SkillDefinition},
};
use crate::workflows::text_analysis::TextAnalysisInput;

// ── Output type ───────────────────────────────────────────────────────────────

/// Statistics produced by the word-count skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordCountOutput {
    pub word_count: usize,
    pub sentence_count: usize,
    pub char_count: usize,
    pub avg_word_length: f32,
}

// ── Skill definition ──────────────────────────────────────────────────────────

pub const DEFINITION: SkillDefinition = SkillDefinition {
    id: "word_count",
    name: "Word Count",
    description: "Counts words, sentences, and characters in the input text.",
    version: "1.0.0",
    dependencies: &[], // no prerequisites — runs immediately
};

// ── Skill implementation ──────────────────────────────────────────────────────

pub struct WordCountSkill;

#[async_trait]
impl Skill<TextAnalysisInput> for WordCountSkill {
    fn definition(&self) -> &'static SkillDefinition {
        &DEFINITION
    }

    async fn execute(&self, ctx: &mut WorkflowContext<TextAnalysisInput>) -> Result<()> {
        let text = &ctx.input.text;

        // Word count: split on whitespace
        let word_count = text.split_whitespace().count();

        // Sentence count: split on '.', '!', '?' — filter empties
        let sentence_count = text
            .split(['.', '!', '?'])
            .filter(|s| !s.trim().is_empty())
            .count()
            .max(1); // at least 1 sentence

        // Character count (excluding whitespace)
        let char_count = text.chars().filter(|c| !c.is_whitespace()).count();

        // Average word length
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
            skill = DEFINITION.id,
            words = output.word_count,
            sentences = output.sentence_count,
            chars = output.char_count,
            "Word count complete"
        );

        ctx.set(DEFINITION.id, output);
        Ok(())
    }
}

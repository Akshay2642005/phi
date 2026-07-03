/*!
Text analysis workflow — analyses a piece of text through a 3-skill pipeline:

1. `word_count`  — pure Rust: counts words, sentences, characters (no LLM).
2. `sentiment`   — LLM call: classifies tone using word-count context.
3. `summary`     — LLM call: produces a one-sentence summary informed by both word count and sentiment.
*/

pub mod plugin;
pub mod skills;

// ── Domain input ──────────────────────────────────────────────────────────────

/// The immutable input handed to every skill in this workflow.
#[derive(Debug, Clone)]
pub struct TextAnalysisInput {
    /// A short label used in log output so you can identify the run.
    pub label: String,
    /// The text to analyse.
    pub text: String,
}

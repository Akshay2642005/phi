/*!
Text analysis workflow — analyses a piece of text through a 3-skill pipeline:

1. `word_count`  — pure Rust: counts words, sentences, characters (no LLM).
2. `sentiment`   — LLM call: classifies tone using word-count context.
3. `summary`     — LLM call: produces a one-sentence summary informed by both
   word count and sentiment.

# Package layout
```
text_analysis/
├── mod.rs        — TextAnalysisInput + re-exports
├── config.rs     — TextAnalysisConfig (tunable parameters)
├── plugin.rs     — TextAnalysisPlugin (registers skills with the engine)
└── skills/
    ├── mod.rs
    ├── word_count.rs
    ├── sentiment.rs
    └── summary.rs
```
*/

pub mod config;
pub mod plugin;
pub mod skills;
pub mod workflow;

pub use config::TextAnalysisConfig;
pub use workflow::TextAnalysisWorkflow;

// ── Domain input ──────────────────────────────────────────────────────────────

/// The immutable input handed to every skill in this workflow.
#[derive(Debug, Clone)]
pub struct TextAnalysisInput {
    /// A short label used in log output so you can identify the run.
    pub label: String,
    /// The text to analyse.
    pub text: String,
}

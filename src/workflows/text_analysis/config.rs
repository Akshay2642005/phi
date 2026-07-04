//! Text analysis workflow configuration.
//!
//! [`TextAnalysisConfig`] controls tunable parameters for the text-analysis
//! pipeline.  All fields have sensible defaults so callers only need to
//! override what they care about.
//!
//! # Usage
//! ```ignore
//! let config = TextAnalysisConfig {
//!     max_summary_sentences: 2,
//!     ..TextAnalysisConfig::default()
//! };
//! ```
//!
//! The config is provided to the plugin at construction time and forwarded to
//! each skill that needs it.

use serde::{Deserialize, Serialize};

// ── Config ────────────────────────────────────────────────────────────────────

/// Tunable parameters for the text-analysis workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextAnalysisConfig {
    /// Maximum number of sentences in the summary output.
    ///
    /// The LLM is instructed to stay within this limit; it is a soft
    /// constraint (the model may occasionally exceed it).
    pub max_summary_sentences: usize,

    /// Number of key topics to extract.
    pub key_topic_count: usize,

    /// Minimum confidence threshold for sentiment classification.
    ///
    /// Results below this threshold will carry a warning diagnostic.
    /// Range: 0.0 – 1.0.
    pub sentiment_confidence_threshold: f32,

    /// Whether to include word-count statistics in the final output.
    pub include_word_stats: bool,
}

impl Default for TextAnalysisConfig {
    fn default() -> Self {
        Self {
            max_summary_sentences: 1,
            key_topic_count: 3,
            sentiment_confidence_threshold: 0.6,
            include_word_stats: true,
        }
    }
}

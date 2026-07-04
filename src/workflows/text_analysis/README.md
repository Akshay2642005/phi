# Text Analysis Workflow

A three-skill text analysis pipeline: **word count → sentiment → summary**.

## Skills

| Skill | ID | Dependencies | Description |
|-------|----|-------------|-------------|
| Word Count | `word_count` | — | Pure Rust: counts words, sentences, characters |
| Sentiment | `sentiment` | `word_count` | LLM: classifies tone (positive/negative/neutral) |
| Summary | `summary` | `sentiment` | LLM: produces one-sentence summary + key topics |

## Flow

```
TextAnalysisInput { label, text }
        │
        ▼
  WordCountSkill  ──► WordCountOutput { word_count, sentence_count, ... }
        │
        ▼
  SentimentSkill  ──► SentimentOutput { sentiment, confidence, reason }
        │
        ▼
  SummarySkill    ──► SummaryOutput { summary, key_topics }
```

## Input

```rust
pub struct TextAnalysisInput {
    pub label: String,  // short identifier for logging
    pub text: String,   // the text to analyse
}
```

## Configuration

See `config.rs` for tunable parameters (`max_summary_sentences`, `key_topic_count`, etc.).

## Plugin

Register via `TextAnalysisPlugin`. The plugin registers all three skills
with their dependency edges.

## Portability

This directory is a self-contained workflow module. Copy it into any
AI Workflow Engine project and register its plugin:

```rust
use text_analysis::plugin::TextAnalysisPlugin;

let plugin = TextAnalysisPlugin::new(llm);
let mut ctx = PluginContext::new();
plugin.register(&mut ctx);
let registry = ctx.into_registry();
```

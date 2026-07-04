//! Tool skill — Web Search
//!
//! Calls the DuckDuckGo Instant Answer API (no API key required) to search
//! the web for information related to the worker's goal.
//!
//! Slot read:    `ctx.input.goal`    — the search query comes from the goal
//! Slot written: `"web_search"`      → [`WebSearchOutput`]
//! Dependencies: none (can run immediately)
//!
//! # DuckDuckGo API
//! `GET https://api.duckduckgo.com/?q=<query>&format=json&no_html=1&skip_disambig=1`
//!
//! Returns an instant-answer JSON blob. We extract:
//! - `AbstractText`  — a short paragraph summary (Wikipedia-backed)
//! - `AbstractURL`   — canonical source URL
//! - `RelatedTopics` — list of related result snippets
//!
//! This is a free, no-auth API best suited for factual look-ups. For deeper
//! web search (SERPs), swap in Brave Search API or SerpAPI with an env-var key.

use std::sync::Arc;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::core::{
    context::WorkflowContext,
    skill::{Skill, SkillDefinition},
};
use crate::runtime::RuntimeInput;

// ── API response shapes ───────────────────────────────────────────────────────

/// Raw DuckDuckGo API response (only the fields we care about).
#[derive(Debug, Deserialize)]
struct DdgResponse {
    #[serde(rename = "AbstractText")]
    abstract_text: String,
    #[serde(rename = "AbstractURL")]
    abstract_url: String,
    #[serde(rename = "AbstractSource")]
    abstract_source: String,
    #[serde(rename = "RelatedTopics")]
    related_topics: Vec<DdgRelatedTopic>,
}

#[derive(Debug, Deserialize)]
struct DdgRelatedTopic {
    #[serde(rename = "Text", default)]
    text: String,
    #[serde(rename = "FirstURL", default)]
    first_url: String,
}

// ── Output type ───────────────────────────────────────────────────────────────

/// Result written to the `"web_search"` context slot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchOutput {
    /// The search query that was issued.
    pub query: String,
    /// Short abstract paragraph from the top result.
    pub abstract_text: String,
    /// URL of the top result.
    pub abstract_url: String,
    /// Source name (e.g. "Wikipedia").
    pub abstract_source: String,
    /// Related snippets (up to 5).
    pub related: Vec<RelatedResult>,
}

/// A single related search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedResult {
    pub text: String,
    pub url: String,
}

impl WebSearchOutput {
    /// Format the search results into a compact string suitable for feeding
    /// back into an LLM prompt.
    pub fn as_context(&self) -> String {
        let mut parts = Vec::new();

        if !self.abstract_text.is_empty() {
            parts.push(format!(
                "**{}** ({})\n{}",
                self.abstract_source, self.abstract_url, self.abstract_text
            ));
        }

        if !self.related.is_empty() {
            let related: Vec<String> = self
                .related
                .iter()
                .take(3)
                .map(|r| format!("- {} ({})", r.text, r.url))
                .collect();
            parts.push(format!("Related:\n{}", related.join("\n")));
        }

        if parts.is_empty() {
            format!("No results found for query: {}", self.query)
        } else {
            parts.join("\n\n")
        }
    }
}

// ── Skill definition ──────────────────────────────────────────────────────────

pub const DEFINITION: SkillDefinition = SkillDefinition {
    id: "web_search",
    name: "Web Search",
    description: "Searches the web for information using DuckDuckGo Instant Answers.",
    version: "1.0.0",
    dependencies: &[],
};

// ── Skill implementation ──────────────────────────────────────────────────────

pub struct WebSearchSkill {
    client: Client,
}

impl WebSearchSkill {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            client: Client::builder()
                .user_agent("rust-agent/1.0 (digital worker demo)")
                .build()
                .expect("failed to build reqwest client"),
        })
    }
}

#[async_trait]
impl Skill<RuntimeInput> for WebSearchSkill {
    fn definition(&self) -> &'static SkillDefinition {
        &DEFINITION
    }

    async fn execute(&self, ctx: &mut WorkflowContext<RuntimeInput>) -> Result<()> {
        let query = &ctx.input.goal;

        tracing::info!(skill = DEFINITION.id, query = %query, "Searching the web");

        let url = format!(
            "https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1",
            urlencoding(query)
        );

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow!("web search HTTP request failed: {e}"))?;

        let ddg: DdgResponse = resp
            .json()
            .await
            .map_err(|e| anyhow!("failed to parse DuckDuckGo response: {e}"))?;

        let related: Vec<RelatedResult> = ddg
            .related_topics
            .into_iter()
            .filter(|t| !t.text.is_empty())
            .take(5)
            .map(|t| RelatedResult {
                text: t.text,
                url: t.first_url,
            })
            .collect();

        let output = WebSearchOutput {
            query: query.clone(),
            abstract_text: ddg.abstract_text,
            abstract_url: ddg.abstract_url,
            abstract_source: ddg.abstract_source,
            related,
        };

        tracing::info!(
            skill = DEFINITION.id,
            has_abstract = !output.abstract_text.is_empty(),
            related_count = output.related.len(),
            "Web search complete"
        );

        ctx.set(DEFINITION.id, output);
        Ok(())
    }
}

// ── URL encoding helper ───────────────────────────────────────────────────────

/// Minimal percent-encoding for query strings (spaces → `+`, special chars → `%XX`).
fn urlencoding(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ' ' => "+".to_string(),
            c if c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') => c.to_string(),
            c => {
                let mut buf = [0u8; 4];
                let bytes = c.encode_utf8(&mut buf).as_bytes();
                bytes.iter().map(|b| format!("%{:02X}", b)).collect()
            }
        })
        .collect()
}

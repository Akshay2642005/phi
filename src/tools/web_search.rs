//! Tool skill — Web Search
//!
//! Calls the Tavily Search API (https://tavily.com) — a search API designed
//! for AI agents. Returns clean JSON with full-text snippets and an optional
//! AI-generated answer, so the planner gets real content to reason over.
//!
//! Requires a free Tavily API key in the `TAVILY_API_KEY` env var
//! (1,000 credits/month free tier — 1 credit per basic search).
//!
//! Slot read:    `ctx.input.goal`    — the search query comes from the goal
//! Slot written: `"web_search"`      → [`WebSearchOutput`]
//! Dependencies: none (can run immediately)
//!
//! # Tavily API
//! `POST https://api.tavily.com/search` with a JSON body:
//! `{ "api_key", "query", "search_depth": "basic", "include_answer": true, "max_results": 5 }`
//!
//! Returns an instant-answer JSON blob. We extract:
//! - `answer`        — AI-generated answer when `include_answer` is true
//! - `results[]`     — list of { title, url, content } result snippets
//!
//! The top result becomes the "abstract"; the rest are `related` results.

use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::core::{
    context::WorkflowContext,
    skill::{Skill, SkillDefinition},
};
use crate::runtime::RuntimeInput;

#[derive(Debug, Deserialize)]
struct TavilyResponse {
    #[serde(default)]
    answer: String,
    #[serde(default)]
    results: Vec<TavilyResult>,
}

#[derive(Debug, Deserialize)]
struct TavilyResult {
    title: String,
    url: String,
    content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchOutput {
    pub query: String,
    pub abstract_text: String,
    pub abstract_url: String,
    pub abstract_source: String,
    pub related: Vec<RelatedResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedResult {
    pub text: String,
    pub url: String,
}

impl WebSearchOutput {
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

pub const DEFINITION: SkillDefinition = SkillDefinition {
    id: "web_search",
    name: "Web Search",
    description: "Searches the web for information using the Tavily Search API.",
    version: "2.0.0",
    dependencies: &[],
};

pub struct WebSearchSkill {
    client: Client,
    api_key: String,
}

impl WebSearchSkill {
    pub fn new(api_key: String) -> Arc<Self> {
        Arc::new(Self {
            client: Client::builder()
                .user_agent("rust-agent/1.0 (digital worker demo)")
                .build()
                .expect("failed to build reqwest client"),
            api_key,
        })
    }
}

#[async_trait]
impl Skill<RuntimeInput> for WebSearchSkill {
    fn definition(&self) -> &'static SkillDefinition {
        &DEFINITION
    }

    async fn execute(&self, ctx: &mut WorkflowContext<RuntimeInput>) -> Result<()> {
        if self.api_key.is_empty() {
            bail!(
                "TAVILY_API_KEY not set — get a free API key at https://tavily.com \
                 and add it to your environment"
            );
        }

        let query = ctx
            .get::<String>(DEFINITION.id)
            .map(|s| s.as_ref().clone())
            .unwrap_or_else(|| ctx.input.goal.clone());

        tracing::info!(skill = DEFINITION.id, query = %query, "Searching the web");

        let payload = serde_json::json!({
            "api_key": self.api_key,
            "query": query,
            "search_depth": "basic",
            "include_answer": true,
            "max_results": 5,
        });

        let resp = self
            .client
            .post("https://api.tavily.com/search")
            .json(&payload)
            .send()
            .await
            .map_err(|e| anyhow!("web search HTTP request failed: {e}"))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            bail!("Tavily API returned HTTP {status}: {body}");
        }

        let tavily: TavilyResponse = resp
            .json()
            .await
            .map_err(|e| anyhow!("failed to parse Tavily response: {e}"))?;

        let output = build_output(&query, &tavily);
        let display = display_for(&query, &tavily, &output);

        tracing::info!(
            skill = DEFINITION.id,
            has_abstract = !output.abstract_text.is_empty(),
            related_count = output.related.len(),
            "Web search complete"
        );

        ctx.set_with_display(DEFINITION.id, output, display);
        Ok(())
    }
}

/// Map a parsed Tavily response to the public output shape.
///
/// Pure function — no network, no context — so the mapping rules are
/// unit-testable offline. The `answer` field wins over the top result's
/// content; the remaining results (max 5) become `related`.
fn build_output(query: &str, tavily: &TavilyResponse) -> WebSearchOutput {
    match tavily.results.split_first() {
        Some((top, rest)) => WebSearchOutput {
            query: query.to_string(),
            abstract_text: if tavily.answer.is_empty() {
                top.content.clone()
            } else {
                tavily.answer.clone()
            },
            abstract_url: top.url.clone(),
            abstract_source: top.title.clone(),
            related: rest
                .iter()
                .take(5)
                .map(|r| RelatedResult {
                    text: r.content.clone(),
                    url: r.url.clone(),
                })
                .collect(),
        },
        None => WebSearchOutput {
            query: query.to_string(),
            abstract_text: tavily.answer.clone(),
            abstract_url: String::new(),
            abstract_source: "Tavily".to_string(),
            related: Vec::new(),
        },
    }
}

/// Human-readable one-liner for the context display slot.
fn display_for(query: &str, tavily: &TavilyResponse, output: &WebSearchOutput) -> String {
    if !output.abstract_text.is_empty() {
        if tavily.answer.is_empty() {
            format!(
                "web_search for '{query}': found result from {}",
                output.abstract_source
            )
        } else {
            format!("web_search for '{query}': {}", output.abstract_text)
        }
    } else {
        format!("web_search for '{query}': no results")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::context::WorkflowContext;

    fn fixture_answer(query: &str) -> TavilyResponse {
        TavilyResponse {
            answer: format!("The {query} is 300m tall."),
            results: vec![],
        }
    }

    fn fixture_results() -> TavilyResponse {
        let results: Vec<TavilyResult> = (1..=6)
            .map(|i| TavilyResult {
                title: format!("Source {i}"),
                url: format!("https://example.com/{i}"),
                content: format!("Content snippet {i}"),
            })
            .collect();
        TavilyResponse {
            answer: String::new(),
            results,
        }
    }

    #[test]
    fn answer_wins_over_top_result_content() {
        let mut response = fixture_results();
        response.answer = "The Eiffel Tower is 330m tall.".to_string();

        let output = build_output("Eiffel Tower", &response);

        assert_eq!(output.abstract_text, "The Eiffel Tower is 330m tall.");
        assert_eq!(output.abstract_url, "https://example.com/1");
        assert_eq!(output.abstract_source, "Source 1");
    }

    #[test]
    fn top_result_content_becomes_abstract_without_answer() {
        let response = fixture_results();

        let output = build_output("Eiffel Tower", &response);

        assert_eq!(output.abstract_text, "Content snippet 1");
        assert_eq!(output.abstract_url, "https://example.com/1");
        assert_eq!(output.abstract_source, "Source 1");
    }

    #[test]
    fn related_keeps_max_five_results() {
        let response = fixture_results();

        let output = build_output("Eiffel Tower", &response);

        assert_eq!(output.related.len(), 5, "excess results beyond top are capped at 5");
        assert_eq!(output.related[0].text, "Content snippet 2");
        assert_eq!(output.related[4].url, "https://example.com/6");
    }

    #[test]
    fn no_results_but_answer_still_surfaces() {
        let response = fixture_answer("Eiffel Tower");

        let output = build_output("Eiffel Tower", &response);

        assert_eq!(output.abstract_text, "The Eiffel Tower is 300m tall.");
        assert_eq!(output.abstract_source, "Tavily");
        assert!(output.abstract_url.is_empty());
        assert!(output.related.is_empty());
    }

    #[test]
    fn no_results_no_answer_is_empty() {
        let response = TavilyResponse {
            answer: String::new(),
            results: vec![],
        };

        let output = build_output("Nothing here", &response);

        assert!(output.abstract_text.is_empty());
        assert!(output.abstract_url.is_empty());
        assert!(output.related.is_empty());
    }

    #[test]
    fn display_reflects_source_and_answer() {
        let answer = fixture_answer("Eiffel Tower");
        let output = build_output("Eiffel Tower", &answer);
        assert_eq!(
            display_for("Eiffel Tower", &answer, &output),
            "web_search for 'Eiffel Tower': The Eiffel Tower is 300m tall."
        );

        let results = fixture_results();
        let output = build_output("Eiffel Tower", &results);
        assert_eq!(
            display_for("Eiffel Tower", &results, &output),
            "web_search for 'Eiffel Tower': found result from Source 1"
        );

        let empty = TavilyResponse {
            answer: String::new(),
            results: vec![],
        };
        let output = build_output("Nothing here", &empty);
        assert_eq!(
            display_for("Nothing here", &empty, &output),
            "web_search for 'Nothing here': no results"
        );
    }

    #[tokio::test]
    async fn empty_key_bails_before_any_request() {
        let skill = WebSearchSkill::new(String::new());
        let input = RuntimeInput {
            goal: "Eiffel Tower height".to_string(),
            context: None,
        };
        let (mut ctx, _tx) = WorkflowContext::new(input);

        let result = skill.execute(&mut ctx).await;

        let err = result.expect_err("empty API key must fail");
        assert!(
            err.to_string().contains("TAVILY_API_KEY"),
            "error should mention the missing env var, got: {err}"
        );
        assert!(ctx.get::<WebSearchOutput>(DEFINITION.id).is_none());
    }
}

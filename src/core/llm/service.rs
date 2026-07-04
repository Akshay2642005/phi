//! LLM service abstraction layer.
//!
//! The engine and all skills depend only on [`LlmService`]; they never
//! reference any concrete provider (Ollama, OpenAI, Anthropic, etc.).
//!
//! # Design
//! - [`LlmService`] is the **dyn-compatible** base trait — only `prompt`
//!   lives here so it can be stored as `Arc<dyn LlmService>`.
//! - [`LlmServiceExt`] is a **blanket extension trait** that adds the
//!   generic `prompt_typed<T>` helper to every `LlmService` implementor.
//!   Skills call it via `self.llm.prompt_typed(…)` without needing a type
//!   parameter on the stored `Arc`.
//! - [`OllamaLlmService`] is the sole concrete implementation.
//! - [`LlmServiceRef`] is `Arc<dyn LlmService>` — cheap to clone, store, and
//!   pass across tasks.

use std::sync::Arc;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use rig::{client::CompletionClient, completion::Prompt};
use serde::de::DeserializeOwned;

use crate::core::errors::SkillError;

// ── Public type alias ─────────────────────────────────────────────────────────

/// A cheaply-cloneable reference to any [`LlmService`] implementation.
///
/// Use this everywhere: in skills, in `AppContainer`, in the bootstrap layer.
pub type LlmServiceRef = Arc<dyn LlmService>;

// ── Base trait (dyn-compatible) ───────────────────────────────────────────────

/// Provider-agnostic interface for LLM text completion.
///
/// This trait is **dyn-compatible**: it contains only `prompt`, which has no
/// generic parameters.  Store it as `Arc<dyn LlmService>`.
///
/// For the typed variant (`prompt_typed<T>`), use [`LlmServiceExt`] which is
/// automatically available on any `&dyn LlmService` and `Arc<dyn LlmService>`
/// through a blanket impl.
#[async_trait]
pub trait LlmService: Send + Sync {
    /// Send `prompt` to the model and return the raw text response.
    async fn prompt(&self, prompt: &str) -> Result<String>;
}

// ── Extension trait (blanket, NOT dyn) ───────────────────────────────────────

/// Adds `prompt_typed<T>` to any type that implements [`LlmService`].
///
/// Because this method is generic, it cannot live in the dyn-compatible
/// `LlmService` trait.  The blanket impl below makes it available everywhere
/// a concrete or trait-object reference to an `LlmService` is held.
///
/// # Usage in skills
/// ```ignore
/// use crate::core::llm::LlmServiceExt as _;
///
/// let output: MyOutput = self.llm.prompt_typed(&prompt).await?;
/// ```
pub trait LlmServiceExt: LlmService {
    /// Send `prompt`, extract the first JSON object/array from the response,
    /// and deserialise it as `T`.
    fn prompt_typed<T>(&self, prompt: &str) -> impl std::future::Future<Output = Result<T>> + Send
    where
        T: DeserializeOwned + Send,
    {
        async move {
            let response = self.prompt(prompt).await?;

            tracing::debug!(raw_response = %response, "LLM raw response");

            let json =
                extract_json(&response).map_err(|e| SkillError::LlmCallFailed(anyhow!("{}", e)))?;

            tracing::debug!(extracted_json = %json, "Extracted JSON from LLM response");

            serde_json::from_str(json)
                .map_err(SkillError::OutputDeserializationFailed)
                .map_err(|e| anyhow!(e))
        }
    }
}

/// Blanket impl: every `LlmService` implementor automatically gets
/// `LlmServiceExt` without any extra code.
impl<T: LlmService + ?Sized> LlmServiceExt for T {}

// ── Concrete implementation — Ollama ──────────────────────────────────────────

/// Ollama-backed [`LlmService`] implementation using the `rig` crate.
pub struct OllamaLlmService {
    client: rig::providers::ollama::Client,
    model: String,
}

impl OllamaLlmService {
    /// Create a new `OllamaLlmService` from an Ollama client and model name.
    pub fn new(client: rig::providers::ollama::Client, model: impl Into<String>) -> Self {
        Self {
            client,
            model: model.into(),
        }
    }
}

#[async_trait]
impl LlmService for OllamaLlmService {
    async fn prompt(&self, prompt: &str) -> Result<String> {
        let agent = self.client.agent(&self.model).build();
        Ok(agent.prompt(prompt).await?)
    }
}

// ── JSON extraction helper ────────────────────────────────────────────────────

/// Extract the first `{ … }` JSON object (or `[ … ]` array) from `text`.
fn extract_json(text: &str) -> Result<&str> {
    for (open, close) in [('{', '}'), ('[', ']')] {
        let Some(start) = text.find(open) else {
            continue;
        };
        let Some(end) = text.rfind(close) else {
            continue;
        };
        if end >= start {
            return Ok(&text[start..=end]);
        }
    }
    Err(anyhow!("no JSON object or array found in LLM response"))
}

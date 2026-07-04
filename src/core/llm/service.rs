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

pub type LlmServiceRef = Arc<dyn LlmService>;

#[async_trait]
pub trait LlmService: Send + Sync {
    async fn prompt(&self, prompt: &str) -> Result<String>;
}

pub trait LlmServiceExt: LlmService {
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

impl<T: LlmService + ?Sized> LlmServiceExt for T {}

pub struct OllamaLlmService {
    client: rig::providers::ollama::Client,
    model: String,
}

impl OllamaLlmService {
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

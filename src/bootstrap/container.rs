//! Application container — constructs all long-lived services at startup.
//!
//! `AppContainer` is built once in `main` and shared via `Arc`. It owns:
//! - Application configuration.
//! - The Rig/Ollama runtime.
//! - The `LlmServiceRef` (`Arc<dyn LlmService>`) used by every skill.

use std::sync::Arc;

use rig::providers::ollama;

use crate::{
    core::llm::{LlmServiceRef, OllamaLlmService},
    setup::{config::AppConfig, rig::RigRuntime},
};

pub struct AppContainer {
    pub config: Arc<AppConfig>,
    pub runtime: Arc<RigRuntime>,
    pub llm: LlmServiceRef,
}

impl AppContainer {
    pub async fn build() -> anyhow::Result<Self> {
        let config = AppConfig::from_vault().await?;
        let runtime = Arc::new(RigRuntime::new(
            config.ollama_api_key.clone(),
            config.ollama_base_url.clone(),
            config.ollama_model.clone(),
        )?);
        let client = ollama::Client::builder()
            .api_key(config.ollama_api_key.clone())
            .base_url(config.ollama_base_url.clone())
            .build()?;
        let llm: LlmServiceRef =
            Arc::new(OllamaLlmService::new(client, config.ollama_model.clone()));
        Ok(Self {
            config: Arc::new(config),
            runtime,
            llm,
        })
    }
}

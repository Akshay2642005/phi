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
        // Try Vault-backed config first, fall back to direct env vars.
        let config = match AppConfig::from_vault().await {
            Ok(cfg) => cfg,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "Vault config failed — falling back to direct env vars"
                );
                AppConfig::from_env()?
            }
        };
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

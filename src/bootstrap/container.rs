//! Application container — constructs all long-lived services at startup.
//!
//! `AppContainer` is built once in `main` and shared via `Arc`. It owns:
//! - Application configuration.
//! - The Rig/Ollama runtime.
//! - The `LlmServiceRef` (`Arc<dyn LlmService>`) used by every skill.

use std::sync::Arc;

use crate::{
    core::llm::{LlmServiceRef, OllamaLlmService},
    setup::{config::AppConfig, rig::RigRuntime},
};

/// All long-lived, shared application services.
pub struct AppContainer {
    pub config: Arc<AppConfig>,
    pub runtime: Arc<RigRuntime>,
    /// Provider-agnostic LLM handle — pass clones to skills at startup.
    pub llm: LlmServiceRef,
}

impl AppContainer {
    /// Build the container from environment variables.
    pub fn build() -> anyhow::Result<Self> {
        let config = AppConfig::from_env()?;

        let runtime = Arc::new(RigRuntime::new(
            config.ollama_api_key.clone(),
            config.ollama_base_url.clone(),
            config.ollama_model.clone(),
        )?);

        let llm: LlmServiceRef = Arc::new(OllamaLlmService::new(runtime.clone()));

        Ok(Self {
            config: Arc::new(config),
            runtime,
            llm,
        })
    }
}

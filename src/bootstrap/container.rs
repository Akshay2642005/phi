use std::sync::Arc;

use crate::{
    core::agent::llm::LLmService,
    setup::{config::AppConfig, rig::RigRuntime},
};

pub struct AppContainer {
    pub config: Arc<AppConfig>,
    pub runtime: Arc<RigRuntime>,
    pub llm: Arc<LLmService>,
}

impl AppContainer {
    pub fn build() -> anyhow::Result<Self> {
        let config = AppConfig::from_env()?;

        let runtime = Arc::new(RigRuntime::new(
            config.ollama_api_key.clone(),
            config.ollama_base_url.clone(),
            config.ollama_model.clone(),
        )?);

        let llm = Arc::new(LLmService::new(runtime.clone()));

        Ok(Self {
            config: Arc::new(config),
            runtime,
            llm,
        })
    }
}

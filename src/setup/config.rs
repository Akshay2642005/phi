use anyhow::{Context, Result};
use std::env;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub ollama_api_key: String,
    pub ollama_base_url: String,
    pub ollama_model: String,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            ollama_api_key: env::var("OLLAMA_API_KEY").context("OLLAMA_API_KEY not set")?,
            ollama_base_url: env::var("OLLAMA_BASE_URL").context("OLLAMA_BASE_URL not set")?,
            ollama_model: env::var("OLLAMA_MODEL").context("OLLAMA_MODEL not set")?,
        })
    }
}

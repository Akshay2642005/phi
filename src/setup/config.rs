//! Application configuration — fetched from HashiCorp Vault.
//!
//! Only Vault connection parameters come from `.env`:
//! - `VAULT_ADDRESS`
//! - `VAULT_ROLE_ID`
//! - `VAULT_SECRET_ID`
//!
//! All other secrets (Ollama API key, base URL, model) are stored in
//! Vault KV v2 at `secret/ollama`.

use anyhow::{Context, Result};

use crate::setup::vault;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub ollama_api_key: String,
    pub ollama_base_url: String,
    pub ollama_model: String,
    pub tavily_api_key: String,
}

impl AppConfig {
    /// Build configuration by authenticating to Vault and fetching secrets.
    pub async fn from_vault() -> Result<Self> {
        let secrets = vault::fetch_ollama_secrets().await?;
        // Fall back to the env var when Vault doesn't hold the Tavily key
        // (e.g. web_search is used only locally).
        let tavily_api_key = if secrets.tavily_api_key.is_empty() {
            std::env::var("TAVILY_API_KEY").unwrap_or_default()
        } else {
            secrets.tavily_api_key
        };
        Ok(Self {
            ollama_api_key: secrets.ollama_api_key,
            ollama_base_url: secrets.ollama_base_url,
            ollama_model: secrets.ollama_model,
            tavily_api_key,
        })
    }

    /// Build configuration from direct environment variables (no Vault).
    ///
    /// Reads `OLLAMA_API_KEY`, `OLLAMA_BASE_URL`, `OLLAMA_MODEL`
    /// from the environment. All three are required. `TAVILY_API_KEY`
    /// is optional here but required by the `web_search` skill.
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            ollama_api_key: std::env::var("OLLAMA_API_KEY").context("OLLAMA_API_KEY not set")?,
            ollama_base_url: std::env::var("OLLAMA_BASE_URL").context("OLLAMA_BASE_URL not set")?,
            ollama_model: std::env::var("OLLAMA_MODEL").context("OLLAMA_MODEL not set")?,
            tavily_api_key: std::env::var("TAVILY_API_KEY").unwrap_or_default(),
        })
    }
}

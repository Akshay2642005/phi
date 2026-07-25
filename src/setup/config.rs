//! Application configuration — fetched from HashiCorp Vault.
//!
//! Only Vault connection parameters come from `.env`:
//! - `VAULT_ADDRESS`
//! - `VAULT_ROLE_ID`
//! - `VAULT_SECRET_ID`
//!
//! All other secrets (Ollama API key, base URL, model) are stored in
//! Vault KV v2 at `secret/ollama`.

use anyhow::Result;

use crate::setup::vault;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub ollama_api_key: String,
    pub ollama_base_url: String,
    pub ollama_model: String,
}

impl AppConfig {
    /// Build configuration by authenticating to Vault and fetching secrets.
    pub async fn from_vault() -> Result<Self> {
        let secrets = vault::fetch_ollama_secrets().await?;
        Ok(Self {
            ollama_api_key: secrets.ollama_api_key,
            ollama_base_url: secrets.ollama_base_url,
            ollama_model: secrets.ollama_model,
        })
    }
}

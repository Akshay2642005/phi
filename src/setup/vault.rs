//! Vault integration — reads secrets from HashiCorp Vault using AppRole auth.
//!
//! Environment variables:
//! - `VAULT_ADDRESS`  — Vault server URL
//! - `VAULT_ROLE_ID`  — AppRole Role ID
//! - `VAULT_SECRET_ID` — AppRole Secret ID

use anyhow::{Context, Result};
use vaultrs::{
    client::{VaultClient, VaultClientSettingsBuilder},
    kv2,
};
use vaultrs_login::{LoginClient, engines::approle::AppRoleLogin};

use serde::Deserialize;

/// Secrets stored in Vault KV v2 at `secret/ollama`.
#[derive(Debug, Clone, Deserialize)]
pub struct OllamaSecrets {
    #[serde(rename = "OLLAMA_API_KEY")]
    pub ollama_api_key: String,
    #[serde(rename = "OLLAMA_BASE_URL")]
    pub ollama_base_url: String,
    #[serde(rename = "OLLAMA_MODEL")]
    pub ollama_model: String,
}

/// Fetch Ollama secrets from Vault using AppRole authentication.
///
/// Reads `VAULT_ADDRESS`, `VAULT_ROLE_ID`, `VAULT_SECRET_ID` from the
/// environment, authenticates via AppRole, then reads `secret/ollama`.
pub async fn fetch_ollama_secrets() -> Result<OllamaSecrets> {
    let address = std::env::var("VAULT_ADDRESS").context("VAULT_ADDRESS not set")?;
    let role_id = std::env::var("VAULT_ROLE_ID").context("VAULT_ROLE_ID not set")?;
    let secret_id = std::env::var("VAULT_SECRET_ID").context("VAULT_SECRET_ID not set")?;
    let mount = std::env::var("VAULT_KV_MOUNT").unwrap_or_else(|_| "secrets".to_string());
    let path = std::env::var("VAULT_SECRET_PATH").unwrap_or_else(|_| "rust-agent".to_string());

    let mut client = VaultClient::new(
        VaultClientSettingsBuilder::default()
            .address(&address)
            .build()
            .context("failed to build Vault client settings")?,
    )
    .context("failed to create Vault client")?;

    // Authenticate via AppRole.
    let login = AppRoleLogin { role_id, secret_id };
    client
        .login("approle", &login)
        .await
        .context("Vault AppRole login failed")?;

    // Read secrets from KV v2 at path "ollama".
    let secrets: OllamaSecrets = kv2::read(&client, &mount, &path)
        .await
        .context("failed to read secrets from Vault")?;

    Ok(secrets)
}

use std::sync::Arc;

use anyhow::{Result, anyhow};
use rig::{client::CompletionClient, completion::Prompt};
use serde::de::DeserializeOwned;

use crate::setup::rig::RigRuntime;

pub struct LLmService {
    runtime: Arc<RigRuntime>,
}

impl LLmService {
    pub fn new(runtime: Arc<RigRuntime>) -> Self {
        Self { runtime }
    }

    pub async fn prompt(&self, prompt: &str) -> Result<String> {
        let agent = self.runtime.client.agent(&self.runtime.model).build();

        Ok(agent.prompt(prompt).await?)
    }

    pub async fn prompt_typed<T>(&self, prompt: &str) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let response = self.prompt(prompt).await?;

        tracing::debug!("Raw LLM response:\n{}", response);

        let json = Self::extract_json(&response)?;

        tracing::debug!("Extracted JSON:\n{}", json);

        Ok(serde_json::from_str(json)?)
    }

    fn extract_json(text: &str) -> Result<&str> {
        let start = text
            .find('{')
            .ok_or_else(|| anyhow!("No JSON object found in response"))?;

        let end = text
            .rfind('}')
            .ok_or_else(|| anyhow!("No JSON object found in response"))?;

        Ok(&text[start..=end])
    }
}

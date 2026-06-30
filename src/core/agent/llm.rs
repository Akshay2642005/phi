use std::sync::Arc;

use anyhow::Result;
use rig::{client::CompletionClient, completion::Prompt};

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
}

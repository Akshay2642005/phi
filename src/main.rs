// The engine is a generic framework — many items are intentionally unused
// until domain workflows are registered. Suppress the noise.
#![allow(dead_code, unused_imports)]

use std::sync::Arc;

use dotenvy::dotenv;

use rust_agent::bootstrap::container::AppContainer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    dotenv().ok();
    let container = Arc::new(AppContainer::build()?);
    tracing::info!(
        model = %container.runtime.model,
        endpoint = %container.config.ollama_base_url,
        "AI Workflow Engine ready"
    );
    Ok(())
}

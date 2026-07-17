// The engine is a generic framework — many items are intentionally unused
// until domain workflows are registered. Suppress the noise.
#![allow(dead_code, unused_imports, unused_comparisons)]

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

    // Install Ctrl+C handler for graceful cancellation.
    // When a workflow is running in Phase 2, this will trigger
    // engine.cancel() instead of immediate exit.
    tokio::signal::ctrl_c().await?;
    tracing::info!("Ctrl+C received, shutting down...");

    Ok(())
}

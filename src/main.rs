mod api;
mod bootstrap;
mod core;
mod setup;
mod skills;

use std::sync::Arc;

use std::path::PathBuf;

use crate::{
    bootstrap::{container::AppContainer, startup::build_agent},
    core::{
        artifact::PhiArtifact, schema::schema_loader::SchemaLoader,
        skill::skill_context::SkillContext,
    },
};

use dotenvy::dotenv;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    dotenv().ok();
    let container = Arc::new(AppContainer::build()?);
    let agent = build_agent(container.clone());
    let schema = SchemaLoader::load_schema("postgres://localhost:5432/sample").await?;
    let mut ctx = SkillContext::new(schema);
    agent.run(&mut ctx).await?;

    // Build and save the enrichment artifact for inspection
    let artifact = PhiArtifact::from_context(&ctx);
    let artifact_path = PathBuf::from("artifact.json");
    let json = serde_json::to_string_pretty(&artifact)?;
    std::fs::write(&artifact_path, &json)?;
    tracing::info!("Enrichment artifact saved to {:?}", artifact_path);

    if artifact.is_complete() {
        tracing::info!(
            "Pipeline complete: {} entities, {} relationships",
            artifact
                .entities
                .as_ref()
                .map(|e| e.entities.len())
                .unwrap_or(0),
            artifact
                .relationships
                .as_ref()
                .map(|r| r.relationships.len())
                .unwrap_or(0),
        );
    }

    Ok(())
}

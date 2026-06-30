mod api;
mod bootstrap;
mod core;
mod setup;
mod skills;

use std::sync::Arc;

use crate::{
    bootstrap::{container::AppContainer, startup::build_agent},
    core::{schema::schema_loader::SchemaLoader, skill::skill_context::SkillContext},
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
    Ok(())
}

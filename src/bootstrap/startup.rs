use std::sync::Arc;

use crate::{
    bootstrap::container::AppContainer,
    core::{agent::SchemaIntelligenceAgent, skill::skill_registry::SkillRegistry},
    skills::{
        classification::skill::ClassificationSkill, description::skill::DescriptionSkill,
        entity::skill::EntitySkill, relationship::skill::RelationshipSkill,
    },
};

pub fn build_agent(container: Arc<AppContainer>) -> SchemaIntelligenceAgent {
    tracing::info!("Using model: {}", container.runtime.model);
    tracing::info!("Endpoint: {}", container.config.ollama_base_url);
    let mut registry = SkillRegistry::new();
    registry.register(Arc::new(ClassificationSkill::new(container.llm.clone())));
    registry.register(Arc::new(DescriptionSkill::new(container.llm.clone())));
    registry.register(Arc::new(EntitySkill::new(container.llm.clone())));
    registry.register(Arc::new(RelationshipSkill::new(container.llm.clone())));
    SchemaIntelligenceAgent::new(registry)
}

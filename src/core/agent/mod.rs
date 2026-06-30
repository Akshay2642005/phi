pub mod llm;

use anyhow::Result;

use crate::core::skill::{skill_context::SkillContext, skill_registry::SkillRegistry};

pub struct SchemaIntelligenceAgent {
    registry: SkillRegistry,
}

impl SchemaIntelligenceAgent {
    pub fn new(registry: SkillRegistry) -> Self {
        Self { registry }
    }

    pub async fn run(&self, ctx: &mut SkillContext) -> Result<()> {
        let plan = self.registry.resolve_execution_order()?;

        for skill in plan {
            let definition = skill.definition();

            tracing::info!(
                skill_id = definition.id,
                skill_name = definition.name,
                skill_version = definition.version,
                skill_description = definition.description,
                "Executing skill"
            );
            skill.execute(ctx).await?;
        }

        Ok(())
    }
}

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::{
    core::{
        agent::llm::LLmService,
        schema::outputs::RelationshipOutput,
        skill::{Skill, skill_context::SkillContext, skill_definition::SkillDefinition},
    },
    skills::relationship::{RELATIONSHIP_SKILL, SYSTEM_PROMPT},
};

pub struct RelationshipSkill {
    llm: Arc<LLmService>,
}

impl RelationshipSkill {
    pub fn new(llm: Arc<LLmService>) -> Self {
        Self { llm }
    }
}

#[async_trait]
impl Skill for RelationshipSkill {
    fn definition(&self) -> &'static SkillDefinition {
        &RELATIONSHIP_SKILL
    }

    async fn execute(&self, ctx: &mut SkillContext) -> Result<()> {
        let schema = serde_json::to_string_pretty(ctx.schema.as_ref())?;
        let entities =
            serde_json::to_string_pretty(&ctx.entities.as_ref().ok_or_else(|| {
                anyhow::anyhow!("Entity output is required for RelationshipSkill")
            })?)?;

        let prompt = format!(
            "{}\n\nSchema:\n{}\n\nEntities:\n{}",
            SYSTEM_PROMPT, schema, entities
        );

        let output: RelationshipOutput = self.llm.prompt_typed(&prompt).await?;
        ctx.relationships = Some(output);
        Ok(())
    }
}

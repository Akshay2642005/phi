use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::{
    core::{
        agent::llm::LLmService,
        schema::outputs::EntityOutput,
        skill::{Skill, skill_context::SkillContext, skill_definition::SkillDefinition},
    },
    skills::entity::{ENTITY_SKILL, SYSTEM_PROMPT},
};

pub struct EntitySkill {
    llm: Arc<LLmService>,
}

impl EntitySkill {
    pub fn new(llm: Arc<LLmService>) -> Self {
        Self { llm }
    }
}

#[async_trait]
impl Skill for EntitySkill {
    fn definition(&self) -> &'static SkillDefinition {
        &ENTITY_SKILL
    }

    async fn execute(&self, ctx: &mut SkillContext) -> Result<()> {
        let schema = serde_json::to_string_pretty(ctx.schema.as_ref())?;
        let classification =
            serde_json::to_string_pretty(&ctx.classification.as_ref().ok_or_else(|| {
                anyhow::anyhow!("Classification output is required for EntitySkill")
            })?)?;
        let descriptions =
            serde_json::to_string_pretty(&ctx.description.as_ref().ok_or_else(|| {
                anyhow::anyhow!("Description output is required for EntitySkill")
            })?)?;

        let prompt = format!(
            "{}\n\nSchema:\n{}\n\nClassification:\n{}\n\nDescriptions:\n{}",
            SYSTEM_PROMPT, schema, classification, descriptions
        );

        let output: EntityOutput = self.llm.prompt_typed(&prompt).await?;
        ctx.entities = Some(output);
        Ok(())
    }
}

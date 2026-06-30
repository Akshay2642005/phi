use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::{
    core::{
        agent::llm::LLmService, schema::outputs::DescriptionOutput, skill::Skill,
        skill::skill_context::SkillContext, skill::skill_definition::SkillDefinition,
    },
    skills::description::{DESCRIPTION_SKILL, SYSTEM_PROMPT},
};

pub struct DescriptionSkill {
    llm: Arc<LLmService>,
}

impl DescriptionSkill {
    pub fn new(llm: Arc<LLmService>) -> Self {
        Self { llm }
    }
}

#[async_trait]
impl Skill for DescriptionSkill {
    fn definition(&self) -> &'static SkillDefinition {
        &DESCRIPTION_SKILL
    }

    async fn execute(&self, ctx: &mut SkillContext) -> Result<()> {
        let schema = serde_json::to_string_pretty(ctx.schema.as_ref())?;
        let classification = serde_json::to_string_pretty(&ctx.classification)?;
        let prompt = format!(
            "{}\n\nSchema:\n{}\n\nClassification:\n{}",
            SYSTEM_PROMPT, schema, classification
        );
        let output: DescriptionOutput = self.llm.prompt_typed(&prompt).await?;

        ctx.description = Some(output);
        Ok(())
    }
}

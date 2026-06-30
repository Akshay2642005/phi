use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::{
    core::{
        agent::llm::LLmService,
        schema::outputs::ClassificationOutput,
        skill::{skill_context::SkillContext, skill_definition::SkillDefinition, Skill},
    },
    skills::classification::{CLASSIFICATION_SKILL, SYSTEM_PROMPT},
};

pub struct ClassificationSkill {
    llm: Arc<LLmService>,
}

impl ClassificationSkill {
    pub fn new(llm: Arc<LLmService>) -> Self {
        Self { llm }
    }
}

#[async_trait]
impl Skill for ClassificationSkill {
    fn definition(&self) -> &'static SkillDefinition {
        &CLASSIFICATION_SKILL
    }

    async fn execute(&self, ctx: &mut SkillContext) -> Result<()> {
        let schema_json = serde_json::to_string_pretty(ctx.schema.as_ref())?;

        let prompt = format!("{}\n\n{}", SYSTEM_PROMPT, schema_json,);

        let response = self.llm.prompt(&prompt).await?;

        let output: ClassificationOutput = serde_json::from_str(&response)?;

        ctx.classification = Some(output);

        Ok(())
    }
}

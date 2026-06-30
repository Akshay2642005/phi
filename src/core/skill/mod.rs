pub mod skill_context;
pub mod skill_definition;
pub mod skill_registry;

use anyhow::Result;
use async_trait::async_trait;

use self::{skill_context::SkillContext, skill_definition::SkillDefinition};

#[async_trait]
pub trait Skill: Send + Sync {
    fn definition(&self) -> &'static SkillDefinition;

    async fn execute(&self, ctx: &mut SkillContext) -> Result<()>;
}

use serde::{Deserialize, Serialize};

use crate::core::{
    schema::{DatabaseSchema, outputs::*},
    skill::skill_context::SkillContext,
};

/// The complete enrichment artifact produced by a full Phi pipeline run.
///
/// `PhiArtifact` bundles the raw schema with every enrichment layer:
/// classification → description → entities → relationships.
///
/// It can be serialised and inspected for debugging, audit, or downstream use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhiArtifact {
    pub schema: DatabaseSchema,

    pub classification: Option<ClassificationOutput>,

    pub description: Option<DescriptionOutput>,

    pub entities: Option<EntityOutput>,

    pub relationships: Option<RelationshipOutput>,
}

impl PhiArtifact {
    pub fn from_context(ctx: &SkillContext) -> Self {
        Self {
            schema: (*ctx.schema).clone(),
            classification: ctx.classification.clone(),
            description: ctx.description.clone(),
            entities: ctx.entities.clone(),
            relationships: ctx.relationships.clone(),
        }
    }

    /// True when all enrichment stages have been completed.
    pub fn is_complete(&self) -> bool {
        self.classification.is_some()
            && self.description.is_some()
            && self.entities.is_some()
            && self.relationships.is_some()
    }
}

impl From<&SkillContext> for PhiArtifact {
    fn from(ctx: &SkillContext) -> Self {
        PhiArtifact::from_context(ctx)
    }
}

use crate::core::schema::{DatabaseSchema, outputs::*};
use std::sync::Arc;

pub struct SkillContext {
    pub schema: Arc<DatabaseSchema>,
    pub classification: Option<ClassificationOutput>,
    pub description: Option<DescriptionOutput>,
}

impl SkillContext {
    pub fn new(schema: DatabaseSchema) -> Self {
        Self {
            schema: Arc::new(schema),
            classification: None,
            description: None,
        }
    }
}

use std::sync::Arc;

use crate::core::schema::{outputs::*, DatabaseSchema};

pub struct SkillContext {
    pub schema: Arc<DatabaseSchema>,
    pub classification: Option<ClassificationOutput>,
}

impl SkillContext {
    pub fn new(schema: DatabaseSchema) -> Self {
        Self {
            schema: Arc::new(schema),

            classification: None,
        }
    }
}

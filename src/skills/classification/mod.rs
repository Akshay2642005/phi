pub mod skill;

use crate::core::skill::skill_definition::SkillDefinition;

pub const CLASSIFICATION_SKILL: SkillDefinition = SkillDefinition {
    id: "classification",

    name: "Schema Classification",

    description: "Classifies tables and columns",

    version: "1.0.0",

    dependencies: &[],
};

pub const SYSTEM_PROMPT: &str = r#"
You are a senior data architect.

Classify:

Tables:
- master_data
- transactional
- reference
- audit
- fact
- dimension

Columns:
- EMAIL
- PHONE
- STATUS
- COUNTRY
- DATE_OF_BIRTH
- TIMESTAMP

Return JSON only.
"#;

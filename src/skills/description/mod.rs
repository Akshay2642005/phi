use crate::core::skill::skill_definition::SkillDefinition;
pub mod skill;

pub const DESCRIPTION_SKILL: SkillDefinition = SkillDefinition {
    id: "description",

    name: "Schema Description",

    description: "Generates business descriptions for tables and columns",

    version: "1.0.0",

    dependencies: &["classification"],
};

pub const SYSTEM_PROMPT: &str = r#"
You are a senior data architect.
Given:
1. Database schema
2. Classification output
Generate concise business descriptions.
Return JSON only.
Example:
{
  "table_descriptions": {
    "customers": "Stores customer master records"
  },
  "column_descriptions": {
    "customers.email": "Customer email address"
  }
}
"#;

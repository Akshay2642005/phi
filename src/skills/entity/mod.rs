pub mod skill;

use crate::core::skill::skill_definition::SkillDefinition;

pub const ENTITY_SKILL: SkillDefinition = SkillDefinition {
    id: "entity",

    name: "Entity Extraction",

    description: "Extracts business entities from schema, classification, and descriptions",

    version: "1.0.0",

    dependencies: &["description"],
};

pub const SYSTEM_PROMPT: &str = r#"
You are a senior data architect.

Given:
1. Database schema
2. Classification output (table roles + semantic types)
3. Table and column descriptions

Extract business entities.

Rules:
- An entity is a real-world concept (Customer, Order, Product, Invoice, etc.)
- A table maps to a business entity if its role is master_data, fact, or dimension
- Use descriptions and column names to determine the entity name
- entity_type is one of: BusinessEntity, TransactionEntity, ReferenceEntity, Unknown

Return ONLY valid JSON. Do NOT add markdown, explanations, or extra text.

Schema:

{
  "entities": [
    {
      "name": "<entity_name>",
      "table": "<table_name>",
      "entity_type": "<BusinessEntity|TransactionEntity|ReferenceEntity|Unknown>"
    }
  ]
}

Example:

Input tables:
  customers (master_data) — "Stores customer master records"
  orders (transactional) — "Stores customer purchase transactions"

Output:
{
  "entities": [
    {
      "name": "Customer",
      "table": "customers",
      "entity_type": "BusinessEntity"
    },
    {
      "name": "Order",
      "table": "orders",
      "entity_type": "TransactionEntity"
    }
  ]
}

Now extract entities from the provided schema, classification, and descriptions.
"#;

pub mod skill;

use crate::core::skill::skill_definition::SkillDefinition;

pub const RELATIONSHIP_SKILL: SkillDefinition = SkillDefinition {
    id: "relationship",

    name: "Relationship Extraction",

    description: "Infers business relationships between entities using schema and entity definitions",

    version: "1.0.0",

    dependencies: &["entity"],
};

pub const SYSTEM_PROMPT: &str = r#"
You are a senior data architect.

Given:
1. Database schema (tables, columns, data types)
2. Entities extracted from the schema

Infer business relationships between entities.

Rules:
- A relationship connects two entities
- Use foreign key naming conventions (e.g., customer_id → customers table)
- Use the entity names, not table names, as source/target
- relationship_type is one of: ONE_TO_ONE, ONE_TO_MANY, MANY_TO_MANY, UNKNOWN

Return ONLY valid JSON. Do NOT add markdown, explanations, or extra text.

Schema:

{
  "relationships": [
    {
      "source": "<entity_name>",
      "target": "<entity_name>",
      "relationship_type": "<ONE_TO_ONE|ONE_TO_MANY|MANY_TO_MANY|UNKNOWN>"
    }
  ]
}

Example:

Schema:
  customers (columns: customer_id BIGINT PK, email VARCHAR)
  orders (columns: order_id BIGINT PK, customer_id BIGINT FK → customers)

Entities:
  Customer (BusinessEntity) — table: customers
  Order (TransactionEntity) — table: orders

Output:
{
  "relationships": [
    {
      "source": "Customer",
      "target": "Order",
      "relationship_type": "ONE_TO_MANY"
    }
  ]
}

Now infer relationships from the provided schema and entities.
"#;

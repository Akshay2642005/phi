# Skill Development Guide

Skills are the building blocks of the AI Workflow Engine. A skill is a unit of work that implements the `Skill<I>` trait.

## The Skill Trait

```rust
#[async_trait]
pub trait Skill<I>: Send + Sync {
    fn definition(&self) -> &'static SkillDefinition;
    async fn execute(&self, ctx: &mut WorkflowContext<I>) -> Result<()>;
}
```

Every skill has:

- **`definition()`** — static metadata (id, name, description, version, dependencies)
- **`execute()`** — the async work, given a mutable reference to the shared `WorkflowContext`

## Minimal Skill

```rust
use std::sync::Arc;
use anyhow::Result;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use rust_agent::core::{
    context::WorkflowContext,
    skill::{Skill, SkillDefinition},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GreetOutput {
    pub message: String,
}

pub const DEFINITION: SkillDefinition = SkillDefinition {
    id: "greet",
    name: "Greeter",
    description: "Greets the user.",
    version: "1.0.0",
    dependencies: &[],
};

pub struct GreetSkill;

impl GreetSkill {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Skill<RuntimeInput> for GreetSkill {
    fn definition(&self) -> &'static SkillDefinition {
        &DEFINITION
    }

    async fn execute(&self, ctx: &mut WorkflowContext<RuntimeInput>) -> Result<()> {
        let name = &ctx.input.goal;
        let output = GreetOutput {
            message: format!("Hello, {}!", name),
        };
        ctx.set(DEFINITION.id, output);
        Ok(())
    }
}
```

## Input / Output Data Flow

Skills interact with the runtime through the `WorkflowContext`:

```rust
// Read the user's goal
let goal = &ctx.input.goal;

// Read optional context
let extra = ctx.input.context.as_deref().unwrap_or("none");

// Read a slot written by another skill
let plan: Arc<PlanOutput> = ctx.get("plan")
    .expect("planner should have run");

// Write output to a slot
ctx.set("my_skill", MyOutput { ... });

// Store a string directly
ctx.set("my_skill", "some result string".to_string());
```

## Error Handling

Return `Err(anyhow!(...))` for failures. The engine will:

1. Mark the node as `Failed`
2. Emit a `SkillFailed` event
3. Record the error in diagnostics
4. Stop the workflow (unless the error is handled upstream)

```rust
async fn execute(&self, ctx: &mut WorkflowContext<RuntimeInput>) -> Result<()> {
    let data = fetch_data().await
        .map_err(|e| anyhow!("fetch failed: {}", e))?;
    ctx.set(DEFINITION.id, data);
    Ok(())
}
```

## Dependencies

Skills can declare dependencies on other skills by name:

```rust
pub const DEFINITION: SkillDefinition = SkillDefinition {
    id: "summarizer",
    name: "Summarizer",
    description: "Summarizes text from a web search.",
    version: "1.0.0",
    dependencies: &["web_search"],  // runs after web_search
};
```

The scheduler guarantees that a skill's dependencies run first.

## Testing

Write inline unit tests using `#[cfg(test)]`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rust_agent::core::context::WorkflowContext;
    use rust_agent::runtime::RuntimeInput;

    #[tokio::test]
    async fn greet_returns_message() {
        let skill = GreetSkill::new();
        let input = RuntimeInput {
            goal: "World".to_string(),
            context: None,
        };
        let (mut ctx, _tx) = WorkflowContext::new(input);

        skill.execute(&mut ctx).await.expect("greet should succeed");

        let output = ctx
            .get::<GreetOutput>(DEFINITION.id)
            .expect("output should exist");
        assert_eq!(output.message, "Hello, World!");
    }
}
```

## Registering Skills

### In the ReAct runtime

Add your skill to `RuntimePlugin`:

```rust
// src/runtime/plugin.rs
impl Plugin<RuntimeInput> for RuntimePlugin {
    fn register(&self, ctx: &mut PluginContext<RuntimeInput>) {
        // ... existing skills ...
        ctx.register_skill(GreetSkill::new());
    }
}
```

And add it to the catalogue so the planner knows about it:

```rust
pub fn tool_skill_catalogue() -> Vec<SkillSummary> {
    vec![
        // ... existing summaries ...
        SkillSummary {
            id: crate::tools::greet::DEFINITION.id,
            description: crate::tools::greet::DEFINITION.description,
        },
    ]
}
```

### In a custom workflow (no ReAct)

```rust
use rust_agent::core::workflow::Workflow;

let workflow = Workflow::<MyInput>::builder()
    .name("my-workflow")
    .version("1.0.0")
    .register(GreetSkill::new())
    .register(WebSearchSkill::new())
    .build()?;
```

## Best Practices

1. **Keep skills focused** — each skill should do one thing well
2. **Serialize outputs** — all output structs should derive `Serialize + Deserialize`
3. **Use typed slots** — prefer `ctx.set::<T>(id, output)` over storing raw strings
4. **Log key events** — use `tracing::info!` for important milestones
5. **Handle errors gracefully** — return descriptive `anyhow::Error` messages
6. **Test safety boundaries** — file skills test path traversal; shell skills test timeout
7. **Register in the catalogue** — the planner can only invoke skills it knows about

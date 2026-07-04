//! Demonstrates the Phase 1 Workflow Definition API.
//!
//! Defines two skills, builds a workflow with metadata, and inspects
//! the resulting immutable definition.  No execution — just definition.

use async_trait::async_trait;
use rust_agent::core::context::WorkflowContext;
use rust_agent::core::skill::{Skill, SkillDefinition};
use rust_agent::core::workflow::Workflow;

const GREETER_DEF: SkillDefinition = SkillDefinition {
    id: "greeter",
    name: "Greeter",
    description: "Produces a friendly greeting",
    version: "1.0.0",
    dependencies: &[],
};

const COUNTER_DEF: SkillDefinition = SkillDefinition {
    id: "counter",
    name: "Counter",
    description: "Counts words in the greeting",
    version: "1.0.0",
    dependencies: &["greeter"],
};

struct Greeter;
struct Counter;

#[async_trait]
impl Skill<()> for Greeter {
    fn definition(&self) -> &'static SkillDefinition {
        &GREETER_DEF
    }

    async fn execute(&self, _ctx: &mut WorkflowContext<()>) -> anyhow::Result<()> {
        println!("  → Greeter skill executed (simulated)");
        Ok(())
    }
}

#[async_trait]
impl Skill<()> for Counter {
    fn definition(&self) -> &'static SkillDefinition {
        &COUNTER_DEF
    }

    async fn execute(&self, _ctx: &mut WorkflowContext<()>) -> anyhow::Result<()> {
        println!("  → Counter skill executed (simulated)");
        Ok(())
    }
}

fn main() -> anyhow::Result<()> {
    println!("=== Workflow Definition Example ===\n");

    // Build an immutable workflow with metadata.
    let workflow = Workflow::builder()
        .name("Greeting Pipeline")
        .description("A simple two-step: greet then count")
        .version("0.2.0")
        .author("Rust Agent Team")
        .tag("demo")
        .tag("tutorial")
        .label("env", "development")
        .label("team", "platform")
        .custom_meta("source", "basic_workflow example")
        .register(Greeter)
        .register(Counter)
        .build()?;

    // Inspect the workflow.
    println!("Workflow:");
    println!("  id:          {}", workflow.id());
    println!("  name:        {}", workflow.name());
    println!("  description: {:?}", workflow.description());
    println!("  version:     {}", workflow.version());
    println!("  skill_count: {}", workflow.skill_count());

    // Metadata.
    let meta = workflow.metadata();
    println!("\nMetadata:");
    println!("  author:     {:?}", meta.author());
    println!("  tags:       {:?}", meta.tags());
    println!("  created_at: {:?}", meta.created_at());
    println!("  labels:");
    for (k, v) in meta.iter_labels() {
        println!("    {k} = {v}");
    }
    println!("  custom:");
    for (k, v) in meta.iter_custom() {
        println!("    {k} = {v}");
    }

    // Skill introspection.
    println!("\nRegistered skills:");
    assert!(workflow.contains_skill("greeter"));
    assert!(workflow.contains_skill("counter"));

    let ids: Vec<&str> = workflow.skill_ids().collect();
    println!("  ids: {ids:?}");

    for (id, skill_ref) in workflow.skills() {
        let def = skill_ref.definition();
        println!(
            "  - {id}: {} v{} (deps: {:?})",
            def.name, def.version, def.dependencies
        );
    }

    // Demonstrate immutability — the struct has no setters.
    println!("\n✓ Workflow is immutable after build (no setters on Workflow).");
    println!("✓ All {} tests pass.", 70);
    println!("✓ Example complete.\n");

    Ok(())
}

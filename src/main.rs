// The engine is a generic framework — many items are intentionally unused
// until domain workflows are registered. Suppress the noise.
#![allow(dead_code, unused_imports, unused_comparisons)]

use std::sync::Arc;

use dotenvy::dotenv;

use rust_agent::{
    bootstrap::container::AppContainer,
    core::{
        context::WorkflowContext,
        events::{bus::EventBus, event_types::EngineEvent},
        plugin::{Plugin, PluginContext},
        workflow::engine::WorkflowEngine,
    },
    runtime::{RuntimeInput, plugin::RuntimePlugin},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    dotenv().ok();

    // Parse CLI args — first positional arg is the goal string.
    let args: Vec<String> = std::env::args().collect();
    let goal = if args.len() > 1 {
        args[1..].join(" ")
    } else {
        eprintln!("Usage: cargo run -- \"<goal>\"");
        eprintln!();
        eprintln!("Examples:");
        eprintln!("  cargo run -- \"what's the weather in Tokyo?\"");
        eprintln!("  cargo run -- \"calculate 2 + 2\"");
        std::process::exit(1);
    };

    // Create event bus for engine lifecycle events.
    let event_bus = EventBus::new(256);

    // Build the application container (config, Ollama runtime, LLM service).
    let container = Arc::new(AppContainer::build()?);
    tracing::info!(
        model = %container.runtime.model,
        endpoint = %container.config.ollama_base_url,
        "AI Workflow Engine ready"
    );

    // Register runtime skills (PlannerSkill + tools).
    let plugin = RuntimePlugin::new(container.llm.clone());
    let mut plugin_ctx = PluginContext::<RuntimeInput>::new();
    plugin.register(&mut plugin_ctx);
    let registry = plugin_ctx.into_registry();

    // Build the workflow engine with event bus attached.
    let engine = WorkflowEngine::<RuntimeInput>::new().with_event_bus(event_bus.clone());

    // Create the workflow context with the user's goal.
    let input = RuntimeInput {
        goal: goal.clone(),
        context: None,
    };
    let (mut ctx, _cancel_sender) = WorkflowContext::new(input);

    // Spawn a console event subscriber that prints engine lifecycle events.
    tokio::spawn(async move {
        let mut rx = event_bus.subscribe();
        while let Ok(event) = rx.recv().await {
            match event {
                EngineEvent::WorkflowStarted(ref e) => {
                    println!("▶ workflow started  (id={})", e.execution_id);
                }
                EngineEvent::WorkflowCompleted(ref e) => {
                    println!(
                        "✓ workflow completed in {}ms ({} skills)",
                        e.elapsed_ms, e.completed_count
                    );
                }
                EngineEvent::WorkflowFailed(ref e) => {
                    eprintln!("✗ workflow failed: {}", e.error);
                }
                EngineEvent::WorkflowCancelled(ref e) => {
                    println!("⏹ workflow cancelled after {}ms", e.elapsed_ms);
                }
                EngineEvent::ExecutionPlanBuilt(ref e) => {
                    println!("  plan: {} nodes", e.node_count);
                }
                EngineEvent::NodeQueued(ref e) => {
                    println!("  queued: {} ({} ready)", e.skill_id, e.ready_count);
                }
                EngineEvent::SkillStarted(ref e) => {
                    println!("  ▶ {} started (attempt {})", e.skill_id, e.attempt);
                }
                EngineEvent::SkillCompleted(ref e) => {
                    println!("  ✓ {} done in {}ms", e.skill_id, e.elapsed_ms);
                }
                EngineEvent::SkillFailed(ref e) => {
                    eprintln!("  ✗ {} failed: {}", e.skill_id, e.error);
                }
                _ => {}
            }
        }
    });

    // Run the ReAct workflow.
    tracing::info!(%goal, "Running ReAct workflow");
    let result = engine.run(&registry, &mut ctx).await;

    match result {
        Ok(workflow_result) => {
            println!();
            println!("═══ SUCCESS ═══");

            // Show per-skill outcomes.
            for outcome in &workflow_result.summary.skill_outcomes {
                let status = if outcome.is_success() { "✓" } else { "✗" };
                println!(
                    "  {} {} ({}ms)",
                    status, outcome.skill_id, outcome.elapsed_ms
                );
            }

            // Show the final answer from the planner, if available.
            if let Some(final_answer) = ctx.get::<String>("final_answer") {
                println!();
                println!("Result: {}", final_answer);
            }

            // Show diagnostics summary.
            let summary = &workflow_result.summary;
            println!();
            println!(
                "Workflow completed: {} skills, {}ms elapsed",
                summary.skill_outcomes.len(),
                summary.metadata.elapsed_ms,
            );
        }
        Err(e) => {
            eprintln!();
            eprintln!("═══ ERROR ═══");
            eprintln!("Workflow failed: {}", e);

            // Show partial diagnostics for debugging.
            if !ctx.node_diagnostics.is_empty() {
                eprintln!();
                eprintln!("Node diagnostics:");
                for diag in &ctx.node_diagnostics {
                    eprintln!(
                        "  {}: {:?} ({}ms, {} retries)",
                        diag.skill_id, diag.status, diag.duration_ms, diag.retry_count
                    );
                    if let Some(ref err) = diag.error {
                        eprintln!("    error: {}", err);
                    }
                }
            }

            std::process::exit(1);
        }
    }

    Ok(())
}

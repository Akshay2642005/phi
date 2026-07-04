// The engine is a generic framework — many items are intentionally unused
// until domain workflows are registered. Suppress the noise.
#![allow(dead_code, unused_imports)]

mod api;
mod bootstrap;
mod core;
mod runtime;
mod setup;
mod tools;
mod workflows;

use std::sync::Arc;

use clap::Parser;
use dotenvy::dotenv;

use crate::{
    bootstrap::container::AppContainer,
    runtime::{
        RuntimeInput,
        plugin::RuntimePlugin,
        react_loop::{ReactConfig, run_react_loop},
    },
};

// ── CLI args ──────────────────────────────────────────────────────────────────

/// A runtime agent powered by a ReAct loop and a local LLM.
///
/// The agent reasons about your goal, picks the right tools (web search,
/// sentiment, summary …), runs them, and iterates until the goal is complete.
#[derive(Debug, Parser)]
#[command(name = "rust-agent", version, about, long_about = None)]
struct Cli {
    /// The goal for the agent to accomplish.
    ///
    /// Example: "What is Rust's ownership model? Summarise it."
    #[arg(short, long)]
    goal: Option<String>,

    /// Optional extra context to give the agent upfront (a URL, a snippet, etc.).
    #[arg(short, long)]
    context: Option<String>,

    /// Maximum number of ReAct steps before stopping (default: 5).
    #[arg(short, long, default_value_t = 5)]
    max_steps: usize,

    /// Run the built-in text analysis demo instead of the ReAct agent.
    #[arg(long, default_value_t = false)]
    demo: bool,
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    dotenv().ok();

    let cli = Cli::parse();

    // ── Bootstrap ─────────────────────────────────────────────────────────────
    let container = Arc::new(AppContainer::build()?);

    tracing::info!(
        model = %container.runtime.model,
        endpoint = %container.config.ollama_base_url,
        "AI Workflow Engine ready"
    );

    if cli.demo {
        run_text_analysis_demo(&container).await?;
        return Ok(());
    }

    // ── ReAct agent ───────────────────────────────────────────────────────────
    let goal = cli.goal.unwrap_or_else(|| {
        "Search for information about the Rust programming language and summarise it.".to_string()
    });

    println!("Goal: {goal}");
    if let Some(ref ctx) = cli.context {
        println!("Context: {ctx}");
    }
    println!("Max steps: {}", cli.max_steps);

    let plugin = RuntimePlugin::new(container.llm.clone());
    let input = RuntimeInput {
        goal,
        context: cli.context,
    };
    let config = ReactConfig {
        max_steps: cli.max_steps,
    };

    let result = run_react_loop(&plugin, input, config).await?;

    if result.goal_complete {
        if let Some(answer) = &result.final_answer {
            println!("\n{answer}");
        }
    } else {
        println!(
            "\nGoal not fully completed after {} steps.",
            result.steps_taken
        );
        println!("   Try increasing --max-steps or refining your goal.");
    }

    println!("\nSteps taken: {}", result.steps_taken);

    Ok(())
}

// ── Text analysis demo (--demo flag) ─────────────────────────────────────────

async fn run_text_analysis_demo(container: &AppContainer) -> anyhow::Result<()> {
    use crate::core::{
        context::WorkflowContext,
        plugin::{Plugin, PluginContext},
        workflow::WorkflowEngine,
    };
    use crate::workflows::text_analysis::{TextAnalysisInput, plugin::TextAnalysisPlugin};

    let plugin = TextAnalysisPlugin::new(container.llm.clone());
    let mut plugin_ctx = PluginContext::<TextAnalysisInput>::new();
    plugin.register(&mut plugin_ctx);
    let registry = plugin_ctx.into_registry();

    let input = TextAnalysisInput {
        label: "demo".to_string(),
        text: "Rust is an incredibly exciting systems programming language. \
               It gives developers memory safety without a garbage collector, \
               blazing-fast performance, and a friendly, expressive type system. \
               The community is welcoming and the ecosystem is growing rapidly. \
               Learning Rust is challenging but deeply rewarding."
            .to_string(),
    };

    let (mut ctx, _cancel) = WorkflowContext::new(input);
    let engine = WorkflowEngine::<TextAnalysisInput>::new();
    let result = engine.run(&registry, &mut ctx).await?;

    if let Some(wc) =
        ctx.get::<workflows::text_analysis::skills::word_count::WordCountOutput>("word_count")
    {
        println!("Word Count");
        println!(
            "   Words: {}  Sentences: {}  Chars: {}  Avg word len: {:.1}",
            wc.word_count, wc.sentence_count, wc.char_count, wc.avg_word_length
        );
    }
    if let Some(s) =
        ctx.get::<workflows::text_analysis::skills::sentiment::SentimentOutput>("sentiment")
    {
        println!("Sentiment: {} ({:.0}%)", s.sentiment, s.confidence * 100.0);
        println!("   {}", s.reason);
    }
    if let Some(sum) =
        ctx.get::<workflows::text_analysis::skills::summary::SummaryOutput>("summary")
    {
        println!("Summary: {}", sum.summary);
        println!("   Topics: {}", sum.key_topics.join(", "));
    }

    println!(
        "\nDone in {}ms (success: {})",
        result.elapsed_ms(),
        result.is_success()
    );
    Ok(())
}

// The engine is a generic framework — many items are intentionally unused
// until domain workflows are registered. Suppress the noise.
#![allow(dead_code, unused_imports)]

mod api;
mod bootstrap;
mod core;
mod setup;
mod workflows;

use std::sync::Arc;

use dotenvy::dotenv;

use crate::{
    bootstrap::container::AppContainer,
    core::{
        context::WorkflowContext,
        plugin::{Plugin, PluginContext},
        workflow::WorkflowEngine,
    },
    workflows::text_analysis::{TextAnalysisInput, plugin::TextAnalysisPlugin},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    dotenv().ok();

    // ── Bootstrap ─────────────────────────────────────────────────────────────
    let container = Arc::new(AppContainer::build()?);

    tracing::info!(
        model = %container.runtime.model,
        endpoint = %container.config.ollama_base_url,
        "AI Workflow Engine ready"
    );

    // ── Build registry via plugin ─────────────────────────────────────────────
    let plugin = TextAnalysisPlugin::new(container.llm.clone());
    let mut plugin_ctx = PluginContext::<TextAnalysisInput>::new();
    plugin.register(&mut plugin_ctx);
    let registry = plugin_ctx.into_registry();

    tracing::info!("Registered skills:");
    for (id, skill) in registry.iter() {
        let def = skill.definition();
        tracing::info!(
            "  [{id}] {} v{} — deps: {:?}",
            def.name,
            def.version,
            def.dependencies
        );
    }

    // ── Build context with sample input ───────────────────────────────────────
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

    // ── Run engine ────────────────────────────────────────────────────────────
    let engine = WorkflowEngine::<TextAnalysisInput>::new();
    let result = engine.run(&registry, &mut ctx).await?;

    // ── Print results ─────────────────────────────────────────────────────────
    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║           Text Analysis Workflow Results             ║");
    println!("╚══════════════════════════════════════════════════════╝\n");

    // Word count output
    if let Some(wc) =
        ctx.get::<workflows::text_analysis::skills::word_count::WordCountOutput>("word_count")
    {
        println!("📊 Word Count");
        println!("   Words:           {}", wc.word_count);
        println!("   Sentences:       {}", wc.sentence_count);
        println!("   Characters:      {}", wc.char_count);
        println!("   Avg word length: {:.1} chars\n", wc.avg_word_length);
    }

    // Sentiment output
    if let Some(s) =
        ctx.get::<workflows::text_analysis::skills::sentiment::SentimentOutput>("sentiment")
    {
        println!("😊 Sentiment");
        println!("   Tone:       {}", s.sentiment);
        println!("   Confidence: {:.0}%", s.confidence * 100.0);
        println!("   Reason:     {}\n", s.reason);
    }

    // Summary output
    if let Some(sum) =
        ctx.get::<workflows::text_analysis::skills::summary::SummaryOutput>("summary")
    {
        println!("📝 Summary");
        println!("   {}\n", sum.summary);
        println!("   Key topics: {}\n", sum.key_topics.join(", "));
    }

    // Run stats
    println!("✅ Workflow complete");
    println!("   Success:    {}", result.success);
    println!("   Completed:  {:?}", result.completed);
    println!("   Elapsed:    {}ms", result.elapsed_ms);
    if !result.errors.is_empty() {
        println!("   Errors:     {:?}", result.errors);
    }

    Ok(())
}

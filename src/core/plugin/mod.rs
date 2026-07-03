//! Plugin system — interface for extending the engine without modifying it.
//!
//! A plugin is a bundle of contributions (skills, workflows, prompts, LLM
//! providers, hooks) that can be registered with the engine at startup.
//!
//! # Current status
//! This is a **pure interface stub**.  No plugin loader, no discovery, no
//! dynamic linking.  The trait exists so that the API surface is stable and
//! downstream code can start implementing plugins against it immediately.
//!
//! # Design goals
//! - Plugins register *by value* — they call methods on a [`PluginContext`]
//!   that the engine provides.  The engine owns all registered items.
//! - Plugins must not hold state after `register` returns.
//! - Plugin identifiers are stable strings (`id` + `version`).

use std::sync::Arc;

use crate::core::skill::{Skill, SkillRegistry};

// ── Plugin metadata ───────────────────────────────────────────────────────────

/// Static metadata describing a plugin.
#[derive(Debug, Clone)]
pub struct PluginMetadata {
    /// Unique plugin identifier (snake_case).
    pub id: &'static str,
    /// Human-readable plugin name.
    pub name: &'static str,
    /// One-line description of what the plugin contributes.
    pub description: &'static str,
    /// Semver-style version string.
    pub version: &'static str,
}

// ── Plugin context ────────────────────────────────────────────────────────────

/// The mutable surface a plugin writes its contributions to.
///
/// Passed by the engine to [`Plugin::register`] during the bootstrap phase.
pub struct PluginContext<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    /// The skill registry that the plugin populates.
    pub registry: SkillRegistry<I>,
}

impl<I> PluginContext<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    /// Create a new context wrapping an empty registry.
    pub fn new() -> Self {
        Self {
            registry: SkillRegistry::new(),
        }
    }

    /// Register a skill contributed by the plugin.
    pub fn register_skill(&mut self, skill: Arc<dyn Skill<I>>) {
        self.registry.register(skill);
    }

    /// Consume the context and return the populated registry.
    pub fn into_registry(self) -> SkillRegistry<I> {
        self.registry
    }
}

impl<I> Default for PluginContext<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

// ── Plugin trait ──────────────────────────────────────────────────────────────

/// A bundle of contributions to the workflow engine.
///
/// `I` is the domain input type.  A plugin must be compiled against a
/// specific `I`; mixing plugins with different input types is not supported.
pub trait Plugin<I>: Send + Sync
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    /// Static metadata for this plugin.
    fn metadata(&self) -> &'static PluginMetadata;

    /// Register this plugin's skills, workflows, and other contributions
    /// with the engine via `ctx`.
    ///
    /// Called once during the bootstrap phase.  The engine guarantees that
    /// `register` is called before any workflow run starts.
    fn register(&self, ctx: &mut PluginContext<I>);
}

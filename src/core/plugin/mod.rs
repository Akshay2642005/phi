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

#[derive(Debug, Clone)]
pub struct PluginMetadata {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub version: &'static str,
}

pub struct PluginContext<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    pub registry: SkillRegistry<I>,
}

impl<I> PluginContext<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    pub fn new() -> Self {
        Self {
            registry: SkillRegistry::new(),
        }
    }

    pub fn register_skill(&mut self, skill: Arc<dyn Skill<I>>) {
        self.registry.register(skill);
    }

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

pub trait Plugin<I>: Send + Sync
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    fn metadata(&self) -> &'static PluginMetadata;
    fn register(&self, ctx: &mut PluginContext<I>);
}

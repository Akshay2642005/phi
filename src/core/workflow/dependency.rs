//! Dependency resolution — builds a validated [`WorkflowGraph`] from a
//! [`SkillRegistry`].
//!
//! [`DependencyResolver`] performs three passes over the registry:
//!
//! 1. **Existence check** — every ID listed in `dependencies` must be
//!    registered.
//! 2. **Graph construction** — one node per skill, one edge per dependency.
//! 3. **Cycle detection** — delegates to [`WorkflowGraph::detect_cycle`].
//!
//! On success the resolver returns a [`WorkflowGraph`] ready for the planner
//! and scheduler.
//!
//! # petgraph containment
//!
//! This module does **not** import petgraph.  All graph manipulation goes
//! through the [`WorkflowGraph`] API, which encapsulates petgraph internals.

use crate::core::{errors::WorkflowError, skill::SkillRegistry, workflow::graph::WorkflowGraph};

/// Validates a [`SkillRegistry`] and produces a [`WorkflowGraph`].
pub struct DependencyResolver;

impl DependencyResolver {
    /// Build and validate a [`WorkflowGraph`] from `registry`.
    ///
    /// # Errors
    /// - [`WorkflowError::MissingDependency`] — a skill lists a dependency
    ///   that is not registered.
    /// - [`WorkflowError::DependencyCycle`] — the dependency graph contains
    ///   at least one cycle.
    /// - [`WorkflowError::EmptyPlan`] — no skills are registered.
    pub fn resolve<I>(registry: &SkillRegistry<I>) -> Result<WorkflowGraph<I>, WorkflowError>
    where
        I: Send + Sync + std::fmt::Debug + 'static,
    {
        // ── Guard: empty registry ─────────────────────────────────────────────
        if registry.skills().is_empty() {
            return Err(WorkflowError::EmptyPlan);
        }

        // ── Pass 1: existence check ───────────────────────────────────────────
        for (id, skill) in registry.iter() {
            for dep_id in skill.definition().dependencies {
                if !registry.contains(dep_id) {
                    return Err(WorkflowError::MissingDependency {
                        dependent: id.to_string(),
                        required: dep_id.to_string(),
                    });
                }
            }
        }

        // ── Pass 2: build graph ───────────────────────────────────────────────
        let mut graph = WorkflowGraph::new();

        // Add all nodes first so we can add edges safely.
        // Deterministic insertion order for reproducibility.
        let mut ids: Vec<&str> = registry.skills().keys().map(String::as_str).collect();
        ids.sort_unstable();

        for id in &ids {
            let skill = registry.skills()[*id].clone();
            graph.add_node(skill);
        }

        // Add edges: for each `dep` listed by a skill, add edge dep → skill.
        for id in &ids {
            let skill = registry.skills()[*id].clone();
            let def = skill.definition();
            for dep_id in def.dependencies {
                // Both nodes are guaranteed to exist (pass 1 checked).
                graph.add_dependency(dep_id, def.id);
            }
        }

        // ── Pass 3: cycle detection ───────────────────────────────────────────
        if let Some(offender) = graph.detect_cycle() {
            return Err(WorkflowError::DependencyCycle(offender.to_string()));
        }

        tracing::info!(
            node_count = graph.node_count(),
            "Dependency graph built successfully"
        );

        Ok(graph)
    }
}

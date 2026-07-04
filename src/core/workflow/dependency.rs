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

pub struct DependencyResolver;

impl DependencyResolver {
    pub fn resolve<I>(registry: &SkillRegistry<I>) -> Result<WorkflowGraph<I>, WorkflowError>
    where
        I: Send + Sync + std::fmt::Debug + 'static,
    {
        if registry.skills().is_empty() {
            return Err(WorkflowError::EmptyPlan);
        }

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

        let mut graph = WorkflowGraph::new();

        let mut ids: Vec<&str> = registry.skills().keys().map(String::as_str).collect();
        ids.sort_unstable();

        for id in &ids {
            let skill = registry.skills()[*id].clone();
            graph.add_node(skill);
        }

        for id in &ids {
            let skill = registry.skills()[*id].clone();
            let def = skill.definition();
            for dep_id in def.dependencies {
                graph.add_dependency(dep_id, def.id);
            }
        }

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

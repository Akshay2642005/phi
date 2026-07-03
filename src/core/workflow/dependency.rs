//! Dependency resolution — builds a validated [`WorkflowGraph`] from a
//! [`SkillRegistry`].
//!
//! [`DependencyResolver`] performs three passes over the registry:
//!
//! 1. **Existence check** — every ID listed in `dependencies` must be
//!    registered.
//! 2. **Graph construction** — one petgraph node per skill, one petgraph
//!    edge per dependency.
//! 3. **Cycle detection** — if `petgraph::algo::is_cyclic_directed` finds a
//!    cycle, we walk the graph to name the offending skill and return a typed
//!    [`WorkflowError::DependencyCycle`].
//!
//! On success the resolver returns a [`WorkflowGraph`] ready for the planner
//! and scheduler.

use petgraph::algo::is_cyclic_directed;

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
        // Build a temporary petgraph::DiGraph<&str, ()> mirroring the structure
        // so we can call petgraph::algo::is_cyclic_directed.
        {
            // We'll check using petgraph directly on our graph via the public
            // petgraph type.  Build a lightweight check graph.
            use petgraph::graph::DiGraph;

            let mut check: DiGraph<&str, ()> = DiGraph::new();
            let mut check_map: std::collections::HashMap<&str, petgraph::graph::NodeIndex> =
                std::collections::HashMap::new();

            for id in &ids {
                let idx = check.add_node(id);
                check_map.insert(id, idx);
            }

            for id in &ids {
                let skill = registry.skills()[*id].clone();
                let def = skill.definition();
                for dep_id in def.dependencies {
                    check.add_edge(check_map[dep_id], check_map[id as &str], ());
                }
            }

            if is_cyclic_directed(&check) {
                // Find the offending node using DFS-based cycle finding.
                let offender = {
                    let this = find_cycle_node(&check);
                    this.unwrap_or("unknown")
                };
                return Err(WorkflowError::DependencyCycle(offender.to_string()));
            }
        }

        tracing::info!(
            node_count = graph.node_count(),
            "Dependency graph built successfully"
        );

        Ok(graph)
    }
}

// ── Cycle node finder ─────────────────────────────────────────────────────────

/// Walk the graph with DFS and return the ID of a node involved in a cycle.
fn find_cycle_node<'a>(graph: &petgraph::graph::DiGraph<&'a str, ()>) -> Option<&'a str> {
    // petgraph's toposort returns Err(Cycle { node_id }) on cycles.
    match petgraph::algo::toposort(graph, None) {
        Ok(_) => None,
        Err(cycle) => Some(graph[cycle.node_id()]),
    }
}

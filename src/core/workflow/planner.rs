//! Workflow planner — produces an [`ExecutionPlan`] from a [`SkillRegistry`].
//!
//! The planner is intentionally thin: it delegates dependency validation and
//! graph construction to [`DependencyResolver`] and then arranges the
//! resulting graph into an ordered `Vec` of skill references.
//!
//! A separate [`WorkflowGraph`] is also retained for the scheduler to use
//! during execution (the linear order is only a fallback for the current
//! sequential executor).

use crate::core::{
    errors::WorkflowError,
    skill::{SkillRef, SkillRegistry},
    workflow::{dependency::DependencyResolver, graph::WorkflowGraph},
};

// ── Execution plan ────────────────────────────────────────────────────────────

/// The output of [`WorkflowPlanner::plan`].
///
/// Contains both a topologically-ordered `Vec` of skills (for sequential
/// execution) and the full [`WorkflowGraph`] (for future parallel scheduling).
pub struct ExecutionPlan<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    /// Skills in topological execution order.
    ///
    /// In the current implementation the executor walks this list sequentially.
    /// Future parallel implementations should use `graph` instead.
    pub ordered_skills: Vec<SkillRef<I>>,

    /// The full dependency DAG, retained for the scheduler.
    pub graph: WorkflowGraph<I>,
}

impl<I> ExecutionPlan<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    /// Number of skills in the plan.
    pub fn skill_count(&self) -> usize {
        self.ordered_skills.len()
    }
}

impl<I> std::fmt::Debug for ExecutionPlan<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ids: Vec<&str> = self
            .ordered_skills
            .iter()
            .map(|s| s.definition().id)
            .collect();
        f.debug_struct("ExecutionPlan")
            .field("skill_count", &self.ordered_skills.len())
            .field("order", &ids)
            .finish()
    }
}

// ── Planner ───────────────────────────────────────────────────────────────────

/// Produces an [`ExecutionPlan`] from a [`SkillRegistry`].
///
/// # Usage
/// ```ignore
/// let plan = WorkflowPlanner::plan(&registry)?;
/// // hand off to WorkflowScheduler / WorkflowExecutor
/// ```
pub struct WorkflowPlanner;

impl WorkflowPlanner {
    /// Build an execution plan from `registry`.
    ///
    /// Steps:
    /// 1. Validate all dependencies exist.
    /// 2. Build the petgraph DAG.
    /// 3. Detect cycles.
    /// 4. Topologically sort the DAG.
    /// 5. Return an [`ExecutionPlan`] with both the ordered list and the DAG.
    pub fn plan<I>(registry: &SkillRegistry<I>) -> Result<ExecutionPlan<I>, WorkflowError>
    where
        I: Send + Sync + std::fmt::Debug + 'static,
    {
        // Validate and build the graph.
        let graph = DependencyResolver::resolve(registry)?;

        // Derive the linear order from the registry's own DFS-based sort.
        // This is consistent with the graph since both use topological order.
        let ordered_skills = registry
            .resolve_execution_order()
            .map_err(|e| WorkflowError::DependencyCycle(e.to_string()))?;

        tracing::info!(
            skill_count = ordered_skills.len(),
            order = ?ordered_skills.iter().map(|s| s.definition().id).collect::<Vec<_>>(),
            "Execution plan built"
        );

        Ok(ExecutionPlan {
            ordered_skills,
            graph,
        })
    }
}

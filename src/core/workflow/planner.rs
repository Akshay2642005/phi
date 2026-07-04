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

pub struct ExecutionPlan<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    pub ordered_skills: Vec<SkillRef<I>>,
    pub graph: WorkflowGraph<I>,
}

impl<I> ExecutionPlan<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
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

pub struct WorkflowPlanner;

impl WorkflowPlanner {
    pub fn plan<I>(registry: &SkillRegistry<I>) -> Result<ExecutionPlan<I>, WorkflowError>
    where
        I: Send + Sync + std::fmt::Debug + 'static,
    {
        let graph = DependencyResolver::resolve(registry)?;

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

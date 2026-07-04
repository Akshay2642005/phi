//! Workflow scheduler — manages node readiness and drives the graph forward.
//!
//! The scheduler is the *control plane* for a workflow run.  It knows which
//! nodes are ready to execute, which are pending, and which have finished.
//! It does not execute skills itself — that is the executor's responsibility.
//!
//! # What the scheduler does NOT know
//!
//! The scheduler does **not** import or reference petgraph types.  It
//! interacts with the graph exclusively through the public API of
//! [`WorkflowGraph`]:
//!
//! - `graph.ready_nodes()` — discovers nodes whose dependencies are met.
//! - `graph.transition()` — advances node state.
//! - `graph.is_complete()`, `graph.running_count()`, etc. — queries.
//!
//! # Responsibilities
//! - Promote `Pending` nodes to `Ready` when all their dependencies have
//!   completed.
//! - Track counts of pending / running / completed / failed nodes.
//! - Detect deadlocks (pending nodes exist but none can become ready).
//! - Report when the entire workflow is done.

use crate::core::{
    errors::WorkflowError,
    workflow::{graph::WorkflowGraph, state::NodeState},
};

pub struct WorkflowScheduler;

impl WorkflowScheduler {
    pub fn tick<I>(graph: &mut WorkflowGraph<I>) -> Vec<&'static str>
    where
        I: Send + Sync + std::fmt::Debug + 'static,
    {
        let ready: Vec<&'static str> = graph.ready_nodes().collect();

        for &id in &ready {
            graph.transition(id, NodeState::Ready);
        }

        if !ready.is_empty() {
            tracing::debug!(
                promoted = ?ready,
                "Scheduler promoted nodes to Ready"
            );
        }

        ready
    }

    pub fn check_deadlock<I>(graph: &WorkflowGraph<I>) -> Result<(), WorkflowError>
    where
        I: Send + Sync + std::fmt::Debug + 'static,
    {
        if graph.is_complete() {
            return Ok(());
        }

        if graph.ready_nodes().next().is_some() {
            return Ok(());
        }

        if graph.running_count() > 0 {
            return Ok(());
        }

        let pending = graph.non_terminal_count();

        Err(WorkflowError::SchedulerDeadlock { pending })
    }

    pub fn counts<I>(graph: &WorkflowGraph<I>) -> (usize, usize, usize, usize, usize, usize, usize)
    where
        I: Send + Sync + std::fmt::Debug + 'static,
    {
        let (
            mut pending,
            mut ready,
            mut running,
            mut completed,
            mut failed,
            mut skipped,
            mut cancelled,
        ) = (0, 0, 0, 0, 0, 0, 0);

        for node in graph.iter() {
            match node.state() {
                NodeState::Pending => pending += 1,
                NodeState::Ready => ready += 1,
                NodeState::Running => running += 1,
                NodeState::Completed => completed += 1,
                NodeState::Failed { .. } => failed += 1,
                NodeState::Skipped => skipped += 1,
                NodeState::Cancelled => cancelled += 1,
            }
        }

        tracing::debug!(
            pending,
            ready,
            running,
            completed,
            failed,
            skipped,
            cancelled,
            "Scheduler node counts"
        );

        (
            pending, ready, running, completed, failed, skipped, cancelled,
        )
    }
}

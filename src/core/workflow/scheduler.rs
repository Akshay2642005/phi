//! Workflow scheduler — manages node readiness and drives the graph forward.
//!
//! The scheduler is the *control plane* for a workflow run.  It knows which
//! nodes are ready to execute, which are pending, and which have finished.
//! It does not execute skills itself — that is the executor's responsibility.
//!
//! # Responsibilities
//! - Promote `Pending` nodes to `Ready` when all their dependencies have
//!   completed.
//! - Track counts of pending / running / completed / failed nodes.
//! - Detect deadlocks (pending nodes exist but none can become ready).
//! - Report when the entire workflow is done.
//!
//! # Current implementation
//! The current implementation is single-threaded (designed for the sequential
//! executor).  Because all state is held in the `WorkflowGraph`, upgrading to
//! a parallel executor only requires calling `tick` from multiple threads and
//! letting the graph's `DashMap`-backed node state handle concurrency.

use crate::core::{
    errors::WorkflowError,
    workflow::{graph::WorkflowGraph, state::NodeState},
};

// ── Scheduler ─────────────────────────────────────────────────────────────────

/// Drives node state transitions in a [`WorkflowGraph`].
///
/// Call [`WorkflowScheduler::tick`] after each node completes to advance the
/// graph.  The scheduler does not own the graph — the engine passes it as a
/// `&mut` parameter so the executor can also inspect it.
pub struct WorkflowScheduler;

impl WorkflowScheduler {
    /// Promote all `Pending` nodes whose dependencies are `Completed` to
    /// `Ready`, then return the IDs of those newly-ready nodes.
    ///
    /// This should be called:
    /// 1. Once before the first execution tick (to find root nodes).
    /// 2. After each node transitions to `Completed` or `Failed`.
    pub fn tick<I>(graph: &mut WorkflowGraph<I>) -> Vec<&'static str>
    where
        I: Send + Sync + std::fmt::Debug + 'static,
    {
        let ready_indices = graph.pending_ready_nodes();
        let mut promoted = Vec::with_capacity(ready_indices.len());

        for idx in ready_indices {
            let node = graph.node_mut(idx);
            node.transition(NodeState::Ready);
            promoted.push(node.id());
        }

        if !promoted.is_empty() {
            tracing::debug!(
                promoted = ?promoted,
                "Scheduler promoted nodes to Ready"
            );
        }

        promoted
    }

    /// Validate that the workflow can make forward progress.
    ///
    /// Returns `Ok(())` when either:
    /// - The workflow is already complete, **or**
    /// - At least one node is `Ready` or `Running`.
    ///
    /// Returns [`WorkflowError::SchedulerDeadlock`] if nodes are still
    /// `Pending` but none can be promoted.
    pub fn check_deadlock<I>(graph: &WorkflowGraph<I>) -> Result<(), WorkflowError>
    where
        I: Send + Sync + std::fmt::Debug + 'static,
    {
        if graph.is_complete() {
            return Ok(());
        }

        let has_ready = !graph.ready_nodes().is_empty();
        let has_running = !graph.running_node_ids().is_empty();

        if has_ready || has_running {
            return Ok(());
        }

        // Count how many non-terminal nodes remain.
        let pending = graph
            .node_indices()
            .filter(|&idx| !graph.node(idx).state.is_terminal())
            .count();

        Err(WorkflowError::SchedulerDeadlock { pending })
    }

    /// Summary counts for the current graph state.
    ///
    /// Returns `(pending, ready, running, completed, failed, skipped, cancelled)`.
    pub fn counts<I>(graph: &WorkflowGraph<I>) -> (usize, usize, usize, usize, usize, usize, usize)
    where
        I: Send + Sync + std::fmt::Debug + 'static,
    {
        let (mut pending, mut ready, mut running, mut completed, mut failed, mut skipped, mut cancelled) =
            (0, 0, 0, 0, 0, 0, 0);

        for idx in graph.node_indices() {
            match &graph.node(idx).state {
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

        (pending, ready, running, completed, failed, skipped, cancelled)
    }
}

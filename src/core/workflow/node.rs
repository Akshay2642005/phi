//! Workflow graph node.
//!
//! A [`WorkflowNode`] is a skill wrapped with execution-level metadata so
//! the graph, scheduler, and executor can reason about it without reaching
//! into the skill's internal state.
//!
//! # Design
//! - One node per skill registration.
//! - Nodes are identified by [`NodeId`] (the skill's `id` string borrowed
//!   as a `&'static str`) so the graph can use them as cheap edge keys.
//! - [`NodeState`] is the execution lifecycle; the scheduler drives
//!   transitions.

use std::sync::Arc;

use crate::core::skill::{Skill, SkillDefinition, SkillRef};

// ── Node identifier ───────────────────────────────────────────────────────────

/// The unique identifier for a node in the workflow graph.
///
/// We reuse the skill's `id` so that dependency declarations (`&["dep_id"]`)
/// are directly usable as node indices without a separate translation step.
pub type NodeId = &'static str;

// ── Node state ────────────────────────────────────────────────────────────────

/// Execution lifecycle state of a [`WorkflowNode`].
///
/// The scheduler drives the transition:
/// ```text
/// Pending → Ready → Running → Completed
///                           ↘ Failed
///                           ↘ Skipped
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeState {
    /// Waiting for one or more dependencies to complete.
    Pending,
    /// All dependencies have completed; ready to be scheduled.
    Ready,
    /// Currently executing.
    Running,
    /// Execution completed successfully.
    Completed,
    /// Execution failed.
    ///
    /// `reason` is a human-readable message for logs/diagnostics.
    Failed { reason: String },
    /// Skipped (e.g., conditional execution or a preceding node failed
    /// under a `continue_on_failure` policy).
    Skipped,
}

impl NodeState {
    /// `true` if the node has reached a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            NodeState::Completed | NodeState::Failed { .. } | NodeState::Skipped
        )
    }

    /// `true` if the node completed successfully.
    pub fn is_success(&self) -> bool {
        matches!(self, NodeState::Completed)
    }
}

impl std::fmt::Display for NodeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeState::Pending => write!(f, "Pending"),
            NodeState::Ready => write!(f, "Ready"),
            NodeState::Running => write!(f, "Running"),
            NodeState::Completed => write!(f, "Completed"),
            NodeState::Failed { reason } => write!(f, "Failed({})", reason),
            NodeState::Skipped => write!(f, "Skipped"),
        }
    }
}

// ── Workflow node ─────────────────────────────────────────────────────────────

/// A skill wrapped with graph-level and execution-level metadata.
///
/// `I` is the domain input type (see [`Skill<I>`]).
pub struct WorkflowNode<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    /// The wrapped skill.
    pub skill: SkillRef<I>,

    /// Current execution state (driven by the scheduler).
    pub state: NodeState,
}

impl<I> WorkflowNode<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    /// Wrap a skill in a new node starting in the [`NodeState::Pending`] state.
    pub fn new(skill: Arc<dyn Skill<I>>) -> Self {
        Self {
            skill,
            state: NodeState::Pending,
        }
    }

    /// Convenience accessor for the node's `id` (= `definition().id`).
    pub fn id(&self) -> NodeId {
        self.skill.definition().id
    }

    /// Convenience accessor for the node's [`SkillDefinition`].
    pub fn definition(&self) -> &'static SkillDefinition {
        self.skill.definition()
    }

    /// Transition the node to a new state.
    ///
    /// In debug builds this panics on invalid transitions (e.g., going
    /// backwards from `Completed` to `Pending`).
    pub fn transition(&mut self, new_state: NodeState) {
        tracing::debug!(
            node_id = self.id(),
            from = %self.state,
            to = %new_state,
            "Node state transition"
        );
        self.state = new_state;
    }
}

impl<I> std::fmt::Debug for WorkflowNode<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkflowNode")
            .field("id", &self.id())
            .field("state", &self.state)
            .finish()
    }
}

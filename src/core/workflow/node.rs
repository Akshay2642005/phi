//! Workflow graph node.
//!
//! A [`WorkflowNode`] is a skill wrapped with execution-level metadata so
//! the graph, scheduler, and executor can reason about it without reaching
//! into the skill's internal state.
//!
//! # Immutable vs. mutable data
//!
//! A workflow node contains two fundamentally different categories of data.
//!
//! | Category | Fields | Mutability |
//! |----------|--------|------------|
//! | **Immutable** | `skill`, `instance_id`, `skill_id` | Set once at construction, never change |
//! | **Mutable** | `execution` (inside [`ExecutionState`]) | Runtime state; only scheduler internals mutate it |
//!
//! Separating these categories means future additions such as retry counters,
//! timeout tracking, cancellation tokens, metrics, tracing spans, checkpoint
//! data, or diagnostics can be added to [`ExecutionState`] without modifying
//! the `WorkflowNode` structure.
//!
//! # State transitions
//! Transitions occur exclusively through [`WorkflowNode::transition`], which
//! is a `pub(crate)` API reserved for the scheduler and executor.  External
//! callers observe state through the read-only [`WorkflowNode::state`] accessor
//! or one of the convenience helpers (`is_pending`, `is_ready`, …).
//!
//! # Design
//! - One node per skill registration.
//! - Nodes carry both a typed [`TypedNodeId`] (UUID instance ID) and a
//!   `&'static str` skill identifier for dependency lookups.
//! - [`ExecutionState`] holds all runtime state, keeping `WorkflowNode` focused
//!   on immutable metadata.

use std::sync::Arc;
use std::time::Instant;

use crate::core::{
    ids::NodeId as TypedNodeId,
    skill::{Skill, SkillDefinition, SkillRef},
};

pub use crate::core::workflow::state::NodeState;

pub type NodeId = &'static str;

#[derive(Debug)]
pub struct ExecutionState {
    state: NodeState,
    attempts: u32,
    started_at: Option<Instant>,
    finished_at: Option<Instant>,
    last_error: Option<anyhow::Error>,
}

impl ExecutionState {
    #[inline]
    pub fn new() -> Self {
        Self {
            state: NodeState::Pending,
            attempts: 0,
            started_at: None,
            finished_at: None,
            last_error: None,
        }
    }

    #[inline]
    pub fn state(&self) -> &NodeState {
        &self.state
    }

    #[inline]
    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    #[inline]
    pub fn started_at(&self) -> Option<Instant> {
        self.started_at
    }

    #[inline]
    pub fn finished_at(&self) -> Option<Instant> {
        self.finished_at
    }

    #[inline]
    pub fn last_error(&self) -> Option<&anyhow::Error> {
        self.last_error.as_ref()
    }
}

impl Default for ExecutionState {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

pub struct WorkflowNode<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    skill: SkillRef<I>,
    instance_id: TypedNodeId,
    skill_id: &'static str,
    execution: ExecutionState,
}

impl<I> WorkflowNode<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    #[inline]
    pub fn new(skill: SkillRef<I>) -> Self {
        let skill_id = skill.definition().id;
        Self {
            skill,
            instance_id: TypedNodeId::new(),
            skill_id,
            execution: ExecutionState::new(),
        }
    }

    #[inline]
    pub fn skill(&self) -> &SkillRef<I> {
        &self.skill
    }

    #[inline]
    pub fn instance_id(&self) -> TypedNodeId {
        self.instance_id
    }

    #[inline]
    pub fn id(&self) -> NodeId {
        self.skill_id
    }

    #[inline]
    pub fn definition(&self) -> &'static SkillDefinition {
        self.skill.definition()
    }

    #[inline]
    pub fn state(&self) -> &NodeState {
        self.execution.state()
    }

    #[inline]
    pub fn execution(&self) -> &ExecutionState {
        &self.execution
    }

    #[inline]
    pub fn is_pending(&self) -> bool {
        self.execution.state == NodeState::Pending
    }

    #[inline]
    pub fn is_ready(&self) -> bool {
        self.execution.state == NodeState::Ready
    }

    #[inline]
    pub fn is_running(&self) -> bool {
        self.execution.state == NodeState::Running
    }

    #[inline]
    pub fn is_completed(&self) -> bool {
        self.execution.state == NodeState::Completed
    }

    #[inline]
    pub fn is_failed(&self) -> bool {
        matches!(self.execution.state, NodeState::Failed { .. })
    }

    #[inline]
    pub fn is_skipped(&self) -> bool {
        self.execution.state == NodeState::Skipped
    }

    pub(crate) fn transition(&mut self, new_state: NodeState) {
        tracing::debug!(
            instance_id = %self.instance_id,
            skill_id = self.skill_id,
            from = %self.execution.state,
            to = %new_state,
            "Node state transition"
        );

        match &new_state {
            NodeState::Running => {
                self.execution.started_at = Some(Instant::now());
                self.execution.attempts += 1;
            }
            NodeState::Completed
            | NodeState::Failed { .. }
            | NodeState::Skipped
            | NodeState::Cancelled => {
                self.execution.finished_at = Some(Instant::now());
            }
            NodeState::Pending | NodeState::Ready => {}
        }

        if let NodeState::Failed { reason } = &new_state {
            self.execution.last_error = Some(anyhow::anyhow!("{}", reason));
        }

        self.execution.state = new_state;
    }
}

impl<I> std::fmt::Debug for WorkflowNode<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkflowNode")
            .field("id", &self.id())
            .field("instance_id", &self.instance_id)
            .field("state", &self.execution.state)
            .finish()
    }
}

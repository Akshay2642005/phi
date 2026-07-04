//! Workflow graph node.
//!
//! A [`WorkflowNode`] is a skill wrapped with execution-level metadata so
//! the graph, scheduler, and executor can reason about it without reaching
//! into the skill's internal state.
//!
//! # Design
//! - One node per skill registration.
//! - Nodes carry both a typed [`NodeId`] (UUID instance ID) and a
//!   `&'static str` skill identifier for dependency lookups.
//! - [`NodeState`] (from [`crate::core::workflow::state`]) drives the
//!   execution lifecycle; the scheduler owns transitions.

use std::sync::Arc;

use crate::core::{
    ids::NodeId as TypedNodeId,
    skill::{Skill, SkillDefinition, SkillRef},
};

pub use crate::core::workflow::state::NodeState;

// ── Node identifier ───────────────────────────────────────────────────────────

/// The unique identifier for a node in the workflow graph, used as a key
/// for dependency lookups and slot-map indexing.
///
/// Currently aliased to `&'static str` (the skill's static ID) so that
/// dependency declarations are directly usable.  The typed [`TypedNodeId`]
/// (UUID) is available via [`WorkflowNode::instance_id`] and will become
/// the primary identifier in a future release.
pub type NodeId = &'static str;

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

    /// Unique UUID-based instance identifier for this node.
    ///
    /// Every node in the graph gets a fresh [`TypedNodeId`] at creation.
    /// This is currently unused by the scheduler/executor but exists so that
    /// future features (multi-instance skills, checkpointing, distributed
    /// execution) can distinguish node *instances* from skill *definitions*.
    instance_id: TypedNodeId,

    /// The skill's static string identifier (for dependency lookups and
    /// backward compatibility).
    skill_id: &'static str,
}

impl<I> WorkflowNode<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    /// Wrap a skill in a new node starting in the [`NodeState::Pending`] state.
    pub fn new(skill: Arc<dyn Skill<I>>) -> Self {
        let skill_id = skill.definition().id;
        Self {
            skill,
            state: NodeState::Pending,
            instance_id: TypedNodeId::new(),
            skill_id,
        }
    }

    /// Instance UUID — unique across all nodes in all runs.
    ///
    /// Use this when you need a globally unique identity for this node
    /// instance.  For dependency/slot-map lookups, use [`Self::id`] instead.
    #[inline]
    pub fn instance_id(&self) -> TypedNodeId {
        self.instance_id
    }

    /// Return the node's string identifier (= the skill's static `id`).
    #[inline]
    pub fn id(&self) -> NodeId {
        self.skill_id
    }

    /// Convenience accessor for the node's [`SkillDefinition`].
    #[inline]
    pub fn definition(&self) -> &'static SkillDefinition {
        self.skill.definition()
    }

    /// Transition the node to a new state.
    ///
    /// Logs a `DEBUG` trace for every transition.  In a future version this
    /// will also emit a [`crate::core::events::EngineEvent`].
    pub fn transition(&mut self, new_state: NodeState) {
        tracing::debug!(
            instance_id = %self.instance_id,
            skill_id = self.skill_id,
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
            .field("instance_id", &self.instance_id)
            .field("state", &self.state)
            .finish()
    }
}

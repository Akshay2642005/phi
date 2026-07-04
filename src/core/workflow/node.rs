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

// ── Node identifier ───────────────────────────────────────────────────────────

/// The unique identifier for a node in the workflow graph, used as a key
/// for dependency lookups and slot-map indexing.
///
/// Currently aliased to `&'static str` (the skill's static ID) so that
/// dependency declarations are directly usable.  The typed [`TypedNodeId`]
/// (UUID) is available via [`WorkflowNode::instance_id`] and will become
/// the primary identifier in a future release.
pub type NodeId = &'static str;

// ── Execution state ───────────────────────────────────────────────────────────

/// Mutable runtime state for a single workflow node.
///
/// This struct holds everything that can change during a node's lifecycle.
/// It is deliberately separated from [`WorkflowNode`] so that the immutable
/// metadata (skill reference, identifiers) never mixes with runtime state.
///
/// # Future extensibility
/// Add fields here — metrics, tracing spans, timeout trackers, cancellation
/// tokens, checkpointing state, output handles, diagnostics — without ever
/// touching the `WorkflowNode` type.  The node owns one `ExecutionState` and
/// delegates all state transitions to it.
///
/// # Access control
/// - **Read**: exposed through [`WorkflowNode::execution`] (returns `&ExecutionState`).
/// - **Write**: only through [`WorkflowNode::transition`], which is `pub(crate)`.
///   External code cannot mutate execution state directly.
#[derive(Debug)]
pub struct ExecutionState {
    /// Current lifecycle state (drives the scheduler and executor).
    state: NodeState,

    /// Number of times the node has transitioned to `Running`.
    ///
    /// This reflects the number of distinct execution attempts.  Internal
    /// retries within a single `Running` period (handled by the executor's
    /// retry policy) do not increment this counter — only a fresh
    /// `Ready → Running` transition does.
    attempts: u32,

    /// Wall-clock time when the node most recently entered `Running`.
    started_at: Option<Instant>,

    /// Wall-clock time when the node most recently entered a terminal state
    /// (`Completed`, `Failed`, `Skipped`, or `Cancelled`).
    finished_at: Option<Instant>,

    /// The error from the last failed attempt, captured when transitioning
    /// to `Failed { .. }`.
    last_error: Option<anyhow::Error>,
}

impl ExecutionState {
    /// Create a fresh execution state starting in [`NodeState::Pending`].
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

    // ── Read accessors ────────────────────────────────────────────────────────

    /// Current node lifecycle state.
    #[inline]
    pub fn state(&self) -> &NodeState {
        &self.state
    }

    /// Number of distinct execution attempts so far.
    #[inline]
    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    /// When the node most recently started executing, if ever.
    #[inline]
    pub fn started_at(&self) -> Option<Instant> {
        self.started_at
    }

    /// When the node most recently finished executing, if ever.
    #[inline]
    pub fn finished_at(&self) -> Option<Instant> {
        self.finished_at
    }

    /// The error from the last failed attempt, if any.
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

// ── Workflow node ─────────────────────────────────────────────────────────────

/// A skill wrapped with graph-level and execution-level metadata.
///
/// `I` is the domain input type (see [`Skill<I>`]).
///
/// # Immutable metadata
/// - [`skill`](WorkflowNode::skill) — the wrapped skill reference.
/// - [`instance_id`](WorkflowNode::instance_id) — UUID unique to this node instance.
/// - [`id`](WorkflowNode::id) — the skill's `&'static str` identifier.
/// - [`definition`](WorkflowNode::definition) — the skill's static [`SkillDefinition`].
///
/// # Mutable execution state
/// - [`execution`](WorkflowNode::execution) — an [`ExecutionState`] holding all
///   runtime state.  Mutated only through the `pub(crate)` [`transition`](WorkflowNode::transition)
///   API.
///
/// # Thread safety
/// `WorkflowNode<I>` is `Send + Sync` when `I: Send + Sync + Debug + 'static`.
/// All mutation is explicit and single-threaded by design; the node will be
/// wrapped in `Arc<Mutex<…>>` or similar when the scheduler moves to parallel
/// execution.
pub struct WorkflowNode<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    /// The wrapped skill — immutable after construction.
    skill: SkillRef<I>,

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

    /// Runtime execution state — mutable, owned.
    execution: ExecutionState,
}

impl<I> WorkflowNode<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    // ── Constructor ────────────────────────────────────────────────────────────

    /// Wrap a skill in a new node starting in the [`NodeState::Pending`] state.
    ///
    /// Accepts a [`SkillRef<I>`] (alias for `Arc<dyn Skill<I>>`), which is the
    /// standard way to hold skills in the engine.
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

    // ── Immutable metadata accessors ───────────────────────────────────────────

    /// Reference to the wrapped skill.
    ///
    /// Returns `&SkillRef<I>` (i.e. `&Arc<dyn Skill<I>>`).  Clone the `Arc`
    /// if an owned reference is needed.
    #[inline]
    pub fn skill(&self) -> &SkillRef<I> {
        &self.skill
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

    // ── Execution state accessors ──────────────────────────────────────────────

    /// Current node lifecycle state.
    ///
    /// Returns a reference to the inner [`NodeState`].  For boolean checks,
    /// prefer the convenience helpers (`is_pending`, `is_ready`, …).
    #[inline]
    pub fn state(&self) -> &NodeState {
        self.execution.state()
    }

    /// Borrow the full [`ExecutionState`] (read-only).
    ///
    /// Use this when you need access to fields beyond `state`, such as
    /// `attempts()`, `started_at()`, `finished_at()`, or `last_error()`.
    #[inline]
    pub fn execution(&self) -> &ExecutionState {
        &self.execution
    }

    // ── Convenience helpers ────────────────────────────────────────────────────

    /// `true` when the node is waiting for dependencies to complete.
    #[inline]
    pub fn is_pending(&self) -> bool {
        self.execution.state == NodeState::Pending
    }

    /// `true` when all dependencies are met and the node is ready to execute.
    #[inline]
    pub fn is_ready(&self) -> bool {
        self.execution.state == NodeState::Ready
    }

    /// `true` when the node is currently executing.
    #[inline]
    pub fn is_running(&self) -> bool {
        self.execution.state == NodeState::Running
    }

    /// `true` when the node completed successfully.
    #[inline]
    pub fn is_completed(&self) -> bool {
        self.execution.state == NodeState::Completed
    }

    /// `true` when the node failed.
    #[inline]
    pub fn is_failed(&self) -> bool {
        matches!(self.execution.state, NodeState::Failed { .. })
    }

    /// `true` when the node was skipped (excluded by a conditional or
    /// cancelled).
    #[inline]
    pub fn is_skipped(&self) -> bool {
        self.execution.state == NodeState::Skipped
    }

    // ── State transitions ──────────────────────────────────────────────────────

    /// Transition the node to `new_state`.
    ///
    /// This is the **only** way to mutate a node's runtime state.  It:
    ///
    /// 1. Logs the transition at `DEBUG` level with structured fields.
    /// 2. Updates timers (`started_at` on `Running`, `finished_at` on terminal).
    /// 3. Increments the `attempts` counter on each `Running` transition.
    /// 4. Captures the error reason on `Failed { .. }`.
    ///
    /// # Future
    /// This method is the natural insertion point for:
    /// - Emitting [`EngineEvent`]s via an optional event bus.
    /// - Updating execution metrics (histograms, counters).
    /// - Validating that `new_state` is a legal transition from the current
    ///   state (state-machine validation).
    /// - Recording checkpoint data for persistence and replay.
    ///
    /// # Visibility
    /// `pub(crate)` — intended for use by the scheduler and executor only.
    /// External code observes state through the read-only accessors above.
    pub(crate) fn transition(&mut self, new_state: NodeState) {
        tracing::debug!(
            instance_id = %self.instance_id,
            skill_id = self.skill_id,
            from = %self.execution.state,
            to = %new_state,
            "Node state transition"
        );

        // Update timers and counters based on the target state.
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
            NodeState::Pending | NodeState::Ready => {
                // No timer updates for intermediate states.
            }
        }

        // Capture the error when transitioning to Failed.
        if let NodeState::Failed { reason } = &new_state {
            // Preserve the existing error if we already have one from a
            // previous attempt; otherwise create a new one from the reason.
            self.execution.last_error = Some(anyhow::anyhow!("{}", reason));
        }

        self.execution.state = new_state;
    }
}

// ── Debug ─────────────────────────────────────────────────────────────────────

impl<I> std::fmt::Debug for WorkflowNode<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    /// Formats the node without printing the wrapped skill.
    ///
    /// Only the node's id, instance id, and current state are shown — the
    /// skill is omitted to keep output concise and to avoid invoking
    /// `Skill::fmt` (which may be expensive or side-effecting).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkflowNode")
            .field("id", &self.id())
            .field("instance_id", &self.instance_id)
            .field("state", &self.execution.state)
            .finish()
    }
}

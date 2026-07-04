//! Workflow and node state machines.
//!
//! Every workflow execution and every node within it progresses through a
//! well-defined lifecycle.  Strong enums replace boolean flags and ad-hoc
//! string comparisons.
//!
//! # State diagrams
//!
//! ## WorkflowState
//! ```text
//! Pending
//!   └─► Planning
//!         └─► Ready
//!               └─► Running
//!                     ├─► Waiting  (paused, e.g. human-in-the-loop)
//!                     │     └─► Running  (resumed)
//!                     ├─► Completed
//!                     ├─► Failed
//!                     └─► Cancelled
//! ```
//!
//! ## NodeState
//! ```text
//! Pending
//!   └─► Ready
//!         └─► Running
//!               ├─► Completed
//!               ├─► Failed
//!               ├─► Skipped
//!               └─► Cancelled
//! ```

use std::fmt;

use serde::{Deserialize, Serialize};

// ── WorkflowState ─────────────────────────────────────────────────────────────

/// Execution lifecycle state of an entire workflow run.
///
/// Drives the engine's top-level state machine.  The scheduler and executor
/// transition the workflow through these states; external observers (event
/// bus, monitoring) read it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkflowState {
    /// Workflow has been created but the planner has not started yet.
    Pending,

    /// The planner is building the execution plan and dependency graph.
    Planning,

    /// The plan is ready; waiting for the first execution tick.
    ///
    /// This state exists so that pre-flight validation can happen between
    /// planning and the first skill execution.
    Ready,

    /// One or more skills are currently executing.
    Running,

    /// Execution is paused, waiting for an external signal.
    ///
    /// Used by human-in-the-loop nodes, approval gates, or external
    /// callbacks.  Transitions back to `Running` when the signal arrives.
    Waiting,

    /// All skills completed successfully.
    Completed,

    /// At least one skill failed and the error policy did not allow
    /// continuation.
    Failed,

    /// The workflow was cancelled before completion.
    Cancelled,
}

impl WorkflowState {
    /// `true` if the workflow has reached a terminal state and will not
    /// progress further.
    #[inline]
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            WorkflowState::Completed | WorkflowState::Failed | WorkflowState::Cancelled
        )
    }

    /// `true` if the workflow completed without errors.
    #[inline]
    pub fn is_success(&self) -> bool {
        matches!(self, WorkflowState::Completed)
    }

    /// `true` if the workflow is actively executing (Running or Waiting).
    #[inline]
    pub fn is_active(&self) -> bool {
        matches!(self, WorkflowState::Running | WorkflowState::Waiting)
    }
}

impl fmt::Display for WorkflowState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkflowState::Pending => write!(f, "Pending"),
            WorkflowState::Planning => write!(f, "Planning"),
            WorkflowState::Ready => write!(f, "Ready"),
            WorkflowState::Running => write!(f, "Running"),
            WorkflowState::Waiting => write!(f, "Waiting"),
            WorkflowState::Completed => write!(f, "Completed"),
            WorkflowState::Failed => write!(f, "Failed"),
            WorkflowState::Cancelled => write!(f, "Cancelled"),
        }
    }
}

// ── NodeState ─────────────────────────────────────────────────────────────────

/// Execution lifecycle state of a single node (skill instance) in the graph.
///
/// The scheduler drives transitions from `Pending` → `Ready`; the executor
/// drives `Ready` → `Running` → terminal.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeState {
    /// Waiting for one or more dependencies to complete.
    Pending,

    /// All dependencies have completed; ready to be dispatched to the executor.
    Ready,

    /// Currently executing.
    Running,

    /// Execution completed successfully.
    Completed,

    /// Skipped — either the node was excluded by a conditional, or a
    /// preceding node failed under a `continue_on_failure` policy.
    Skipped,

    /// Execution failed.
    ///
    /// `reason` is a human-readable message preserved for logs and
    /// diagnostics.  Avoid storing structured data here; emit an event
    /// instead.
    Failed {
        /// Human-readable failure reason.
        reason: String,
    },

    /// The execution was cancelled before this node ran or while it was
    /// running.
    Cancelled,
}

impl NodeState {
    /// `true` if the node has reached a terminal state and will not
    /// transition further.
    #[inline]
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            NodeState::Completed
                | NodeState::Failed { .. }
                | NodeState::Skipped
                | NodeState::Cancelled
        )
    }

    /// `true` if the node completed without errors.
    #[inline]
    pub fn is_success(&self) -> bool {
        matches!(self, NodeState::Completed)
    }

    /// `true` if the node is actively executing.
    #[inline]
    pub fn is_active(&self) -> bool {
        matches!(self, NodeState::Running)
    }
}

impl fmt::Display for NodeState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeState::Pending => write!(f, "Pending"),
            NodeState::Ready => write!(f, "Ready"),
            NodeState::Running => write!(f, "Running"),
            NodeState::Completed => write!(f, "Completed"),
            NodeState::Skipped => write!(f, "Skipped"),
            NodeState::Failed { reason } => write!(f, "Failed({})", reason),
            NodeState::Cancelled => write!(f, "Cancelled"),
        }
    }
}

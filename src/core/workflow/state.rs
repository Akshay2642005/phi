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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkflowState {
    Pending,
    Planning,
    Ready,
    Running,
    Waiting,
    Completed,
    Failed,
    Cancelled,
}

impl WorkflowState {
    #[inline]
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            WorkflowState::Completed | WorkflowState::Failed | WorkflowState::Cancelled
        )
    }

    #[inline]
    pub fn is_success(&self) -> bool {
        matches!(self, WorkflowState::Completed)
    }

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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeState {
    Pending,
    Ready,
    Running,
    Completed,
    Skipped,
    Failed {
        reason: String,
    },
    Cancelled,
}

impl NodeState {
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

    #[inline]
    pub fn is_success(&self) -> bool {
        matches!(self, NodeState::Completed)
    }

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

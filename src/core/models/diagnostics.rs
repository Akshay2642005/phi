//! Structured diagnostic types.
//!
//! Skills and the engine emit [`WorkflowDiagnostic`] entries during a run.
//! These are collected in the context and summarised in the
//! [`ExecutionSummary`].
//!
//! Diagnostics are *informational*: they do not cause the workflow to fail.
//! For failure signalling, return `Err` from `Skill::execute`.

use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::core::workflow::state::NodeState;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DiagnosticLevel {
    Info,
    Warning,
    Error,
}

impl std::fmt::Display for DiagnosticLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiagnosticLevel::Info => write!(f, "INFO"),
            DiagnosticLevel::Warning => write!(f, "WARN"),
            DiagnosticLevel::Error => write!(f, "ERROR"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDiagnostic {
    pub skill_id: Option<String>,
    pub message: String,
    pub level: DiagnosticLevel,
    pub tags: Vec<(String, String)>,
}

// ---------------------------------------------------------------------------
// NodeDiagnostics — per-node execution summary
// ---------------------------------------------------------------------------

/// Per-node diagnostics captured after each skill execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDiagnostics {
    /// The skill / node identifier.
    pub skill_id: String,
    /// Terminal state after execution.
    pub status: NodeState,
    /// Wall-clock start time (monotonic system time).
    pub start_time: SystemTime,
    /// Wall-clock end time (set when the node reaches a terminal state).
    pub end_time: Option<SystemTime>,
    /// Execution wall-clock duration in milliseconds.
    pub duration_ms: u64,
    /// Number of retry attempts consumed (0 = first attempt succeeded).
    pub retry_count: u32,
    /// Captured error message on failure.
    pub error: Option<String>,
    /// Warning messages emitted during execution.
    pub warnings: Vec<String>,
    /// Keys written to the context slot map by this node.
    pub output_keys: Vec<String>,
}

impl NodeDiagnostics {
    pub fn new(skill_id: impl Into<String>) -> Self {
        Self {
            skill_id: skill_id.into(),
            status: NodeState::Running,
            start_time: SystemTime::now(),
            end_time: None,
            duration_ms: 0,
            retry_count: 0,
            error: None,
            warnings: Vec::new(),
            output_keys: Vec::new(),
        }
    }

    /// Finalise the diagnostic entry when the node reaches a terminal state.
    pub fn finalize(
        mut self,
        status: NodeState,
        retry_count: u32,
        error: Option<String>,
        warnings: Vec<String>,
        output_keys: Vec<String>,
        duration_ms: u64,
    ) -> Self {
        self.status = status;
        self.end_time = Some(SystemTime::now());
        self.retry_count = retry_count;
        self.error = error;
        self.warnings = warnings;
        self.output_keys = output_keys;
        self.duration_ms = duration_ms;
        self
    }
}

impl WorkflowDiagnostic {
    pub fn info(skill_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            skill_id: Some(skill_id.into()),
            message: message.into(),
            level: DiagnosticLevel::Info,
            tags: Vec::new(),
        }
    }

    pub fn warn(skill_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            skill_id: Some(skill_id.into()),
            message: message.into(),
            level: DiagnosticLevel::Warning,
            tags: Vec::new(),
        }
    }

    pub fn error(skill_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            skill_id: Some(skill_id.into()),
            message: message.into(),
            level: DiagnosticLevel::Error,
            tags: Vec::new(),
        }
    }

    pub fn engine(level: DiagnosticLevel, message: impl Into<String>) -> Self {
        Self {
            skill_id: None,
            message: message.into(),
            level,
            tags: Vec::new(),
        }
    }

    pub fn with_tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.push((key.into(), value.into()));
        self
    }
}

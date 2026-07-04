//! Structured diagnostic types.
//!
//! Skills and the engine emit [`WorkflowDiagnostic`] entries during a run.
//! These are collected in the context and summarised in the
//! [`ExecutionSummary`].
//!
//! Diagnostics are *informational*: they do not cause the workflow to fail.
//! For failure signalling, return `Err` from `Skill::execute`.

use serde::{Deserialize, Serialize};

// ── Severity ──────────────────────────────────────────────────────────────────

/// Severity level of a [`WorkflowDiagnostic`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DiagnosticLevel {
    /// Informational — normal operating notes.
    Info,
    /// Warning — something unexpected but non-fatal occurred.
    Warning,
    /// Error — a recoverable problem; included here when execution continued
    /// past the failure (e.g., `continue_on_failure` policy).
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

// ── Diagnostic entry ──────────────────────────────────────────────────────────

/// A single structured diagnostic message emitted during a workflow run.
///
/// Replaces the old `DiagnosticEntry` in `workflow_context.rs` with a version
/// that references typed IDs and carries structured metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDiagnostic {
    /// The skill that produced this message (if skill-scoped).
    pub skill_id: Option<String>,

    /// Human-readable message.
    pub message: String,

    /// Severity level.
    pub level: DiagnosticLevel,

    /// Optional structured metadata (kept as string pairs to avoid
    /// `serde_json::Value` in core).
    pub tags: Vec<(String, String)>,
}

impl WorkflowDiagnostic {
    /// Create an info-level diagnostic from a skill.
    pub fn info(skill_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            skill_id: Some(skill_id.into()),
            message: message.into(),
            level: DiagnosticLevel::Info,
            tags: Vec::new(),
        }
    }

    /// Create a warning-level diagnostic from a skill.
    pub fn warn(skill_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            skill_id: Some(skill_id.into()),
            message: message.into(),
            level: DiagnosticLevel::Warning,
            tags: Vec::new(),
        }
    }

    /// Create an error-level diagnostic from a skill.
    pub fn error(skill_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            skill_id: Some(skill_id.into()),
            message: message.into(),
            level: DiagnosticLevel::Error,
            tags: Vec::new(),
        }
    }

    /// Create an engine-level diagnostic (no skill scope).
    pub fn engine(level: DiagnosticLevel, message: impl Into<String>) -> Self {
        Self {
            skill_id: None,
            message: message.into(),
            level,
            tags: Vec::new(),
        }
    }

    /// Attach a key/value tag.
    pub fn with_tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.push((key.into(), value.into()));
        self
    }
}

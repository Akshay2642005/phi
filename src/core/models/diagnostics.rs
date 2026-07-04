//! Structured diagnostic types.
//!
//! Skills and the engine emit [`WorkflowDiagnostic`] entries during a run.
//! These are collected in the context and summarised in the
//! [`ExecutionSummary`].
//!
//! Diagnostics are *informational*: they do not cause the workflow to fail.
//! For failure signalling, return `Err` from `Skill::execute`.

use serde::{Deserialize, Serialize};

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

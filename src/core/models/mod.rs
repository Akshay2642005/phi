//! Shared engine models.
//!
//! Types in this module are used across multiple engine subsystems.  Keeping
//! them here prevents duplicate definitions and circular imports.
//!
//! # Contents
//! | Type | Description |
//! |------|-------------|
//! | [`ExecutionSummary`] | Outcome of a completed workflow run |
//! | [`SkillOutcome`] | Per-skill execution result |
//! | [`ExecutionMetadata`] | Timing and identity metadata for a run |
//! | [`WorkflowDiagnostic`] | A structured diagnostic entry |
//! | [`DiagnosticLevel`] | Severity of a diagnostic |
//! | [`LifecycleTimestamps`] | Timestamps for workflow lifecycle transitions |

pub mod diagnostics;
pub mod execution;

pub use diagnostics::{DiagnosticLevel, WorkflowDiagnostic};
pub use execution::{ExecutionMetadata, ExecutionSummary, LifecycleTimestamps, SkillOutcome};

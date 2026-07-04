//! Workflow context — shared state for a single workflow run.

pub mod workflow_context;

pub use workflow_context::{
    CancellationSender, CancellationToken, DiagnosticEntry, SlotMap, WorkflowContext,
};

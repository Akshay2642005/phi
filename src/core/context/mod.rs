//! Workflow context — shared state for a single workflow run.
//!
//! The key type here is [`WorkflowContext`], which is generic over the
//! domain input type `I`.

pub mod workflow_context;

pub use workflow_context::{
    CancellationSender, CancellationToken, DiagnosticEntry, DiagnosticLevel, SlotMap,
    WorkflowContext,
};

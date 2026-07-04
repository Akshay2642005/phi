//! Workflow lifecycle events — bridge to the core event bus.
//!
//! This module re-exports the canonical event types from [`crate::core::events`]
//! for use within the workflow subsystem, and provides the legacy
//! [`WorkflowEventSink`] adapter trait for backwards compatibility during the
//! migration to the full event bus.
//!
//! # Migration path
//! Old code that uses `WorkflowEventSink` continues to compile.  New code
//! should subscribe to the [`EventBus`] directly via
//! `WorkflowEngine::with_event_bus`.

pub use crate::core::events::{
    EngineEvent, ExecutionPlanBuiltEvent, SkillCompletedEvent, SkillFailedEvent, SkillStartedEvent,
    WorkflowCancelledEvent, WorkflowCompletedEvent, WorkflowFailedEvent, WorkflowStartedEvent,
};

pub trait WorkflowEventSink: Send + Sync {
    fn on_event(&self, event: EngineEvent);
}

pub struct NoopEventSink;

impl WorkflowEventSink for NoopEventSink {
    fn on_event(&self, _event: EngineEvent) {}
}

pub struct TracingEventSink;

impl WorkflowEventSink for TracingEventSink {
    fn on_event(&self, event: EngineEvent) {
        tracing::debug!(event = ?event, "EngineEvent");
    }
}

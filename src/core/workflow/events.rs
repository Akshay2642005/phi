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

// ── Legacy sink (kept for backwards compatibility) ────────────────────────────

/// Receive [`EngineEvent`]s from the engine.
///
/// Implementations must be `Send + Sync` because the engine may (in future)
/// emit events from async tasks.
///
/// # Deprecation notice
/// Prefer subscribing to the [`crate::core::events::EventBus`] directly.
/// This trait will be removed once all call sites are migrated.
pub trait WorkflowEventSink: Send + Sync {
    /// Called synchronously by the engine whenever an event occurs.
    fn on_event(&self, event: EngineEvent);
}

// ── No-op sink ────────────────────────────────────────────────────────────────

/// A sink that discards all events.
pub struct NoopEventSink;

impl WorkflowEventSink for NoopEventSink {
    fn on_event(&self, _event: EngineEvent) {}
}

/// A sink that logs every event at `DEBUG` level via `tracing`.
pub struct TracingEventSink;

impl WorkflowEventSink for TracingEventSink {
    fn on_event(&self, event: EngineEvent) {
        tracing::debug!(event = ?event, "EngineEvent");
    }
}

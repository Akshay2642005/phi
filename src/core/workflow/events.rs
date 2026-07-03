//! Workflow lifecycle events.
//!
//! Events are emitted by the engine at key lifecycle moments.  The engine
//! itself does not depend on any concrete listener — it calls an optional
//! [`WorkflowEventSink`] if one is attached.
//!
//! # Current status
//! This is an **interface stub**.  The `WorkflowEventSink` trait is fully
//! defined so external code can implement listeners (e.g., logging, metrics,
//! UI progress, audit log).  No built-in implementations are shipped yet.
//!
//! # Adding a listener
//! ```ignore
//! struct MyListener;
//! impl WorkflowEventSink for MyListener {
//!     fn on_event(&self, event: WorkflowEvent) {
//!         println!("{:?}", event);
//!     }
//! }
//!
//! let engine = WorkflowEngine::new()
//!     .with_event_sink(Box::new(MyListener));
//! ```

// ── Events ────────────────────────────────────────────────────────────────────

/// A lifecycle event emitted by the [`WorkflowEngine`].
#[derive(Debug, Clone)]
pub enum WorkflowEvent {
    /// Emitted when a workflow run begins.
    WorkflowStarted {
        /// The UUID of the run.
        run_id: String,
    },

    /// Emitted immediately before a skill begins executing.
    SkillStarted {
        run_id: String,
        skill_id: String,
    },

    /// Emitted immediately after a skill finishes (success or failure).
    SkillCompleted {
        run_id: String,
        skill_id: String,
        success: bool,
    },

    /// Emitted when every skill in the plan has reached a terminal state.
    WorkflowCompleted {
        run_id: String,
        success: bool,
    },

    /// Emitted when the workflow stops early due to an error or cancellation.
    WorkflowFailed {
        run_id: String,
        reason: String,
    },
}

// ── Sink trait ────────────────────────────────────────────────────────────────

/// Receive [`WorkflowEvent`]s from the engine.
///
/// Implementations must be `Send + Sync` because the engine may (in future)
/// emit events from async tasks.
pub trait WorkflowEventSink: Send + Sync {
    /// Called synchronously by the engine whenever an event occurs.
    fn on_event(&self, event: WorkflowEvent);
}

// ── No-op sink ────────────────────────────────────────────────────────────────

/// A sink that discards all events.  Used internally by the engine when no
/// external sink is attached.
pub struct NoopEventSink;

impl WorkflowEventSink for NoopEventSink {
    fn on_event(&self, _event: WorkflowEvent) {}
}

/// A sink that logs every event at `DEBUG` level via `tracing`.
pub struct TracingEventSink;

impl WorkflowEventSink for TracingEventSink {
    fn on_event(&self, event: WorkflowEvent) {
        tracing::debug!(event = ?event, "WorkflowEvent");
    }
}

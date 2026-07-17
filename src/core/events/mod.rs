//! Event bus architecture for the AI Workflow Engine.
//!
//! The engine emits strongly typed events at every significant lifecycle
//! moment.  Rather than a single fat enum with `run_id: String` fields,
//! each event is its own struct — giving better discoverability, easier
//! pattern matching, and zero-cost field access.
//!
//! # Architecture (current: stub)
//! ```text
//!  Engine / Executor
//!       │  emit(event)
//!       ▼
//!   EventBus  ──── broadcast channel ────►  Listener A
//!                                      └──►  Listener B
//! ```
//!
//! The [`EventBus`] is a lightweight stub backed by a `tokio::sync::broadcast`
//! channel.  Subscriptions and persistent listeners are left for a future
//! iteration; the API surface is stable now.
//!
//! # Adding a listener
//! ```ignore
//! let bus = EventBus::new(256);
//! let mut rx = bus.subscribe();
//! tokio::spawn(async move {
//!     while let Ok(event) = rx.recv().await {
//!         println!("{:?}", event);
//!     }
//! });
//! ```
//!
//! # Emitting from the engine
//! ```ignore
//! bus.emit(EngineEvent::WorkflowStarted(WorkflowStartedEvent { ... }));
//! ```

pub mod bus;
pub mod event_types;

pub use bus::EventBus;
pub use event_types::{
    ArtifactCreatedEvent, EngineEvent, ExecutionPlanBuiltEvent, LlmRequestCompletedEvent,
    LlmRequestStartedEvent, NodeQueuedEvent, SkillCompletedEvent, SkillFailedEvent,
    SkillStartedEvent, ToolCompletedEvent, ToolInvokedEvent, WorkflowCancelledEvent,
    WorkflowCompletedEvent, WorkflowFailedEvent, WorkflowStartedEvent,
};

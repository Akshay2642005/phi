//! Workflow engine — the heart of the AI orchestration framework.
//!
//! # Module layout
//! | Module | Responsibility |
//! |--------|---------------|
//! | [`node`] | `WorkflowNode`, `NodeState`, `NodeId` |
//! | [`graph`] | `WorkflowGraph` — petgraph-backed DAG |
//! | [`dependency`] | `DependencyResolver` — validates + builds the DAG |
//! | [`planner`] | `WorkflowPlanner` + `ExecutionPlan` |
//! | [`scheduler`] | `WorkflowScheduler` — readiness + deadlock detection |
//! | [`execution`] | `WorkflowExecutor` — runs individual skills |
//! | [`engine`] | `WorkflowEngine` — top-level orchestrator |
//! | [`policy`] | `RetryPolicy`, `TimeoutPolicy` stubs |
//! | [`events`] | `WorkflowEvent`, `WorkflowEventSink` stubs |

pub mod dependency;
pub mod engine;
pub mod events;
pub mod execution;
pub mod graph;
pub mod node;
pub mod planner;
pub mod policy;
pub mod scheduler;

// ── Convenience re-exports ────────────────────────────────────────────────────

pub use engine::{WorkflowEngine, WorkflowResult};
pub use events::{NoopEventSink, TracingEventSink, WorkflowEvent, WorkflowEventSink};
pub use execution::WorkflowExecutor;
pub use graph::WorkflowGraph;
pub use node::{NodeId, NodeState, WorkflowNode};
pub use planner::{ExecutionPlan, WorkflowPlanner};
pub use policy::{RetryPolicy, TimeoutPolicy};
pub use scheduler::WorkflowScheduler;

//! Workflow engine — the heart of the AI orchestration framework.
//!
//! # Module layout
//! | Module | Responsibility |
//! |--------|---------------|
//! | [`definition`] | `Workflow<I>` — immutable workflow definition |
//! | [`builder`] | `WorkflowBuilder<I>` + `WorkflowBuildError` |
//! | [`metadata`] | `WorkflowMetadata` — descriptive metadata |
//! | [`state`] | `WorkflowState`, `NodeState` enums |
//! | [`node`] | `WorkflowNode`, `NodeId`, `ExecutionState` |
//! | [`graph`] | `WorkflowGraph` — petgraph-backed DAG |
//! | [`dependency`] | `DependencyResolver` — validates + builds the DAG |
//! | [`planner`] | `WorkflowPlanner` + `ExecutionPlan` |
//! | [`scheduler`] | `WorkflowScheduler` — readiness + deadlock detection |
//! | [`execution`] | `WorkflowExecutor` — runs individual skills |
//! | [`engine`] | `WorkflowEngine` — top-level orchestrator |
//! | [`policy`] | `RetryPolicy`, `TimeoutPolicy` stubs |
//! | [`events`] | `EngineEvent`, `WorkflowEventSink` stubs |

pub mod builder;
pub mod definition;
pub mod dependency;
pub mod engine;
pub mod events;
pub mod execution;
pub mod graph;
pub mod metadata;
pub mod node;
pub mod planner;
pub mod policy;
pub mod scheduler;
pub mod state;

pub use builder::{WorkflowBuildError, WorkflowBuilder};
pub use definition::Workflow;
pub use engine::{WorkflowEngine, WorkflowResult};
pub use events::{NoopEventSink, TracingEventSink, WorkflowEventSink};
pub use execution::WorkflowExecutor;
pub use graph::WorkflowGraph;
pub use metadata::WorkflowMetadata;
pub use node::{ExecutionState, NodeId, NodeState, WorkflowNode};
pub use planner::{ExecutionPlan, WorkflowPlanner};
pub use policy::{RetryPolicy, TimeoutPolicy};
pub use scheduler::WorkflowScheduler;
pub use state::WorkflowState;

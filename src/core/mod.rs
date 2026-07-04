//! Core engine — all domain-agnostic AI workflow abstractions.
//!
//! # Module map
//! | Module | Description |
//! |--------|-------------|
//! | [`ids`] | Strongly typed newtype IDs (`WorkflowId`, `ExecutionId`, `NodeId`, …) |
//! | [`errors`] | Typed domain errors (`WorkflowError`, `SkillError`, `EngineError`) |
//! | [`capability`] | `Capability` enum + `HasCapabilities` trait |
//! | [`llm`] | `LlmService` trait + `OllamaLlmService` |
//! | [`artifact`] | `Artifact` trait + `ArtifactRef` + `ArtifactMeta` |
//! | [`context`] | `WorkflowContext<I>` — shared run-time state |
//! | [`models`] | Shared engine models (`ExecutionSummary`, `SkillOutcome`, …) |
//! | [`events`] | Strongly typed `EngineEvent` structs + `EventBus` |
//! | [`skill`] | `Skill<I>` trait, `SkillRegistry<I>`, `SkillDefinition` |
//! | [`workflow`] | `WorkflowEngine`, planner, scheduler, executor, graph, state |
//! | [`plugin`] | `Plugin<I>` trait stub |

pub mod artifact;
pub mod capability;
pub mod context;
pub mod errors;
pub mod events;
pub mod ids;
pub mod llm;
pub mod models;
pub mod plugin;
pub mod skill;
pub mod workflow;

//! Core engine — all domain-agnostic AI workflow abstractions.
//!
//! | Module | Description |
//! |--------|-------------|
//! | [`errors`] | Typed domain errors (`WorkflowError`, `SkillError`, `EngineError`) |
//! | [`llm`] | `LlmService` trait + `OllamaLlmService` |
//! | [`artifact`] | `Artifact` trait + `ArtifactKind` |
//! | [`context`] | `WorkflowContext<I>` — shared run-time state |
//! | [`skill`] | `Skill<I>` trait, `SkillRegistry<I>`, `SkillDefinition` |
//! | [`workflow`] | `WorkflowEngine`, planner, scheduler, executor, graph |
//! | [`plugin`] | `Plugin<I>` trait stub |

pub mod artifact;
pub mod context;
pub mod errors;
pub mod llm;
pub mod plugin;
pub mod skill;
pub mod workflow;

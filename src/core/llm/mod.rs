//! LLM abstraction — provider-agnostic interface and implementations.
//!
//! Import [`LlmService`] and [`LlmServiceRef`] everywhere skills and the
//! engine store LLM references.
//!
//! Import [`LlmServiceExt`] (usually as `use crate::core::llm::LlmServiceExt as _`)
//! in skills that call `prompt_typed<T>`.
//!
//! Import [`OllamaLlmService`] only at the bootstrap / wiring layer.

pub mod service;

pub use service::{LlmService, LlmServiceExt, LlmServiceRef, OllamaLlmService};

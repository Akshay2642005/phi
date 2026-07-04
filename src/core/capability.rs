//! Capability system — declarative requirement and provision contracts.
//!
//! Capabilities allow skills, plugins, and workflows to declare what they
//! *need* and what they *provide* so the engine can validate compatibility
//! before execution begins.
//!
//! # Motivation
//! Without capability declarations the engine must either:
//! - Trust that every registered plugin provides what every skill expects, or
//! - Discover mismatches at runtime (skill fails because LLM is not wired).
//!
//! Capability declarations shift this discovery to startup time.
//!
//! # Usage (stub — not yet enforced)
//! ```ignore
//! impl HasCapabilities for MySkill {
//!     fn required_capabilities(&self) -> &[Capability] {
//!         &[Capability::Llm, Capability::VectorStore]
//!     }
//! }
//! ```
//!
//! The engine will call `required_capabilities()` before creating the
//! execution plan and return a [`WorkflowError`] if the environment cannot
//! satisfy them.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Capability {
    Llm,
    Embedding,
    VectorStore,
    Vision,
    Audio,
    Tool,
    Search,
    Http,
    Memory,
    Filesystem,
    Database,
    Browser,
    Custom(&'static str),
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Capability::Llm => write!(f, "llm"),
            Capability::Embedding => write!(f, "embedding"),
            Capability::VectorStore => write!(f, "vector_store"),
            Capability::Vision => write!(f, "vision"),
            Capability::Audio => write!(f, "audio"),
            Capability::Tool => write!(f, "tool"),
            Capability::Search => write!(f, "search"),
            Capability::Http => write!(f, "http"),
            Capability::Memory => write!(f, "memory"),
            Capability::Filesystem => write!(f, "filesystem"),
            Capability::Database => write!(f, "database"),
            Capability::Browser => write!(f, "browser"),
            Capability::Custom(name) => write!(f, "custom:{}", name),
        }
    }
}

pub trait HasCapabilities {
    fn required_capabilities(&self) -> &[Capability] {
        &[]
    }

    fn provided_capabilities(&self) -> &[Capability] {
        &[]
    }
}

#[derive(Debug, Default)]
pub struct CapabilitySet {
    capabilities: std::collections::HashSet<Capability>,
}

impl CapabilitySet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn provide(&mut self, cap: Capability) {
        self.capabilities.insert(cap);
    }

    pub fn provide_all(&mut self, caps: impl IntoIterator<Item = Capability>) {
        self.capabilities.extend(caps);
    }

    pub fn has(&self, cap: &Capability) -> bool {
        self.capabilities.contains(cap)
    }

    pub fn satisfies(&self, required: &[Capability]) -> bool {
        required.iter().all(|cap| self.has(cap))
    }

    pub fn missing<'a>(&self, required: &'a [Capability]) -> Vec<&'a Capability> {
        required.iter().filter(|cap| !self.has(cap)).collect()
    }
}

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

// ── Capability ────────────────────────────────────────────────────────────────

/// A named engine capability.
///
/// Use `Capability::Custom` for domain-specific capabilities that don't fit
/// any standard variant.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Capability {
    // ── AI / model capabilities ───────────────────────────────────────────────
    /// A large language model for text generation and reasoning.
    Llm,

    /// A text embedding model for semantic representation.
    Embedding,

    /// A vector store for approximate nearest-neighbour search.
    VectorStore,

    /// Vision / image understanding (multimodal models).
    Vision,

    /// Audio processing (speech-to-text, text-to-speech).
    Audio,

    // ── Tool capabilities ─────────────────────────────────────────────────────
    /// External tool invocation (function calling, code execution).
    Tool,

    /// Web / internet search capability.
    Search,

    /// HTTP/REST API access.
    Http,

    // ── Storage capabilities ──────────────────────────────────────────────────
    /// Persistent key-value or document memory.
    Memory,

    /// Local filesystem read/write.
    Filesystem,

    /// Relational or document database.
    Database,

    // ── Browser capabilities ──────────────────────────────────────────────────
    /// Headless browser / web scraping.
    Browser,

    // ── Extension point ───────────────────────────────────────────────────────
    /// A domain-specific capability not covered by the standard variants.
    ///
    /// The string key must be unique within the capability registry.
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

// ── HasCapabilities trait ─────────────────────────────────────────────────────

/// Declares the capabilities a component requires and provides.
///
/// Implement this on skills, plugins, and workflows to participate in
/// the engine's pre-flight capability validation.
///
/// # Default behaviour
/// Both methods return empty slices — components with no capability
/// requirements or provisions do not need to implement this trait manually.
pub trait HasCapabilities {
    /// Capabilities this component **requires** to function correctly.
    ///
    /// The engine checks that all required capabilities are present in the
    /// runtime environment before scheduling this component.
    fn required_capabilities(&self) -> &[Capability] {
        &[]
    }

    /// Capabilities this component **provides** to others.
    ///
    /// A plugin that wraps an LLM client should declare `[Capability::Llm]`
    /// here so dependent skills can discover it.
    fn provided_capabilities(&self) -> &[Capability] {
        &[]
    }
}

// ── CapabilitySet ─────────────────────────────────────────────────────────────

/// A set of capabilities available in the current runtime environment.
///
/// The bootstrap layer collects all `provided_capabilities()` from registered
/// plugins and stores them here.  The engine queries `CapabilitySet::satisfies`
/// before building the execution plan.
#[derive(Debug, Default)]
pub struct CapabilitySet {
    capabilities: std::collections::HashSet<Capability>,
}

impl CapabilitySet {
    /// Create an empty capability set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a capability as available.
    pub fn provide(&mut self, cap: Capability) {
        self.capabilities.insert(cap);
    }

    /// Register multiple capabilities at once.
    pub fn provide_all(&mut self, caps: impl IntoIterator<Item = Capability>) {
        self.capabilities.extend(caps);
    }

    /// `true` if `cap` is in the set.
    pub fn has(&self, cap: &Capability) -> bool {
        self.capabilities.contains(cap)
    }

    /// `true` if *all* required capabilities are satisfied by this set.
    pub fn satisfies(&self, required: &[Capability]) -> bool {
        required.iter().all(|cap| self.has(cap))
    }

    /// Returns the capabilities in `required` that are *not* satisfied.
    pub fn missing<'a>(&self, required: &'a [Capability]) -> Vec<&'a Capability> {
        required.iter().filter(|cap| !self.has(cap)).collect()
    }
}

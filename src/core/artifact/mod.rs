//! Generic artifact system.
//!
//! An artifact is any structured output produced by a workflow run.  The
//! engine knows only about the [`Artifact`] trait; concrete artifact types
//! live in the domain layer.
//!
//! # Design
//! - [`Artifact`] is a thin, serialisation-aware marker trait.
//! - [`ArtifactKind`] is a discriminant that lets callers inspect the type of
//!   an artifact without downcasting.
//! - [`ArtifactRef`] is the engine-internal representation: an `Arc`-wrapped
//!   `Box<dyn Artifact>` that can be moved cheaply across async tasks.

use std::sync::Arc;

use serde::{Serialize, de::DeserializeOwned};

// ── Core trait ────────────────────────────────────────────────────────────────

/// A typed, serialisable output produced by a workflow run.
///
/// All artifacts must be `Send + Sync` (required for cross-task sharing) and
/// serialisable to JSON (required for persistence and debugging).
///
/// # Implementing
/// ```ignore
/// #[derive(Debug, Serialize, Deserialize)]
/// struct MyArtifact { /* domain fields */ }
///
/// impl Artifact for MyArtifact {
///     fn kind(&self) -> ArtifactKind {
///         ArtifactKind::Custom("my_artifact")
///     }
///     fn is_complete(&self) -> bool { /* ... */ }
/// }
/// ```
pub trait Artifact: Send + Sync + erased_serde::Serialize + std::fmt::Debug {
    /// Discriminant identifying this artifact type.
    fn kind(&self) -> ArtifactKind;

    /// `true` when all required fields have been populated.
    ///
    /// The engine logs a warning when a workflow completes with an incomplete
    /// artifact, but does not treat it as an error — partial artifacts are
    /// valid (e.g., when a skill was skipped due to a conditional).
    fn is_complete(&self) -> bool;

    /// Human-readable summary suitable for log output.
    ///
    /// The default implementation delegates to the `Debug` representation.
    /// Override this for a more concise or domain-friendly message.
    fn summary(&self) -> String {
        format!("{:?}", self)
    }
}

// ── Artifact kind ─────────────────────────────────────────────────────────────

/// Identifies the type of an artifact without requiring a downcast.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ArtifactKind {
    /// Document-analysis workflow artifact.
    Document,
    /// Code-generation workflow artifact.
    Code,
    /// A domain-specific artifact identified by a static string key.
    Custom(&'static str),
}

// ── Reference type ────────────────────────────────────────────────────────────

/// A cheaply-cloneable, heap-allocated artifact that can be shared across
/// async tasks.
pub type ArtifactRef = Arc<Box<dyn Artifact>>;

/// Wrap any [`Artifact`] implementor in an [`ArtifactRef`].
pub fn into_artifact_ref(artifact: impl Artifact + 'static) -> ArtifactRef {
    Arc::new(Box::new(artifact))
}

// ── Serialisation support ─────────────────────────────────────────────────────

erased_serde::serialize_trait_object!(Artifact);

// ── Typed artifact output wrapper ─────────────────────────────────────────────

/// A named artifact slot produced by a skill.
///
/// Skills place their outputs into the workflow context via this wrapper so
/// that downstream skills and the result serialiser can locate them by name.
#[derive(Debug)]
pub struct ArtifactOutput<T>
where
    T: std::fmt::Debug + Serialize + DeserializeOwned + Send + Sync,
{
    /// Stable identifier for this output slot (matches `SkillDefinition.id`).
    pub skill_id: &'static str,
    /// The typed output value.
    pub value: T,
}

impl<T> ArtifactOutput<T>
where
    T: std::fmt::Debug + Serialize + DeserializeOwned + Send + Sync,
{
    /// Create a new artifact output for `skill_id`.
    pub fn new(skill_id: &'static str, value: T) -> Self {
        Self { skill_id, value }
    }
}

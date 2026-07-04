//! Generic artifact system.
//!
//! An artifact is any structured output produced by a workflow run.  The
//! engine knows only about the [`Artifact`] trait; concrete artifact types
//! live in the domain / workflow layer.
//!
//! # Design
//! - [`Artifact`] is a thin, serialisation-aware marker trait.
//! - [`ArtifactMeta`] is engine-managed metadata (id, skill, kind tag).
//! - The engine does **not** know whether an artifact is JSON, Markdown, HTML,
//!   PDF, an image, or any other format.  That is the workflow's concern.
//! - [`ArtifactRef`] is the engine-internal representation: a cheaply cloneable
//!   `Arc`-wrapped trait object.
//!
//! # Adding an artifact type (in a workflow)
//! ```ignore
//! #[derive(Debug, Serialize, Deserialize)]
//! struct SentimentReport { /* domain fields */ }
//!
//! impl Artifact for SentimentReport {
//!     fn kind_tag(&self) -> &'static str { "sentiment_report" }
//!     fn is_complete(&self) -> bool { true }
//! }
//! ```

use std::sync::Arc;

use serde::Serialize;

use crate::core::ids::ArtifactId;

// ── Core trait ────────────────────────────────────────────────────────────────

/// A typed, serialisable output produced by a workflow skill.
///
/// The engine treats all artifacts uniformly; the concrete type, schema, and
/// format are entirely the workflow's responsibility.
///
/// # Contract
/// - Implement [`is_complete`] to let the engine warn about partial outputs.
/// - The `erased_serde::Serialize` super-trait enables JSON serialisation for
///   debugging and persistence without the engine knowing the concrete type.
pub trait Artifact: Send + Sync + erased_serde::Serialize + std::fmt::Debug {
    /// A short, stable, domain-defined tag identifying the artifact type.
    ///
    /// Examples: `"word_count"`, `"sentiment_report"`, `"document_summary"`.
    ///
    /// This is a *workflow* concern — the engine never interprets it.
    fn kind_tag(&self) -> &'static str;

    /// `true` when all required fields have been populated.
    ///
    /// The engine logs a warning when a workflow completes with an incomplete
    /// artifact but does not treat it as an error.
    fn is_complete(&self) -> bool;

    /// Human-readable summary for log output.
    ///
    /// Default: delegates to `Debug`.  Override for a more concise message.
    fn summary(&self) -> String {
        format!("{:?}", self)
    }
}

erased_serde::serialize_trait_object!(Artifact);

// ── Artifact metadata ─────────────────────────────────────────────────────────

/// Engine-managed metadata attached to every artifact instance.
///
/// Stored alongside the artifact value so subscribers and the result
/// serialiser can identify it without downcasting.
#[derive(Debug, Clone)]
pub struct ArtifactMeta {
    /// Unique identifier for this artifact instance.
    pub artifact_id: ArtifactId,
    /// The skill that produced this artifact.
    pub skill_id: &'static str,
    /// Domain-defined kind tag (see [`Artifact::kind_tag`]).
    pub kind_tag: &'static str,
}

impl ArtifactMeta {
    /// Create metadata for a new artifact produced by `skill_id`.
    pub fn new(skill_id: &'static str, kind_tag: &'static str) -> Self {
        Self {
            artifact_id: ArtifactId::new(),
            skill_id,
            kind_tag,
        }
    }
}

// ── Reference type ────────────────────────────────────────────────────────────

/// A cheaply-cloneable, heap-allocated artifact that can be shared across
/// async tasks.
pub type ArtifactRef = Arc<Box<dyn Artifact>>;

/// Wrap any [`Artifact`] implementor in an [`ArtifactRef`].
pub fn into_artifact_ref(artifact: impl Artifact + 'static) -> ArtifactRef {
    Arc::new(Box::new(artifact))
}

// ── Typed artifact output wrapper ─────────────────────────────────────────────

/// A named artifact slot produced by a skill.
///
/// Skills place their outputs into the workflow context via this wrapper so
/// that downstream skills and the result serialiser can locate them by name.
#[derive(Debug)]
pub struct ArtifactOutput<T>
where
    T: std::fmt::Debug + Serialize + Send + Sync,
{
    /// Engine-managed metadata for this artifact.
    pub meta: ArtifactMeta,
    /// The typed output value.
    pub value: T,
}

impl<T> ArtifactOutput<T>
where
    T: std::fmt::Debug + Serialize + Send + Sync,
{
    /// Create a new artifact output.
    pub fn new(skill_id: &'static str, kind_tag: &'static str, value: T) -> Self {
        Self {
            meta: ArtifactMeta::new(skill_id, kind_tag),
            value,
        }
    }
}

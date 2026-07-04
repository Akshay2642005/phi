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

pub trait Artifact: Send + Sync + erased_serde::Serialize + std::fmt::Debug {
    fn kind_tag(&self) -> &'static str;
    fn is_complete(&self) -> bool;
    fn summary(&self) -> String {
        format!("{:?}", self)
    }
}

erased_serde::serialize_trait_object!(Artifact);

#[derive(Debug, Clone)]
pub struct ArtifactMeta {
    pub artifact_id: ArtifactId,
    pub skill_id: &'static str,
    pub kind_tag: &'static str,
}

impl ArtifactMeta {
    pub fn new(skill_id: &'static str, kind_tag: &'static str) -> Self {
        Self {
            artifact_id: ArtifactId::new(),
            skill_id,
            kind_tag,
        }
    }
}

pub type ArtifactRef = Arc<Box<dyn Artifact>>;

pub fn into_artifact_ref(artifact: impl Artifact + 'static) -> ArtifactRef {
    Arc::new(Box::new(artifact))
}

#[derive(Debug)]
pub struct ArtifactOutput<T>
where
    T: std::fmt::Debug + Serialize + Send + Sync,
{
    pub meta: ArtifactMeta,
    pub value: T,
}

impl<T> ArtifactOutput<T>
where
    T: std::fmt::Debug + Serialize + Send + Sync,
{
    pub fn new(skill_id: &'static str, kind_tag: &'static str, value: T) -> Self {
        Self {
            meta: ArtifactMeta::new(skill_id, kind_tag),
            value,
        }
    }
}

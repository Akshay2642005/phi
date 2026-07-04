//! Strongly typed identifier newtypes.
//!
//! Using raw `String` or `Uuid` everywhere leads to accidental misuse — a
//! `WorkflowId` passed where a `NodeId` is expected compiles fine but is
//! semantically wrong.  Newtype wrappers around [`uuid::Uuid`] give us
//! compile-time safety at zero runtime cost.
//!
//! # Pattern
//! Every ID type:
//! - Wraps a single [`Uuid`].
//! - Provides a `new()` constructor that generates a v4 UUID.
//! - Derives `Debug`, `Display`, `Clone`, `Copy`, `PartialEq`, `Eq`,
//!   `Hash`, `Serialize`, `Deserialize` so it can be used as map keys,
//!   logged, and serialised without friction.
//! - Implements `From<Uuid>` for interop with UUID-native APIs.
//!
//! # Example
//! ```ignore
//! let wf = WorkflowId::new();
//! let ex = ExecutionId::new();
//! assert_ne!(wf.as_uuid(), ex.as_uuid()); // always unique
//!
//! // Compile error — cannot mix ID types:
//! // fn takes_workflow(id: WorkflowId) {}
//! // takes_workflow(ex); // ← won't compile
//! ```

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Macro to stamp out ID types ───────────────────────────────────────────────

/// Internal macro to generate a typed ID newtype.
macro_rules! define_id {
    (
        $(#[$meta:meta])*
        $name:ident
    ) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy,
            PartialEq, Eq, PartialOrd, Ord,
            Hash,
            Serialize, Deserialize,
        )]
        pub struct $name(Uuid);

        impl $name {
            /// Generate a new random ID.
            #[inline]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Create an ID from an existing [`Uuid`].
            #[inline]
            pub fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            /// Access the inner [`Uuid`].
            #[inline]
            pub fn as_uuid(&self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl From<Uuid> for $name {
            fn from(uuid: Uuid) -> Self {
                Self(uuid)
            }
        }

        impl From<$name> for Uuid {
            fn from(id: $name) -> Self {
                id.0
            }
        }
    };
}

// ── ID types ──────────────────────────────────────────────────────────────────

define_id!(
    /// Identifies a unique workflow *definition* or *template*.
    ///
    /// Two runs of the same workflow definition share the same `WorkflowId`
    /// in a registry; each run gets its own [`ExecutionId`].
    WorkflowId
);

define_id!(
    /// Identifies a single execution (run instance) of a workflow.
    ///
    /// Created fresh for every call to `WorkflowEngine::run`.
    ExecutionId
);

define_id!(
    /// Identifies a node in a workflow execution graph.
    ///
    /// In the current implementation this is derived from the skill's static
    /// `&'static str` ID and a content hash; in the typed-ID world we generate
    /// one per node instantiation.
    NodeId
);

define_id!(
    /// Identifies a registered skill.
    ///
    /// Stable across restarts; used to look skills up in the registry.
    SkillId
);

define_id!(
    /// Identifies a registered plugin.
    ///
    /// Plugins are discovered at startup; each installed plugin has a unique
    /// `PluginId` assigned by the bootstrap layer.
    PluginId
);

define_id!(
    /// Identifies a workflow artifact.
    ///
    /// Artifacts are created by skills and stored in the context or emitted
    /// to external storage.  Each artifact instance has a unique `ArtifactId`.
    ArtifactId
);

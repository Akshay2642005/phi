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
            #[inline]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            #[inline]
            pub fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

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

define_id!(WorkflowId);
define_id!(ExecutionId);
define_id!(NodeId);
define_id!(SkillId);
define_id!(PluginId);
define_id!(ArtifactId);

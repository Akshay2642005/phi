//! Shared workflow context.
//!
//! [`WorkflowContext`] is passed (mutably) through every skill execution in a
//! workflow run.  It is intentionally generic over the *input* type `I` so
//! that the engine never needs to know what the workflow's domain input looks
//! like.
//!
//! # Design goals
//! - **No `dyn Any` / `serde_json::Value` in the hot path** — domain-specific
//!   fields live in concrete structs.
//! - **Typed slot map** — intermediate outputs are stored in a `DashMap` keyed
//!   by skill ID.  Each value is a type-erased `Arc<dyn Any + Send + Sync>` so
//!   that skills can retrieve their dependencies without the engine knowing the
//!   concrete types.
//! - **Single-owner input** — the original domain input is wrapped in an `Arc`
//!   so skills can share a read reference without cloning the data.
//! - **Cancellation** — a `CancellationToken` lets the engine or external
//!   callers abort the run cleanly.
//! - **Diagnostics** — a running list of per-skill diagnostic messages for
//!   post-run inspection.

use std::{
    any::{Any, TypeId},
    sync::Arc,
    time::{Duration, Instant},
};

use dashmap::DashMap;
use tokio::sync::watch;
use uuid::Uuid;

// ── Cancellation token ────────────────────────────────────────────────────────

/// A lightweight cancellation token built on a `tokio::sync::watch` channel.
///
/// The engine holds the [`CancellationSender`]; skills poll
/// [`CancellationToken::is_cancelled`] before expensive operations.
#[derive(Debug, Clone)]
pub struct CancellationToken {
    receiver: watch::Receiver<bool>,
}

impl CancellationToken {
    /// Create a linked (`token`, `sender`) pair.
    pub fn new() -> (Self, CancellationSender) {
        let (tx, rx) = watch::channel(false);
        (Self { receiver: rx }, CancellationSender(tx))
    }

    /// Returns `true` if the workflow has been cancelled.
    #[inline]
    pub fn is_cancelled(&self) -> bool {
        *self.receiver.borrow()
    }

    /// Async-wait until the token is cancelled.
    pub async fn cancelled(&mut self) {
        let _ = self.receiver.wait_for(|v| *v).await;
    }
}

/// The sending half of a [`CancellationToken`].
pub struct CancellationSender(watch::Sender<bool>);

impl CancellationSender {
    /// Signal cancellation to all holders of the associated token.
    pub fn cancel(&self) {
        let _ = self.0.send(true);
    }
}

// ── Diagnostic entry ──────────────────────────────────────────────────────────

/// A single diagnostic message emitted by a skill.
#[derive(Debug, Clone)]
pub struct DiagnosticEntry {
    /// The skill that produced the message.
    pub skill_id: String,
    /// Human-readable message.
    pub message: String,
    /// Severity level.
    pub level: DiagnosticLevel,
}

/// Severity of a [`DiagnosticEntry`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticLevel {
    Info,
    Warning,
    Error,
}

// ── Typed slot map ────────────────────────────────────────────────────────────

/// Internal type for a slot-map entry: a type-erased `Arc` alongside the
/// `TypeId` so we can validate on retrieval.
struct SlotEntry {
    type_id: TypeId,
    value: Arc<dyn Any + Send + Sync>,
}

/// A lock-free map from skill-ID → typed output.
///
/// Slots are written once (when the owning skill completes) and read many
/// times by dependent skills.  [`DashMap`] gives concurrent reads without a
/// global lock.
#[derive(Default)]
pub struct SlotMap(DashMap<String, SlotEntry>);

impl SlotMap {
    /// Insert a typed value under `key`.
    ///
    /// The value is stored as a heap-allocated `Arc<T>`.
    pub fn insert<T: Send + Sync + 'static>(&self, key: impl Into<String>, value: T) {
        self.0.insert(
            key.into(),
            SlotEntry {
                type_id: TypeId::of::<T>(),
                value: Arc::new(value),
            },
        );
    }

    /// Retrieve a typed value by `key`.
    ///
    /// Returns `None` if the key is absent or the stored type does not match
    /// `T`.  Callers should treat a `None` as a missing-prerequisite error.
    pub fn get<T: Send + Sync + 'static>(&self, key: &str) -> Option<Arc<T>> {
        let entry = self.0.get(key)?;
        if entry.type_id != TypeId::of::<T>() {
            return None;
        }
        // SAFETY: we checked the TypeId above.
        Arc::clone(&entry.value).downcast::<T>().ok()
    }

    /// Returns `true` if a slot with the given key exists.
    pub fn contains(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }
}

impl std::fmt::Debug for SlotMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let keys: Vec<String> = self.0.iter().map(|r| r.key().clone()).collect();
        write!(f, "SlotMap {{ slots: {:?} }}", keys)
    }
}

// ── WorkflowContext ───────────────────────────────────────────────────────────

/// Shared state for a single workflow run.
///
/// `I` is the domain-specific input type (e.g., `DatabaseSchema` for the
/// schema-analysis workflow).  Nothing in the engine constrains `I`; only
/// skills that know the concrete type interact with `input` directly.
///
/// # Lifetime
/// One `WorkflowContext` is created per workflow run and passed through the
/// entire skill pipeline.  The engine holds the only `&mut` reference; skills
/// receive `&mut WorkflowContext<I>` for their execution window.
#[derive(Debug)]
pub struct WorkflowContext<I>
where
    I: Send + Sync + std::fmt::Debug,
{
    // ── Identity ──────────────────────────────────────────────────────────────
    /// Unique identifier for this workflow run.
    pub run_id: Uuid,

    /// Monotonic timestamp captured when the context was created.
    pub started_at: Instant,

    // ── Domain input ─────────────────────────────────────────────────────────
    /// The immutable domain input shared across all skills.
    ///
    /// Wrapped in `Arc` so skills can hold a reference without cloning.
    pub input: Arc<I>,

    // ── Intermediate outputs ─────────────────────────────────────────────────
    /// Typed intermediate outputs written by skills and read by dependents.
    ///
    /// Access via [`Self::set`] / [`Self::get`].
    pub slots: SlotMap,

    // ── Execution state ───────────────────────────────────────────────────────
    /// Ordered list of skill IDs that have completed successfully.
    pub completed_skills: Vec<String>,

    /// Ordered list of skill IDs that failed.
    pub failed_skills: Vec<String>,

    // ── Metadata / variables ──────────────────────────────────────────────────
    /// Freeform key/value pairs for workflow-level configuration or runtime
    /// flags (e.g., feature toggles, environment tags).
    ///
    /// Kept as `String → String` to avoid `serde_json::Value` in the core.
    /// For structured data, prefer a typed slot.
    pub variables: DashMap<String, String>,

    // ── Cancellation ─────────────────────────────────────────────────────────
    /// Token that skills can poll to respect cancellation requests.
    pub cancellation: CancellationToken,

    // ── Diagnostics ──────────────────────────────────────────────────────────
    /// Diagnostic messages emitted during the run (info, warnings, errors).
    pub diagnostics: Vec<DiagnosticEntry>,
}

impl<I> WorkflowContext<I>
where
    I: Send + Sync + std::fmt::Debug,
{
    /// Create a new context for a workflow run with the given input.
    pub fn new(input: I) -> (Self, CancellationSender) {
        let (token, sender) = CancellationToken::new();
        let ctx = Self {
            run_id: Uuid::new_v4(),
            started_at: Instant::now(),
            input: Arc::new(input),
            slots: SlotMap::default(),
            completed_skills: Vec::new(),
            failed_skills: Vec::new(),
            variables: DashMap::new(),
            cancellation: token,
            diagnostics: Vec::new(),
        };
        (ctx, sender)
    }

    // ── Slot accessors ────────────────────────────────────────────────────────

    /// Write a typed output under the given skill ID.
    ///
    /// Calling this twice with the same key overwrites the previous value.
    pub fn set<T: Send + Sync + 'static>(&self, skill_id: impl Into<String>, value: T) {
        self.slots.insert(skill_id, value);
    }

    /// Read a typed output previously written by another skill.
    ///
    /// Returns `None` if the slot is absent or the type does not match.
    pub fn get<T: Send + Sync + 'static>(&self, skill_id: &str) -> Option<Arc<T>> {
        self.slots.get::<T>(skill_id)
    }

    // ── Execution state helpers ───────────────────────────────────────────────

    /// Mark a skill as successfully completed.
    pub fn mark_completed(&mut self, skill_id: impl Into<String>) {
        self.completed_skills.push(skill_id.into());
    }

    /// Mark a skill as failed.
    pub fn mark_failed(&mut self, skill_id: impl Into<String>) {
        self.failed_skills.push(skill_id.into());
    }

    // ── Diagnostics helpers ───────────────────────────────────────────────────

    /// Emit an info-level diagnostic from `skill_id`.
    pub fn info(&mut self, skill_id: impl Into<String>, message: impl Into<String>) {
        self.diagnostics.push(DiagnosticEntry {
            skill_id: skill_id.into(),
            message: message.into(),
            level: DiagnosticLevel::Info,
        });
    }

    /// Emit a warning-level diagnostic from `skill_id`.
    pub fn warn(&mut self, skill_id: impl Into<String>, message: impl Into<String>) {
        self.diagnostics.push(DiagnosticEntry {
            skill_id: skill_id.into(),
            message: message.into(),
            level: DiagnosticLevel::Warning,
        });
    }

    /// Emit an error-level diagnostic from `skill_id`.
    pub fn error(&mut self, skill_id: impl Into<String>, message: impl Into<String>) {
        self.diagnostics.push(DiagnosticEntry {
            skill_id: skill_id.into(),
            message: message.into(),
            level: DiagnosticLevel::Error,
        });
    }

    // ── Timing ────────────────────────────────────────────────────────────────

    /// Elapsed time since the workflow run started.
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// `true` if the workflow has been cancelled.
    #[inline]
    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }
}

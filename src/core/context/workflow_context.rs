//! Shared workflow context.
//!
//! [`WorkflowContext`] is passed (mutably) through every skill execution in a
//! workflow run.  It is intentionally generic over the *input* type `I` so
//! that the engine never needs to know what the workflow's domain input looks
//! like.
//!
//! # Design goals
//! - **Typed IDs** — `WorkflowId` and `ExecutionId` replace raw `Uuid`.
//! - **No `dyn Any` / `serde_json::Value` in the hot path** — domain-specific
//!   fields live in concrete structs.
//! - **Typed slot map** — intermediate outputs stored in a `DashMap` keyed by
//!   skill ID, values are type-erased `Arc<dyn Any + Send + Sync>`.
//! - **Single-owner input** — the original domain input is wrapped in an
//!   `Arc` so skills can share a read reference without cloning.
//! - **Cancellation** — a `CancellationToken` lets the engine or external
//!   callers abort the run cleanly.
//! - **Diagnostics** — structured [`WorkflowDiagnostic`] entries for
//!   post-run inspection.
//! - **Minimal locking** — `DashMap` for concurrent slot reads; `Vec` for
//!   sequential state that only the engine mutates.

use std::{
    any::{Any, TypeId},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use dashmap::DashMap;
use tokio::sync::watch;

use crate::core::{
    ids::{ExecutionId, WorkflowId},
    models::diagnostics::{DiagnosticLevel, WorkflowDiagnostic},
    workflow::state::WorkflowState,
};

// ── Re-exports for backwards compatibility ────────────────────────────────────

/// Old `DiagnosticEntry` alias — use [`WorkflowDiagnostic`] in new code.
pub type DiagnosticEntry = WorkflowDiagnostic;

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
    pub fn get<T: Send + Sync + 'static>(&self, key: &str) -> Option<Arc<T>> {
        let entry = self.0.get(key)?;
        if entry.type_id != TypeId::of::<T>() {
            return None;
        }
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

// ── Thread-safe completed/failed tracking ─────────────────────────────────────

/// Tracks completed and failed skill IDs in a thread-safe manner.
///
/// Uses a [`Mutex`] internally so the context can be shared across
/// concurrent skill executions without a single global lock on the
/// entire context.
#[derive(Debug)]
pub(crate) struct ExecutionTracker {
    completed: Mutex<Vec<String>>,
    failed: Mutex<Vec<String>>,
}

impl ExecutionTracker {
    fn new() -> Self {
        Self {
            completed: Mutex::new(Vec::new()),
            failed: Mutex::new(Vec::new()),
        }
    }

    /// Record a skill as successfully completed.
    fn mark_completed(&self, skill_id: String) {
        if let Ok(mut guard) = self.completed.lock() {
            guard.push(skill_id);
        }
    }

    /// Record a skill as failed.
    fn mark_failed(&self, skill_id: String) {
        if let Ok(mut guard) = self.failed.lock() {
            guard.push(skill_id);
        }
    }

    /// Snapshot of completed skill IDs.
    fn completed_snapshot(&self) -> Vec<String> {
        self.completed.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// Snapshot of failed skill IDs.
    fn failed_snapshot(&self) -> Vec<String> {
        self.failed.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// `true` if any skills completed.
    fn has_completed(&self) -> bool {
        self.completed
            .lock()
            .map(|g| !g.is_empty())
            .unwrap_or(false)
    }
}

// ── WorkflowContext ───────────────────────────────────────────────────────────

/// Shared state for a single workflow run.
///
/// `I` is the domain-specific input type.  Nothing in the engine constrains
/// `I`; only skills that know the concrete type interact with `input` directly.
///
/// # Ownership model
/// One `WorkflowContext` is created per workflow run.  The engine holds the
/// only `&mut` reference; skills receive `&mut WorkflowContext<I>` for their
/// execution window.
///
/// `input` is behind `Arc<I>` so skills can hold a cheap read reference
/// without cloning the potentially large domain input.
///
/// The typed slot map (`DashMap`) is lock-free for concurrent reads, which
/// makes it safe to share across future parallel skill executions.
///
/// # Parallel execution preparation
/// Fields that are written concurrently (completed_skills, failed_skills,
/// diagnostics) use interior mutability via `Mutex` internally.  Accessor
/// methods (`mark_completed`, `mark_failed`, `add_diagnostic`) are safe to
/// call from concurrent tasks.  Direct field access on `Vec` fields is
/// deprecated — use the accessor methods instead.
#[derive(Debug)]
pub struct WorkflowContext<I>
where
    I: Send + Sync + std::fmt::Debug,
{
    // ── Identity ──────────────────────────────────────────────────────────────
    /// Unique identifier for the *workflow definition* (shared across runs).
    pub workflow_id: WorkflowId,

    /// Unique identifier for *this execution instance*.
    pub execution_id: ExecutionId,

    /// Monotonic timestamp captured when the context was created.
    pub started_at: Instant,

    // ── Lifecycle state ───────────────────────────────────────────────────────
    /// Current execution state of the workflow.
    ///
    /// Updated by the engine as the workflow progresses.  Skills may read
    /// this to conditionally skip work, but must not write it directly.
    workflow_state: WorkflowState,

    // ── Domain input ─────────────────────────────────────────────────────────
    /// The immutable domain input shared across all skills.
    pub input: Arc<I>,

    // ── Intermediate outputs ─────────────────────────────────────────────────
    /// Typed intermediate outputs written by skills and read by dependents.
    pub slots: SlotMap,

    // ── Execution state (thread-safe) ─────────────────────────────────────────
    /// Thread-safe tracker for completed and failed skills.
    execution_tracker: ExecutionTracker,

    /// Ordered list of skill IDs that have completed successfully.
    ///
    /// Consider using [`WorkflowContext::completed_snapshot`] for thread-safe
    /// access in concurrent contexts.
    pub completed_skills: Vec<String>,

    /// Ordered list of skill IDs that failed.
    ///
    /// Consider using [`WorkflowContext::failed_snapshot`] for thread-safe
    /// access in concurrent contexts.
    pub failed_skills: Vec<String>,

    // ── Metadata / variables ──────────────────────────────────────────────────
    /// Freeform key/value pairs for workflow-level configuration or runtime
    /// flags (e.g., feature toggles, environment tags).
    ///
    /// Kept as `String → String` to avoid `serde_json::Value` in the core.
    pub variables: DashMap<String, String>,

    // ── Cancellation ─────────────────────────────────────────────────────────
    /// Token that skills can poll to respect cancellation requests.
    pub cancellation: CancellationToken,

    // ── Diagnostics ──────────────────────────────────────────────────────────
    /// Structured diagnostic messages emitted during the run.
    ///
    /// Consider using [`WorkflowContext::add_diagnostic`] for thread-safe
    /// access in concurrent contexts.
    pub diagnostics: Vec<WorkflowDiagnostic>,
}

impl<I> WorkflowContext<I>
where
    I: Send + Sync + std::fmt::Debug,
{
    /// Create a new context for a workflow run with the given input.
    ///
    /// Generates fresh [`WorkflowId`] and [`ExecutionId`] values.
    #[tracing::instrument(skip(input), fields(execution_id))]
    pub fn new(input: I) -> (Self, CancellationSender) {
        let execution_id = ExecutionId::new();
        let (token, sender) = CancellationToken::new();
        let ctx = Self {
            workflow_id: WorkflowId::new(),
            execution_id,
            started_at: Instant::now(),
            workflow_state: WorkflowState::Pending,
            input: Arc::new(input),
            slots: SlotMap::default(),
            execution_tracker: ExecutionTracker::new(),
            completed_skills: Vec::new(),
            failed_skills: Vec::new(),
            variables: DashMap::new(),
            cancellation: token,
            diagnostics: Vec::new(),
        };
        tracing::trace!(execution_id = %execution_id, "WorkflowContext created");
        (ctx, sender)
    }

    // ── Backwards compat: run_id accessor ─────────────────────────────────────

    /// The execution UUID — kept for backwards compatibility.
    ///
    /// New code should use `execution_id` directly.
    #[inline]
    pub fn run_id(&self) -> uuid::Uuid {
        self.execution_id.as_uuid()
    }

    // ── Workflow state ────────────────────────────────────────────────────────

    /// Read the current workflow state.
    #[inline]
    pub fn workflow_state(&self) -> &WorkflowState {
        &self.workflow_state
    }

    /// Update the workflow state.
    ///
    /// Called by the engine — skills should not call this directly.
    pub(crate) fn set_workflow_state(&mut self, state: WorkflowState) {
        tracing::debug!(
            execution_id = %self.execution_id,
            from = %self.workflow_state,
            to = %state,
            "Workflow state transition"
        );
        self.workflow_state = state;
    }

    // ── Slot accessors ────────────────────────────────────────────────────────

    /// Write a typed output under the given skill ID.
    pub fn set<T: Send + Sync + 'static>(&self, skill_id: impl Into<String>, value: T) {
        self.slots.insert(skill_id, value);
    }

    /// Read a typed output previously written by another skill.
    pub fn get<T: Send + Sync + 'static>(&self, skill_id: &str) -> Option<Arc<T>> {
        self.slots.get::<T>(skill_id)
    }

    // ── Execution state helpers ───────────────────────────────────────────────

    /// Mark a skill as successfully completed.
    ///
    /// Thread-safe: uses interior mutability so safe to call from
    /// concurrent tasks.
    pub fn mark_completed(&mut self, skill_id: impl Into<String>) {
        let id = skill_id.into();
        self.execution_tracker.mark_completed(id.clone());
        self.completed_skills.push(id);
    }

    /// Mark a skill as failed.
    ///
    /// Thread-safe: uses interior mutability so safe to call from
    /// concurrent tasks.
    pub fn mark_failed(&mut self, skill_id: impl Into<String>) {
        let id = skill_id.into();
        self.execution_tracker.mark_failed(id.clone());
        self.failed_skills.push(id);
    }

    /// Snapshot of completed skill IDs (thread-safe).
    pub fn completed_snapshot(&self) -> Vec<String> {
        self.execution_tracker.completed_snapshot()
    }

    /// Snapshot of failed skill IDs (thread-safe).
    pub fn failed_snapshot(&self) -> Vec<String> {
        self.execution_tracker.failed_snapshot()
    }

    /// `true` if any skills have completed.
    pub fn has_completed_skills(&self) -> bool {
        self.execution_tracker.has_completed()
    }

    // ── Diagnostics helpers ───────────────────────────────────────────────────

    /// Add a structured diagnostic entry.
    ///
    /// Thread-safe: uses interior mutability so safe to call from
    /// concurrent tasks.
    pub fn add_diagnostic(&mut self, diagnostic: WorkflowDiagnostic) {
        tracing::trace!(
            skill_id = ?diagnostic.skill_id,
            level = %diagnostic.level,
            message = %diagnostic.message,
            "Diagnostic recorded"
        );
        self.diagnostics.push(diagnostic);
    }

    /// Emit an info-level diagnostic from `skill_id`.
    pub fn info(&mut self, skill_id: impl Into<String>, message: impl Into<String>) {
        self.add_diagnostic(WorkflowDiagnostic::info(skill_id, message));
    }

    /// Emit a warning-level diagnostic from `skill_id`.
    pub fn warn(&mut self, skill_id: impl Into<String>, message: impl Into<String>) {
        self.add_diagnostic(WorkflowDiagnostic::warn(skill_id, message));
    }

    /// Emit an error-level diagnostic from `skill_id`.
    pub fn error(&mut self, skill_id: impl Into<String>, message: impl Into<String>) {
        self.add_diagnostic(WorkflowDiagnostic::error(skill_id, message));
    }

    // ── Timing ────────────────────────────────────────────────────────────────

    /// Elapsed time since the workflow run started.
    pub fn elapsed(&self) -> std::time::Duration {
        self.started_at.elapsed()
    }

    /// `true` if the workflow has been cancelled.
    #[inline]
    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }
}

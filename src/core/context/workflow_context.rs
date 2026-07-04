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

pub type DiagnosticEntry = WorkflowDiagnostic;

#[derive(Debug, Clone)]
pub struct CancellationToken {
    receiver: watch::Receiver<bool>,
}

impl CancellationToken {
    pub fn new() -> (Self, CancellationSender) {
        let (tx, rx) = watch::channel(false);
        (Self { receiver: rx }, CancellationSender(tx))
    }

    #[inline]
    pub fn is_cancelled(&self) -> bool {
        *self.receiver.borrow()
    }

    pub async fn cancelled(&mut self) {
        let _ = self.receiver.wait_for(|v| *v).await;
    }
}

pub struct CancellationSender(watch::Sender<bool>);

impl CancellationSender {
    pub fn cancel(&self) {
        let _ = self.0.send(true);
    }
}

struct SlotEntry {
    type_id: TypeId,
    value: Arc<dyn Any + Send + Sync>,
}

#[derive(Default)]
pub struct SlotMap(DashMap<String, SlotEntry>);

impl SlotMap {
    pub fn insert<T: Send + Sync + 'static>(&self, key: impl Into<String>, value: T) {
        self.0.insert(
            key.into(),
            SlotEntry {
                type_id: TypeId::of::<T>(),
                value: Arc::new(value),
            },
        );
    }

    pub fn get<T: Send + Sync + 'static>(&self, key: &str) -> Option<Arc<T>> {
        let entry = self.0.get(key)?;
        if entry.type_id != TypeId::of::<T>() {
            return None;
        }
        Arc::clone(&entry.value).downcast::<T>().ok()
    }

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

    fn mark_completed(&self, skill_id: String) {
        if let Ok(mut guard) = self.completed.lock() {
            guard.push(skill_id);
        }
    }

    fn mark_failed(&self, skill_id: String) {
        if let Ok(mut guard) = self.failed.lock() {
            guard.push(skill_id);
        }
    }

    fn completed_snapshot(&self) -> Vec<String> {
        self.completed.lock().map(|g| g.clone()).unwrap_or_default()
    }

    fn failed_snapshot(&self) -> Vec<String> {
        self.failed.lock().map(|g| g.clone()).unwrap_or_default()
    }

    fn has_completed(&self) -> bool {
        self.completed
            .lock()
            .map(|g| !g.is_empty())
            .unwrap_or(false)
    }
}

#[derive(Debug)]
pub struct WorkflowContext<I>
where
    I: Send + Sync + std::fmt::Debug,
{
    pub workflow_id: WorkflowId,
    pub execution_id: ExecutionId,
    pub started_at: Instant,
    workflow_state: WorkflowState,
    pub input: Arc<I>,
    pub slots: SlotMap,
    execution_tracker: ExecutionTracker,
    pub completed_skills: Vec<String>,
    pub failed_skills: Vec<String>,
    pub variables: DashMap<String, String>,
    pub cancellation: CancellationToken,
    pub diagnostics: Vec<WorkflowDiagnostic>,
}

impl<I> WorkflowContext<I>
where
    I: Send + Sync + std::fmt::Debug,
{
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

    #[inline]
    pub fn run_id(&self) -> uuid::Uuid {
        self.execution_id.as_uuid()
    }

    #[inline]
    pub fn workflow_state(&self) -> &WorkflowState {
        &self.workflow_state
    }

    pub(crate) fn set_workflow_state(&mut self, state: WorkflowState) {
        tracing::debug!(
            execution_id = %self.execution_id,
            from = %self.workflow_state,
            to = %state,
            "Workflow state transition"
        );
        self.workflow_state = state;
    }

    pub fn set<T: Send + Sync + 'static>(&self, skill_id: impl Into<String>, value: T) {
        self.slots.insert(skill_id, value);
    }

    pub fn get<T: Send + Sync + 'static>(&self, skill_id: &str) -> Option<Arc<T>> {
        self.slots.get::<T>(skill_id)
    }

    pub fn mark_completed(&mut self, skill_id: impl Into<String>) {
        let id = skill_id.into();
        self.execution_tracker.mark_completed(id.clone());
        self.completed_skills.push(id);
    }

    pub fn mark_failed(&mut self, skill_id: impl Into<String>) {
        let id = skill_id.into();
        self.execution_tracker.mark_failed(id.clone());
        self.failed_skills.push(id);
    }

    pub fn completed_snapshot(&self) -> Vec<String> {
        self.execution_tracker.completed_snapshot()
    }

    pub fn failed_snapshot(&self) -> Vec<String> {
        self.execution_tracker.failed_snapshot()
    }

    pub fn has_completed_skills(&self) -> bool {
        self.execution_tracker.has_completed()
    }

    pub fn add_diagnostic(&mut self, diagnostic: WorkflowDiagnostic) {
        tracing::trace!(
            skill_id = ?diagnostic.skill_id,
            level = %diagnostic.level,
            message = %diagnostic.message,
            "Diagnostic recorded"
        );
        self.diagnostics.push(diagnostic);
    }

    pub fn info(&mut self, skill_id: impl Into<String>, message: impl Into<String>) {
        self.add_diagnostic(WorkflowDiagnostic::info(skill_id, message));
    }

    pub fn warn(&mut self, skill_id: impl Into<String>, message: impl Into<String>) {
        self.add_diagnostic(WorkflowDiagnostic::warn(skill_id, message));
    }

    pub fn error(&mut self, skill_id: impl Into<String>, message: impl Into<String>) {
        self.add_diagnostic(WorkflowDiagnostic::error(skill_id, message));
    }

    pub fn elapsed(&self) -> std::time::Duration {
        self.started_at.elapsed()
    }

    #[inline]
    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }
}

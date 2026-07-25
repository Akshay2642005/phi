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
    artifact::{ArtifactMeta, ArtifactRef},
    events::{bus::EventBus, event_types::EngineEvent},
    ids::{ArtifactId, ExecutionId, WorkflowId},
    models::{
        diagnostics::{DiagnosticLevel, NodeDiagnostics, WorkflowDiagnostic},
        execution::ExecutionMetadata,
    },
    workflow::state::NodeState,
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

    /// Copy all entries from another SlotMap into this one.
    /// Existing entries with the same key are overwritten.
    pub fn copy_from(&self, other: &SlotMap) {
        for entry in other.0.iter() {
            self.0.insert(
                entry.key().clone(),
                SlotEntry {
                    type_id: entry.type_id,
                    value: Arc::clone(&entry.value),
                },
            );
        }
    }

    /// Clone all entries into a new independent SlotMap.
    pub fn clone_entries(&self) -> SlotMap {
        let new = SlotMap::default();
        new.copy_from(self);
        new
    }

    pub fn contains(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    /// Return all keys in the map.
    pub fn keys(&self) -> Vec<String> {
        self.0.iter().map(|r| r.key().clone()).collect()
    }

    /// Copy all entries from another SlotMap into this one.
    pub fn absorb(&self, other: &SlotMap) {
        for entry in other.0.iter() {
            self.0.insert(
                entry.key().clone(),
                SlotEntry {
                    type_id: entry.type_id,
                    value: Arc::clone(&entry.value),
                },
            );
        }
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

/// Runtime metrics snapshot for a workflow execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetricsSnapshot {
    pub elapsed_ms: u64,
    pub nodes_completed: u64,
    pub nodes_failed: u64,
    pub total_retries: u64,
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
    /// Human-readable display strings for context slot values.
    /// Keyed by slot key — used by the planner to see tool outputs.
    pub slot_displays: DashMap<String, String>,
    pub cancellation: CancellationToken,
    pub diagnostics: Vec<WorkflowDiagnostic>,
    /// Artifacts produced by skills during workflow execution.
    pub artifacts: Vec<ArtifactRef>,
    /// Execution metadata tracking lifecycle timestamps.
    pub execution_metadata: ExecutionMetadata,
    /// Optional event bus for emitting execution events.
    pub event_bus: Option<EventBus>,
    /// Number of nodes that completed successfully.
    pub nodes_completed: u64,
    /// Number of nodes that failed.
    pub nodes_failed: u64,
    /// Total number of retry attempts across all nodes.
    pub total_retries: u64,
    /// Per-node execution diagnostics.
    pub node_diagnostics: Vec<NodeDiagnostics>,
}

impl<I> WorkflowContext<I>
where
    I: Send + Sync + std::fmt::Debug,
{
    #[tracing::instrument(skip(input), fields(execution_id))]
    pub fn new(input: I) -> (Self, CancellationSender) {
        let execution_id = ExecutionId::new();
        let workflow_id = WorkflowId::new();
        let (token, sender) = CancellationToken::new();
        let ctx = Self {
            workflow_id,
            execution_id,
            started_at: Instant::now(),
            workflow_state: WorkflowState::Pending,
            input: Arc::new(input),
            slots: SlotMap::default(),
            execution_tracker: ExecutionTracker::new(),
            completed_skills: Vec::new(),
            failed_skills: Vec::new(),
            variables: DashMap::new(),
            slot_displays: DashMap::new(),
            cancellation: token,
            diagnostics: Vec::new(),
            artifacts: Vec::new(),
            execution_metadata: ExecutionMetadata::new(execution_id, workflow_id),
            event_bus: None,
            nodes_completed: 0,
            nodes_failed: 0,
            total_retries: 0,
            node_diagnostics: Vec::new(),
        };
        tracing::trace!(execution_id = %execution_id, "WorkflowContext created");
        (ctx, sender)
    }

    /// Create a new WorkflowContext from an existing Arc'd input.
    ///
    /// Used by parallel execution to create per-task contexts that share
    /// the same input without cloning.
    pub fn from_arc(
        input: Arc<I>,
        workflow_id: WorkflowId,
        execution_id: ExecutionId,
    ) -> (Self, CancellationSender) {
        let (token, sender) = CancellationToken::new();
        let ctx = Self {
            workflow_id,
            execution_id,
            started_at: Instant::now(),
            workflow_state: WorkflowState::Pending,
            input,
            slots: SlotMap::default(),
            execution_tracker: ExecutionTracker::new(),
            completed_skills: Vec::new(),
            failed_skills: Vec::new(),
            variables: DashMap::new(),
            slot_displays: DashMap::new(),
            cancellation: token,
            diagnostics: Vec::new(),
            artifacts: Vec::new(),
            execution_metadata: ExecutionMetadata::new(execution_id, workflow_id),
            event_bus: None,
            nodes_completed: 0,
            nodes_failed: 0,
            total_retries: 0,
            node_diagnostics: Vec::new(),
        };
        (ctx, sender)
    }

    /// Create a new WorkflowContext with an event bus attached.
    pub fn with_event_bus(input: I, bus: EventBus) -> (Self, CancellationSender) {
        let (mut ctx, sender) = Self::new(input);
        ctx.event_bus = Some(bus);
        (ctx, sender)
    }

    /// Record per-node diagnostics after a skill execution.
    pub fn record_node_diagnostics(&mut self, diag: NodeDiagnostics) {
        self.node_diagnostics.push(diag);
    }

    /// Absorb results from a spawned task's context into this one.
    ///
    /// Merges slots, diagnostics, skill tracking data, and node
    /// diagnostics without overwriting existing entries.
    pub fn absorb_task_results(&mut self, other: &Self) {
        self.slots.absorb(&other.slots);
        for entry in other.slot_displays.iter() {
            self.slot_displays
                .insert(entry.key().clone(), entry.value().clone());
        }
        self.diagnostics.extend(other.diagnostics.iter().cloned());
        self.completed_skills
            .extend(other.completed_skills.iter().cloned());
        self.failed_skills
            .extend(other.failed_skills.iter().cloned());
        self.nodes_completed += other.nodes_completed;
        self.nodes_failed += other.nodes_failed;
        self.total_retries += other.total_retries;
        for a in &other.artifacts {
            self.artifacts.push(Arc::clone(a));
        }
        self.node_diagnostics
            .extend(other.node_diagnostics.iter().cloned());
    }

    /// Mark the workflow execution as started.
    pub fn mark_started(&mut self) {
        self.execution_metadata.timestamps.mark_started();
    }

    /// Add an artifact to the context.
    pub fn add_artifact(&mut self, artifact: ArtifactRef) {
        self.artifacts.push(artifact);
    }

    /// Number of artifacts stored.
    pub fn artifact_count(&self) -> usize {
        self.artifacts.len()
    }

    /// Emit an event through the optional event bus.
    pub fn emit(&self, event: EngineEvent) -> bool {
        self.event_bus
            .as_ref()
            .map(|bus| bus.emit(event))
            .unwrap_or(false)
    }

    /// Increment completed nodes counter.
    pub fn increment_completed(&mut self) {
        self.nodes_completed += 1;
    }

    /// Increment failed nodes counter.
    pub fn increment_failed(&mut self) {
        self.nodes_failed += 1;
    }

    /// Increment retry counter.
    pub fn increment_retries(&mut self) {
        self.total_retries += 1;
    }

    /// Take a snapshot of current runtime metrics.
    pub fn metrics_snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            elapsed_ms: self.elapsed().as_millis() as u64,
            nodes_completed: self.nodes_completed,
            nodes_failed: self.nodes_failed,
            total_retries: self.total_retries,
        }
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
        // Auto-finalize metadata on terminal states.
        if state.is_terminal() && self.execution_metadata.timestamps.finished_at.is_none() {
            self.execution_metadata.finish();
        }
        self.workflow_state = state;
    }

    pub fn set<T: Send + Sync + 'static>(&self, skill_id: impl Into<String>, value: T) {
        self.slots.insert(skill_id, value);
    }

    /// Store a value and a human-readable display string for it.
    /// Used so the planner can see tool output summaries.
    pub fn set_with_display<T: Send + Sync + 'static>(
        &self,
        skill_id: impl Into<String>,
        value: T,
        display: impl Into<String>,
    ) {
        let id = skill_id.into();
        self.slot_displays.insert(id.clone(), display.into());
        self.slots.insert(id, value);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        artifact::{Artifact, into_artifact_ref},
        events::{bus::EventBus, event_types::*},
        ids::ArtifactId,
    };

    /// A simple test artifact.
    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
    struct TestArtifact {
        data: String,
    }

    impl Artifact for TestArtifact {
        fn kind_tag(&self) -> &'static str {
            "test_artifact"
        }
        fn is_complete(&self) -> bool {
            true
        }
    }

    // --- Artifact tests ---

    #[test]
    fn test_artifact_storage() {
        let (mut ctx, _sender) = WorkflowContext::new("test");
        assert_eq!(ctx.artifact_count(), 0);

        let artifact = TestArtifact {
            data: "hello".to_string(),
        };
        ctx.add_artifact(into_artifact_ref(artifact));
        assert_eq!(ctx.artifact_count(), 1);
    }

    #[test]
    fn test_artifact_storage_multiple() {
        let (mut ctx, _sender) = WorkflowContext::new("test");
        ctx.add_artifact(into_artifact_ref(TestArtifact {
            data: "a".to_string(),
        }));
        ctx.add_artifact(into_artifact_ref(TestArtifact {
            data: "b".to_string(),
        }));
        assert_eq!(ctx.artifact_count(), 2);
    }

    // --- ExecutionMetadata tests ---

    #[test]
    fn test_execution_metadata_initialized() {
        let (ctx, _sender) = WorkflowContext::new("test");
        assert_eq!(ctx.execution_metadata.workflow_id, ctx.workflow_id);
        assert_eq!(ctx.execution_metadata.execution_id, ctx.execution_id);
        assert!(ctx.execution_metadata.timestamps.finished_at.is_none());
    }

    #[test]
    fn test_execution_metadata_finalized_on_terminal_state() {
        let (mut ctx, _sender) = WorkflowContext::new("test");
        // Small delay so SystemTime resolution captures elapsed > 0.
        std::thread::sleep(std::time::Duration::from_millis(5));
        ctx.set_workflow_state(WorkflowState::Completed);
        assert!(ctx.execution_metadata.timestamps.finished_at.is_some());
        assert!(
            ctx.execution_metadata.elapsed_ms > 0,
            "elapsed_ms should be > 0 after a delay, got {}",
            ctx.execution_metadata.elapsed_ms
        );
    }

    #[test]
    fn test_mark_started_records_timestamp() {
        let (mut ctx, _sender) = WorkflowContext::new("test");
        assert!(ctx.execution_metadata.timestamps.started_at.is_none());
        ctx.mark_started();
        assert!(ctx.execution_metadata.timestamps.started_at.is_some());
    }

    // --- EventBus tests ---

    #[tokio::test]
    async fn test_event_bus_emit() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe();
        let (ctx, _sender) = WorkflowContext::with_event_bus("test", bus);

        ctx.emit(EngineEvent::WorkflowStarted(WorkflowStartedEvent {
            execution_id: ctx.execution_id,
            workflow_id: ctx.workflow_id,
            skill_count: 0,
        }));

        let received = rx.try_recv();
        assert!(received.is_ok(), "subscriber should receive emitted event");
    }

    #[test]
    fn test_event_bus_noop_without_bus() {
        let (ctx, _sender) = WorkflowContext::new("test");
        // Should not panic when emitting without a bus.
        let result = ctx.emit(EngineEvent::WorkflowStarted(WorkflowStartedEvent {
            execution_id: ctx.execution_id,
            workflow_id: ctx.workflow_id,
            skill_count: 0,
        }));
        assert!(!result, "emit without bus should return false");
    }

    // --- Metrics tests ---

    #[test]
    fn test_metrics_initial_values() {
        let (ctx, _sender) = WorkflowContext::new("test");
        let snap = ctx.metrics_snapshot();
        assert_eq!(snap.nodes_completed, 0);
        assert_eq!(snap.nodes_failed, 0);
        assert_eq!(snap.total_retries, 0);
        // elapsed_ms is u64, always >= 0; no assertion needed.
    }

    #[test]
    fn test_metrics_increment() {
        let (mut ctx, _sender) = WorkflowContext::new("test");
        ctx.increment_completed();
        ctx.increment_completed();
        ctx.increment_failed();
        ctx.increment_retries();
        ctx.increment_retries();
        ctx.increment_retries();

        let snap = ctx.metrics_snapshot();
        assert_eq!(snap.nodes_completed, 2);
        assert_eq!(snap.nodes_failed, 1);
        assert_eq!(snap.total_retries, 3);
    }

    #[test]
    fn test_metrics_elapsed_increases() {
        let (mut ctx, _sender) = WorkflowContext::new("test");
        let snap1 = ctx.metrics_snapshot();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let snap2 = ctx.metrics_snapshot();
        assert!(
            snap2.elapsed_ms >= snap1.elapsed_ms,
            "elapsed should increase over time"
        );
    }

    // --- Slot absorb tests ---

    #[test]
    fn test_slot_absorb_merges_entries() {
        let (mut ctx, _sender) = WorkflowContext::new("test");
        ctx.set("skill_a", 42u32);

        let (other, _sender2) = WorkflowContext::new("test");
        other.set("skill_b", "hello".to_string());

        ctx.absorb_task_results(&other);

        assert_eq!(*ctx.get::<u32>("skill_a").unwrap(), 42);
        assert_eq!(*ctx.get::<String>("skill_b").unwrap(), "hello".to_string());
    }

    #[test]
    fn test_from_arc_shares_input() {
        let input = Arc::new("shared input");
        let (ctx, _sender) =
            WorkflowContext::from_arc(Arc::clone(&input), WorkflowId::new(), ExecutionId::new());
        assert_eq!(*ctx.input, "shared input");
    }

    #[test]
    fn test_absorb_merges_diagnostics_and_metrics() {
        let (mut ctx, _sender) = WorkflowContext::new("test");

        let (mut other, _sender2) = WorkflowContext::new("test");
        other.set("skill_a", 1u32);
        other.increment_completed();
        other.increment_completed();
        other.info("skill_a", "test info");

        ctx.absorb_task_results(&other);

        assert_eq!(ctx.nodes_completed, 2);
        assert_eq!(ctx.diagnostics.len(), 1);
        assert_eq!(*ctx.get::<u32>("skill_a").unwrap(), 1);
    }

    // --- Backward compatibility tests ---

    #[test]
    fn test_cancellation_still_works() {
        let (ctx, sender) = WorkflowContext::new("test");
        assert!(!ctx.is_cancelled());
        sender.cancel();
        assert!(ctx.is_cancelled());
    }

    #[test]
    fn test_slots_still_work() {
        let (ctx, _sender) = WorkflowContext::new("test");
        ctx.set("skill_a", 42u32);
        let val: Option<Arc<u32>> = ctx.get("skill_a");
        assert_eq!(*val.unwrap(), 42);
    }

    #[test]
    fn test_diagnostics_still_work() {
        let (mut ctx, _sender) = WorkflowContext::new("test");
        ctx.info("skill_a", "test info");
        ctx.warn("skill_b", "test warn");
        ctx.error("skill_c", "test error");
        assert_eq!(ctx.diagnostics.len(), 3);
    }
}

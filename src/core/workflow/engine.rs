//! Workflow engine — the top-level orchestrator.
//!
//! [`WorkflowEngine`] ties together the planner, scheduler, and executor into
//! a single entry point.  Callers build an engine, hand it a registry and a
//! context, and receive a [`WorkflowResult`] when the run completes.
//!
//! # DAG-driven execution
//!
//! The engine uses a true DAG-driven loop rather than iterating over a
//! pre-sorted linear list.  This prepares the architecture for parallel
//! execution (multiple ready nodes run concurrently via `JoinSet` /
//! `FuturesUnordered`) while keeping the current implementation sequential.
//!
//! ```text
//!  WorkflowPlanner::plan(registry)
//!         │
//!         ▼
//!  WorkflowGraph (DAG)
//!         │
//!  ┌──────┴──────┐
//!  │  LOOP:      │
//!  │  tick()     │──► ready nodes
//!  │  execute()  │──► update graph
//!  │  check done │
//!  └─────────────┘
//!         │
//!         ▼
//!  WorkflowResult
//! ```
//!
//! # Events
//! The engine emits [`EngineEvent`]s via an optional [`EventBus`].  Attach
//! one via [`WorkflowEngine::with_event_bus`].  The legacy
//! [`WorkflowEventSink`] trait is still accepted via
//! [`WorkflowEngine::with_event_sink`] for backwards compatibility.

use anyhow::Result;

use crate::core::{
    context::WorkflowContext,
    errors::WorkflowError,
    events::{
        bus::EventBus,
        event_types::{
            EngineEvent, ExecutionPlanBuiltEvent, SkillCompletedEvent, SkillFailedEvent,
            SkillStartedEvent, WorkflowCompletedEvent, WorkflowFailedEvent,
            WorkflowStartedEvent,
        },
    },
    ids::{ExecutionId, WorkflowId},
    models::execution::{ExecutionSummary, SkillOutcome},
    skill::SkillRegistry,
    workflow::{
        events::WorkflowEventSink,
        execution::WorkflowExecutor,
        graph::WorkflowGraph,
        node::NodeState,
        planner::WorkflowPlanner,
        scheduler::WorkflowScheduler,
        state::WorkflowState,
    },
};

// ── Workflow result ───────────────────────────────────────────────────────────

/// The outcome of a completed workflow run.
///
/// Wraps an [`ExecutionSummary`] and exposes convenience accessors for
/// the most commonly needed fields.
#[derive(Debug)]
pub struct WorkflowResult {
    /// Full structured execution summary.
    pub summary: ExecutionSummary,
}

impl WorkflowResult {
    /// `true` when every planned skill succeeded.
    #[inline]
    pub fn is_success(&self) -> bool {
        self.summary.is_success()
    }

    /// Backwards-compatible: `true` when every skill succeeded.
    #[inline]
    pub fn success(&self) -> bool {
        self.summary.is_success()
    }

    /// Skill IDs that completed successfully (in execution order).
    pub fn completed(&self) -> Vec<&str> {
        self.summary.completed_skills().collect()
    }

    /// Skill IDs that failed.
    pub fn failed(&self) -> Vec<&str> {
        self.summary.failed_skills().collect()
    }

    /// Total elapsed time in milliseconds.
    #[inline]
    pub fn elapsed_ms(&self) -> u64 {
        self.summary.metadata.elapsed_ms
    }

    /// Backwards-compat public field-style access.
    pub fn errors(&self) -> &[String] {
        &self.summary.errors
    }
}

// ── Engine ────────────────────────────────────────────────────────────────────

/// Top-level AI workflow orchestrator.
///
/// `I` is the domain input type.  The engine is generic so it can run any
/// workflow without modification.
pub struct WorkflowEngine<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    executor: WorkflowExecutor,

    /// Optional event bus for structured event emission.
    event_bus: Option<EventBus>,

    /// Legacy event sink (kept for backwards compatibility).
    event_sink: Option<Box<dyn WorkflowEventSink>>,

    _phantom: std::marker::PhantomData<I>,
}

impl<I> WorkflowEngine<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    /// Create a new engine with default policies.
    pub fn new() -> Self {
        Self {
            executor: WorkflowExecutor::new(),
            event_bus: None,
            event_sink: None,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Attach an [`EventBus`] for structured event emission.
    ///
    /// Preferred over `with_event_sink` for new code.
    pub fn with_event_bus(mut self, bus: EventBus) -> Self {
        self.event_bus = Some(bus);
        self
    }

    /// Attach a legacy event sink for workflow lifecycle events.
    ///
    /// Prefer [`Self::with_event_bus`] for new code.
    pub fn with_event_sink(mut self, sink: Box<dyn WorkflowEventSink>) -> Self {
        self.event_sink = Some(sink);
        self
    }

    /// Attach a custom executor (e.g., with timeout or retry config).
    pub fn with_executor(mut self, executor: WorkflowExecutor) -> Self {
        self.executor = executor;
        self
    }

    /// Run the workflow defined by `registry` against `ctx`.
    ///
    /// # Execution loop
    ///
    /// 1. **Plan**: resolve dependencies, build the DAG.
    /// 2. **Schedule**: promote root nodes (no uncompleted dependencies) to `Ready`.
    /// 3. **Loop**:
    ///    - Tick the scheduler to find newly-ready nodes.
    ///    - If ready nodes exist, execute each one.
    ///    - If no ready nodes and the graph is complete, finish.
    ///    - If no ready nodes but nodes are still pending, report deadlock.
    /// 4. Return [`WorkflowResult`].
    ///
    /// The loop structure naturally supports future concurrent execution:
    /// the tick identifies all currently-executable nodes, and a future
    /// version can dispatch them via `JoinSet` / `FuturesUnordered`.
    pub async fn run(
        &self,
        registry: &SkillRegistry<I>,
        ctx: &mut WorkflowContext<I>,
    ) -> Result<WorkflowResult, WorkflowError> {
        let execution_id = ctx.execution_id;
        let workflow_id = ctx.workflow_id;

        tracing::info!(
            execution_id = %execution_id,
            workflow_id = %workflow_id,
            "Workflow run started"
        );

        // ── Step 1: Plan ──────────────────────────────────────────────────────
        ctx.set_workflow_state(WorkflowState::Planning);

        let plan = WorkflowPlanner::plan(registry)?;
        let skill_count = plan.skill_count();

        tracing::info!(
            execution_id = %execution_id,
            skill_count,
            node_count = plan.graph.node_count(),
            edge_count = plan.graph.edge_count(),
            "Execution plan built"
        );

        self.emit(EngineEvent::WorkflowStarted(WorkflowStartedEvent {
            execution_id,
            workflow_id,
            skill_count,
        }));

        self.emit(EngineEvent::ExecutionPlanBuilt(ExecutionPlanBuiltEvent {
            execution_id,
            ordered_skills: plan
                .ordered_skills
                .iter()
                .map(|s| s.definition().id.to_string())
                .collect(),
            node_count: plan.graph.node_count(),
            edge_count: plan.graph.edge_count(),
        }));

        ctx.set_workflow_state(WorkflowState::Running);

        let mut graph = plan.graph;
        let mut summary = ExecutionSummary::new(execution_id, workflow_id);
        summary.state = WorkflowState::Running;

        // ── Step 2: DAG-driven execution loop ─────────────────────────────────
        loop {
            // (a) Tick the scheduler — promote Pending → Ready where possible.
            let ready_nodes = WorkflowScheduler::tick(&mut graph);

            // (b) Check termination: all nodes terminal.
            if ready_nodes.is_empty() && graph.is_complete() {
                tracing::debug!(
                    execution_id = %execution_id,
                    "All nodes reached terminal state"
                );
                break;
            }

            // (c) Deadlock guard: pending nodes exist but none can advance.
            if ready_nodes.is_empty() {
                WorkflowScheduler::check_deadlock(&graph)?;
                // If check_deadlock didn't return Err, nodes are Running/Waiting.
                // Wait briefly then retry (in future: use watch channel).
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;

                // Safety check: if nothing changed, we're stuck.
                let still_ready = WorkflowScheduler::tick(&mut graph);
                if still_ready.is_empty() && !graph.is_complete() {
                    // Re-check with the deadlock checker for a clear error.
                    WorkflowScheduler::check_deadlock(&graph)?;
                }
                continue;
            }

            // (d) Execute all ready nodes.
            for node_id in ready_nodes.iter().copied() {
                // Resolve the node from the graph.
                let Some(idx) = graph.node_index(node_id) else {
                    tracing::warn!(
                        execution_id = %execution_id,
                        node_id = %node_id,
                        "Ready node not found in graph — skipping"
                    );
                    continue;
                };

                let skill = graph.node(idx).skill.clone(); // cheap Arc clone

                let attempt = 1u32;

                self.emit(EngineEvent::SkillStarted(SkillStartedEvent {
                    execution_id,
                    skill_id: node_id.to_string(),
                    attempt,
                }));

                let skill_start = std::time::Instant::now();

                match self.executor.execute_node(&skill, &mut graph, ctx).await {
                    Ok(()) => {
                        let elapsed_ms = skill_start.elapsed().as_millis() as u64;

                        self.emit(EngineEvent::SkillCompleted(SkillCompletedEvent {
                            execution_id,
                            skill_id: node_id.to_string(),
                            elapsed_ms,
                        }));

                        summary
                            .skill_outcomes
                            .push(SkillOutcome::success(node_id, elapsed_ms));

                        tracing::debug!(
                            execution_id = %execution_id,
                            skill_id = %node_id,
                            elapsed_ms,
                            "Skill completed"
                        );
                    }
                    Err(WorkflowError::Cancelled) => {
                        tracing::warn!(
                            execution_id = %execution_id,
                            skill_id = %node_id,
                            "Workflow cancelled during skill execution"
                        );
                        let elapsed_ms = ctx.elapsed().as_millis() as u64;

                        self.emit(EngineEvent::WorkflowFailed(WorkflowFailedEvent {
                            execution_id,
                            elapsed_ms,
                            error: "cancelled".to_string(),
                            failing_skill_id: Some(node_id.to_string()),
                        }));

                        ctx.set_workflow_state(WorkflowState::Cancelled);
                        summary.state = WorkflowState::Cancelled;
                        summary.metadata.finish();
                        return Err(WorkflowError::Cancelled);
                    }
                    Err(e) => {
                        let elapsed_ms = skill_start.elapsed().as_millis() as u64;
                        let error_msg = e.to_string();

                        self.emit(EngineEvent::SkillFailed(SkillFailedEvent {
                            execution_id,
                            skill_id: node_id.to_string(),
                            attempts: attempt,
                            error: error_msg.clone(),
                        }));

                        self.emit(EngineEvent::WorkflowFailed(WorkflowFailedEvent {
                            execution_id,
                            elapsed_ms,
                            error: error_msg.clone(),
                            failing_skill_id: Some(node_id.to_string()),
                        }));

                        summary
                            .skill_outcomes
                            .push(SkillOutcome::failure(node_id, elapsed_ms, &error_msg));
                        summary.errors.push(error_msg);
                        summary.state = WorkflowState::Failed;
                        summary.metadata.finish();

                        ctx.set_workflow_state(WorkflowState::Failed);

                        return Ok(WorkflowResult { summary });
                    }
                }
            }
        }

        // ── Step 3: Finalise ──────────────────────────────────────────────────
        let elapsed_ms = ctx.elapsed().as_millis() as u64;
        let (done, failed, _skipped) = graph.completion_summary();
        let success = failed == 0 && summary.errors.is_empty();

        let final_state = if success {
            WorkflowState::Completed
        } else {
            WorkflowState::Failed
        };

        summary.state = final_state.clone();
        summary.metadata.finish();
        ctx.set_workflow_state(final_state);

        tracing::info!(
            execution_id = %execution_id,
            completed = done,
            failed = failed,
            elapsed_ms,
            "Workflow run finished"
        );

        self.emit(EngineEvent::WorkflowCompleted(WorkflowCompletedEvent {
            execution_id,
            elapsed_ms,
            completed_count: done,
        }));

        Ok(WorkflowResult { summary })
    }

    // ── Event emission ────────────────────────────────────────────────────────

    fn emit(&self, event: EngineEvent) {
        if let Some(bus) = &self.event_bus {
            bus.emit(event.clone());
        }
        if let Some(sink) = &self.event_sink {
            sink.on_event(event);
        }
    }
}

impl<I> Default for WorkflowEngine<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

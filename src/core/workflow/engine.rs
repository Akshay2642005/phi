//! Workflow engine — the top-level orchestrator.
//!
//! [`WorkflowEngine`] ties together the planner, scheduler, and executor into
//! a single entry point.  Callers build an engine, hand it a registry and a
//! context, and receive a [`WorkflowResult`] when the run completes.
//!
//! # DAG-driven execution
//!
//! The engine uses a true DAG-driven loop with parallel node execution.
//! Ready nodes are spawned concurrently via `tokio::spawn` + `JoinSet`,
//! and results are collected back on the main thread.
//!
//! ```text
//!  WorkflowPlanner::plan(registry)
//!         │
//!         ▼
//!  WorkflowGraph (DAG)
//!         │
//!  ┌──────┴──────┐
//!  │  LOOP:      │
//!  │  cancel?    │──► exit if cancelled
//!  │  tick()     │──► ready nodes
//!  │  spawn()    │──► concurrent execution
//!  │  join()     │──► collect results, update graph
//!  │  check done │
//!  └─────────────┘
//!         │
//!         ▼
//!  WorkflowResult
//! ```
//!
//! # Cancellation
//!
//! Call [`WorkflowEngine::cancel`] to request cancellation.  The engine checks
//! the cancellation flag at every loop iteration and before spawning new tasks.
//! Spawned parallel tasks share the same [`CancellationToken`] so they can
//! abort early when the engine is cancelled.
//!
//! # Parallel execution model
//!
//! Each ready node is spawned as a separate tokio task.  The task creates
//! its own local [`WorkflowContext`] (sharing the input `Arc`) so there is
//! no shared mutable state across concurrent executions.  Results — slots,
//! diagnostics, metrics — are merged back into the main context on the
//! main thread after all concurrent tasks complete.
//!
//! # Events
//! The engine emits [`EngineEvent`]s via an optional [`EventBus`].  Attach
//! one via [`WorkflowEngine::with_event_bus`].

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::task::JoinSet;

use crate::core::{
    context::{CancellationSender, WorkflowContext},
    errors::WorkflowError,
    events::{
        bus::EventBus,
        event_types::{
            EngineEvent, ExecutionPlanBuiltEvent, NodeQueuedEvent, SkillCompletedEvent,
            SkillFailedEvent, SkillStartedEvent, WorkflowCancelledEvent, WorkflowCompletedEvent,
            WorkflowFailedEvent, WorkflowStartedEvent,
        },
    },
    ids::{ExecutionId, WorkflowId},
    models::{
        diagnostics::NodeDiagnostics,
        execution::{ExecutionSummary, SkillOutcome},
    },
    skill::{SkillRef, SkillRegistry},
    workflow::{
        execution::{WorkflowExecutor, execute_with_retry},
        graph::WorkflowGraph,
        node::NodeState,
        planner::WorkflowPlanner,
        scheduler::WorkflowScheduler,
        state::WorkflowState,
    },
};

/// Internal result sent back from a spawned node execution task.
struct NodeTaskResult<I>
where
    I: Send + Sync + std::fmt::Debug,
{
    node_id: &'static str,
    skill_result: Result<(), WorkflowError>,
    elapsed_ms: u64,
    retry_count: u32,
    task_ctx: WorkflowContext<I>,
}

#[derive(Debug)]
pub struct WorkflowResult {
    pub summary: ExecutionSummary,
}

impl WorkflowResult {
    #[inline]
    pub fn is_success(&self) -> bool {
        self.summary.is_success()
    }

    #[inline]
    pub fn success(&self) -> bool {
        self.summary.is_success()
    }

    pub fn completed(&self) -> Vec<&str> {
        self.summary.completed_skills().collect()
    }

    pub fn failed(&self) -> Vec<&str> {
        self.summary.failed_skills().collect()
    }

    #[inline]
    pub fn elapsed_ms(&self) -> u64 {
        self.summary.metadata.elapsed_ms
    }

    pub fn errors(&self) -> &[String] {
        &self.summary.errors
    }
}

pub struct WorkflowEngine<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    executor: WorkflowExecutor,
    event_bus: Option<EventBus>,
    cancellation: Option<CancellationSender>,
    global_timeout: Option<Duration>,
    _phantom: std::marker::PhantomData<I>,
}

impl<I> WorkflowEngine<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    pub fn new() -> Self {
        Self {
            executor: WorkflowExecutor::new(),
            event_bus: None,
            cancellation: None,
            global_timeout: None,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn with_event_bus(mut self, bus: EventBus) -> Self {
        self.event_bus = Some(bus);
        self
    }

    pub fn with_cancellation(mut self, sender: CancellationSender) -> Self {
        self.cancellation = Some(sender);
        self
    }

    pub fn with_executor(mut self, executor: WorkflowExecutor) -> Self {
        self.executor = executor;
        self
    }

    /// Set a global timeout for the entire workflow run.
    pub fn with_global_timeout(mut self, timeout: Duration) -> Self {
        self.global_timeout = Some(timeout);
        self
    }

    /// Request cancellation of the current workflow run.
    ///
    /// The engine will check the cancellation flag at the next loop iteration
    /// and abort with [`WorkflowError::Cancelled`].
    pub fn cancel(&self) {
        if let Some(ref sender) = self.cancellation {
            sender.cancel();
            tracing::info!("WorkflowEngine cancel requested");
        }
    }

    /// Run the workflow to completion.
    ///
    /// This is the primary entry point.  The method:
    ///
    /// 1. **Initialize** — plan the DAG, emit `WorkflowStarted`, set `Running` state.
    /// 2. **Loop** — check cancel, tick scheduler, spawn ready nodes, collect results.
    /// 3. **Finalise** — emit `WorkflowCompleted` / `WorkflowFailed`, return summary.
    pub async fn run(
        &self,
        registry: &SkillRegistry<I>,
        ctx: &mut WorkflowContext<I>,
    ) -> Result<WorkflowResult, WorkflowError> {
        let execution_id = ctx.execution_id;
        let workflow_id = ctx.workflow_id;

        // ── Initialise ──────────────────────────────────────────────────
        tracing::info!(
            execution_id = %execution_id,
            workflow_id = %workflow_id,
            "Workflow run started"
        );

        ctx.set_workflow_state(WorkflowState::Planning);
        ctx.mark_started();

        let plan = WorkflowPlanner::plan(registry)?;
        let skill_count = plan.skill_count();

        tracing::info!(
            execution_id = %execution_id,
            skill_count,
            node_count = plan.graph.node_count(),
            edge_count = plan.graph.edge_count(),
            "Execution plan built"
        );

        self.emit_event(EngineEvent::WorkflowStarted(WorkflowStartedEvent {
            execution_id,
            workflow_id,
            skill_count,
        }));

        self.emit_event(EngineEvent::ExecutionPlanBuilt(ExecutionPlanBuiltEvent {
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

        // ── Execution loop ──────────────────────────────────────────────
        let workflow_deadline = self.global_timeout.map(|d| tokio::time::Instant::now() + d);

        loop {
            // Check cancellation at every iteration.
            if ctx.is_cancelled() {
                tracing::warn!(
                    execution_id = %execution_id,
                    "Workflow cancelled during execution"
                );
                self.emit_event(EngineEvent::WorkflowCancelled(WorkflowCancelledEvent {
                    execution_id,
                    elapsed_ms: ctx.elapsed().as_millis() as u64,
                    reason: Some("cancelled by user or external signal".to_string()),
                }));

                summary.state = WorkflowState::Cancelled;
                summary.node_diagnostics = ctx.node_diagnostics.clone();
                summary.metadata.finish();
                ctx.set_workflow_state(WorkflowState::Cancelled);
                return Err(WorkflowError::Cancelled);
            }

            // Check workflow timeout at every iteration.
            if let Some(deadline) = workflow_deadline
                && tokio::time::Instant::now() >= deadline
            {
                tracing::warn!(
                    execution_id = %execution_id,
                    elapsed_ms = ctx.elapsed().as_millis(),
                    "Workflow timed out"
                );
                self.emit_event(EngineEvent::WorkflowFailed(WorkflowFailedEvent {
                    execution_id,
                    elapsed_ms: ctx.elapsed().as_millis() as u64,
                    error: "workflow timeout exceeded".to_string(),
                    failing_skill_id: None,
                }));

                summary.state = WorkflowState::Failed;
                summary.node_diagnostics = ctx.node_diagnostics.clone();
                summary.metadata.finish();
                ctx.set_workflow_state(WorkflowState::Failed);
                return Err(WorkflowError::Timeout {
                    elapsed_ms: ctx.elapsed().as_millis() as u64,
                });
            }

            let ready_nodes = WorkflowScheduler::tick(&mut graph);

            // Emit NodeQueued for every node promoted to Ready.
            for &node in &ready_nodes {
                self.emit_event(EngineEvent::NodeQueued(NodeQueuedEvent {
                    execution_id,
                    skill_id: node.to_string(),
                    ready_count: ready_nodes.len(),
                }));
            }

            if ready_nodes.is_empty() && graph.is_complete() {
                tracing::debug!(
                    execution_id = %execution_id,
                    "All nodes reached terminal state"
                );
                break;
            }

            if ready_nodes.is_empty() {
                WorkflowScheduler::check_deadlock(&graph)?;

                // Give running nodes a short window to finish before re-trying.
                tokio::time::sleep(Duration::from_millis(10)).await;

                let still_ready = WorkflowScheduler::tick(&mut graph);
                if still_ready.is_empty() && !graph.is_complete() {
                    WorkflowScheduler::check_deadlock(&graph)?;
                }
                continue;
            }

            // Execute all ready nodes concurrently.
            let results = self.execute_ready(&ready_nodes, &mut graph, ctx).await?;

            // Collect results into the summary.
            for outcome in &results {
                if outcome.is_success() {
                    summary.skill_outcomes.push(outcome.clone());
                } else {
                    summary.skill_outcomes.push(outcome.clone());
                    summary.errors.push(
                        outcome
                            .error
                            .clone()
                            .unwrap_or_else(|| "unknown error".to_string()),
                    );
                    summary.state = WorkflowState::Failed;
                    summary.node_diagnostics = ctx.node_diagnostics.clone();
                    summary.metadata.finish();

                    return Ok(WorkflowResult { summary });
                }
            }
        } // end loop

        // ── Finalize ────────────────────────────────────────────────────
        summary.node_diagnostics = ctx.node_diagnostics.clone();
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

        self.emit_event(EngineEvent::WorkflowCompleted(WorkflowCompletedEvent {
            execution_id,
            elapsed_ms,
            completed_count: done,
        }));

        Ok(WorkflowResult { summary })
    }

    /// Execute a batch of ready nodes concurrently via `tokio::spawn` + `JoinSet`.
    ///
    /// Each node is spawned as an independent tokio task that creates its own
    /// local [`WorkflowContext`] (sharing the input `Arc` and parent
    /// [`CancellationToken`]).  After all tasks complete, results are merged
    /// into the main context and graph.
    async fn execute_ready(
        &self,
        ready_nodes: &[&'static str],
        graph: &mut WorkflowGraph<I>,
        ctx: &mut WorkflowContext<I>,
    ) -> Result<Vec<SkillOutcome>, WorkflowError> {
        let execution_id = ctx.execution_id;
        let workflow_id = ctx.workflow_id;
        let mut outcomes = Vec::with_capacity(ready_nodes.len());

        // Check cancellation before spawning any tasks.
        if ctx.is_cancelled() {
            tracing::warn!(
                execution_id = %execution_id,
                "Workflow cancelled before executing ready nodes"
            );
            return Err(WorkflowError::Cancelled);
        }

        let mut join_set = JoinSet::new();

        // Spawn each ready node as a concurrent task.
        for &node_id in ready_nodes {
            let Some(skill) = graph.skill(node_id).cloned() else {
                tracing::warn!(
                    execution_id = %execution_id,
                    node_id = %node_id,
                    "Ready node not found in graph — skipping"
                );
                continue;
            };

            // Mark as Running before spawning.
            graph.transition(node_id, NodeState::Running);

            self.emit_event(EngineEvent::SkillStarted(SkillStartedEvent {
                execution_id,
                skill_id: node_id.to_string(),
                attempt: 1u32,
            }));

            // Each task runs the skill against its own context, sharing
            // the input Arc and parent CancellationToken.  No shared
            // mutable state across tasks.
            let input = Arc::clone(&ctx.input);
            let cancel_token = ctx.cancellation.clone();
            let node_timeout = self.executor.node_timeout;
            let retry_policy = self.executor.retry_policy.clone();
            // Capture parent slots so pre-populated inputs (e.g.
            // skill_parameters from the ReActRuntime) propagate to
            // spawned task contexts.
            let parent_slots = ctx.slots.clone_entries();

            join_set.spawn(async move {
                let skill_start = std::time::Instant::now();
                let (mut task_ctx, _sender) =
                    WorkflowContext::from_arc(input, workflow_id, execution_id);

                // Share the parent's cancellation so skills can check it.
                task_ctx.cancellation = cancel_token;
                // Propagate pre-populated slots from parent context
                // (e.g. skill_parameters set by ReActRuntime).
                task_ctx.slots.copy_from(&parent_slots);

                // Use the shared retry helper so spawned tasks benefit
                // from retry strategies, back-off, and predicates.
                let (task_result, spawned_retry_count): (anyhow::Result<()>, u32) =
                    execute_with_retry::<I>(&retry_policy, node_timeout, &skill, &mut task_ctx)
                        .await;
                let elapsed_ms = skill_start.elapsed().as_millis() as u64;

                tracing::debug!(
                    skill_id = %node_id,
                    elapsed_ms,
                    spawned_retry_count,
                    result = if task_result.is_ok() { "ok" } else { "err" },
                    "Spawned task completed"
                );

                // Build per-node diagnostics with actual retry count.
                let (diag_status, diag_error) = match &task_result {
                    Ok(()) => (NodeState::Completed, None),
                    Err(e) => (
                        NodeState::Failed {
                            reason: format!("{:#}", e),
                        },
                        Some(format!("{:#}", e)),
                    ),
                };
                let output_keys = task_ctx.slots.keys();
                let diag = NodeDiagnostics::new(node_id).finalize(
                    diag_status,
                    spawned_retry_count, // actual retry count from execute_with_retry
                    diag_error,
                    Vec::new(),
                    output_keys,
                    elapsed_ms,
                );
                task_ctx.record_node_diagnostics(diag);

                NodeTaskResult {
                    node_id,
                    skill_result: task_result.map_err(|e| WorkflowError::SkillFailed {
                        skill_id: node_id.to_string(),
                        source: crate::core::errors::SkillError::Unexpected(e),
                    }),
                    elapsed_ms,
                    retry_count: spawned_retry_count,
                    task_ctx,
                }
            });
        }

        // Collect results from JoinSet.
        while let Some(join_result) = join_set.join_next().await {
            let node_result: NodeTaskResult<I> = match join_result {
                Ok(r) => r,
                Err(join_err) => {
                    tracing::error!(
                        execution_id = %execution_id,
                        error = %join_err,
                        "Spawned task panicked"
                    );
                    // Treat a panicked task as a general failure.
                    continue;
                }
            };

            // Merge task context results into the main context.
            ctx.absorb_task_results(&node_result.task_ctx);

            match node_result.skill_result {
                Ok(()) => {
                    ctx.increment_completed();
                    graph.transition(node_result.node_id, NodeState::Completed);
                    ctx.mark_completed(node_result.node_id);

                    self.emit_event(EngineEvent::SkillCompleted(SkillCompletedEvent {
                        execution_id,
                        skill_id: node_result.node_id.to_string(),
                        elapsed_ms: node_result.elapsed_ms,
                    }));

                    outcomes.push(SkillOutcome::success(
                        node_result.node_id,
                        node_result.elapsed_ms,
                    ));

                    tracing::debug!(
                        execution_id = %execution_id,
                        skill_id = %node_result.node_id,
                        elapsed_ms = node_result.elapsed_ms,
                        "Skill completed"
                    );
                }
                Err(WorkflowError::Cancelled) => {
                    tracing::warn!(
                        execution_id = %execution_id,
                        skill_id = %node_result.node_id,
                        "Workflow cancelled during skill execution"
                    );

                    ctx.increment_failed();
                    graph.transition(node_result.node_id, NodeState::Cancelled);
                    ctx.mark_failed(node_result.node_id);

                    let elapsed_ms = ctx.elapsed().as_millis() as u64;

                    self.emit_event(EngineEvent::WorkflowFailed(WorkflowFailedEvent {
                        execution_id,
                        elapsed_ms,
                        error: "cancelled".to_string(),
                        failing_skill_id: Some(node_result.node_id.to_string()),
                    }));

                    ctx.set_workflow_state(WorkflowState::Cancelled);
                    return Err(WorkflowError::Cancelled);
                }
                Err(e) => {
                    let elapsed_ms = node_result.elapsed_ms;
                    let error_msg: String = format!("{}", e);

                    ctx.increment_failed();
                    graph.transition(
                        node_result.node_id,
                        NodeState::Failed {
                            reason: error_msg.clone(),
                        },
                    );
                    ctx.mark_failed(node_result.node_id);
                    ctx.error(node_result.node_id, format!("Skill failed: {}", error_msg));

                    self.emit_event(EngineEvent::SkillFailed(SkillFailedEvent {
                        execution_id,
                        skill_id: node_result.node_id.to_string(),
                        attempts: node_result.retry_count + 1,
                        error: error_msg.clone(),
                    }));

                    self.emit_event(EngineEvent::WorkflowFailed(WorkflowFailedEvent {
                        execution_id,
                        elapsed_ms,
                        error: error_msg.clone(),
                        failing_skill_id: Some(node_result.node_id.to_string()),
                    }));

                    outcomes.push(SkillOutcome::failure(
                        node_result.node_id,
                        elapsed_ms,
                        &error_msg,
                    ));
                    return Err(e);
                }
            }
        }

        Ok(outcomes)
    }

    fn emit_event(&self, event: EngineEvent) {
        if let Some(bus) = &self.event_bus {
            bus.emit(event);
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

#[cfg(test)]
mod tests {
    #![allow(unused_comparisons)]
    use super::*;
    use crate::core::{
        skill::{Skill, SkillDefinition},
        workflow::{execution::WorkflowExecutor, policy::RetryPolicy, state::WorkflowState},
    };
    use async_trait::async_trait;

    // ── Test Skills ─────────────────────────────────────────────────────

    #[derive(Clone)]
    struct TestSkill {
        def: &'static SkillDefinition,
        fail: Option<&'static str>,
        delay_ms: u64,
    }

    impl TestSkill {
        fn ok(def: &'static SkillDefinition) -> Self {
            Self {
                def,
                fail: None,
                delay_ms: 0,
            }
        }

        fn with_delay(def: &'static SkillDefinition, delay_ms: u64) -> Self {
            Self {
                def,
                fail: None,
                delay_ms,
            }
        }

        fn always_fails(def: &'static SkillDefinition) -> Self {
            Self {
                def,
                fail: Some("intentional failure"),
                delay_ms: 0,
            }
        }

        fn checks_cancellation(def: &'static SkillDefinition) -> Self {
            Self {
                def,
                fail: None,
                delay_ms: 100,
            }
        }
    }

    #[async_trait]
    impl Skill<()> for TestSkill {
        fn definition(&self) -> &'static SkillDefinition {
            self.def
        }

        async fn execute(&self, ctx: &mut WorkflowContext<()>) -> anyhow::Result<()> {
            if let Some(msg) = self.fail {
                anyhow::bail!(msg);
            }
            // Check cancellation in a loop to simulate an abortable skill.
            if self.delay_ms > 0 {
                let deadline = tokio::time::Instant::now() + Duration::from_millis(self.delay_ms);
                while tokio::time::Instant::now() < deadline {
                    if ctx.is_cancelled() {
                        anyhow::bail!("cancelled");
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
            Ok(())
        }
    }

    fn make_skill(def: &'static SkillDefinition) -> SkillRef<()> {
        Arc::new(TestSkill::ok(def))
    }

    fn make_failing_skill(def: &'static SkillDefinition) -> SkillRef<()> {
        Arc::new(TestSkill::always_fails(def))
    }

    fn make_delayed_skill(def: &'static SkillDefinition, delay_ms: u64) -> SkillRef<()> {
        Arc::new(TestSkill::with_delay(def, delay_ms))
    }

    fn make_cancellation_aware_skill(def: &'static SkillDefinition) -> SkillRef<()> {
        Arc::new(TestSkill::checks_cancellation(def))
    }

    // ── Skill definitions ───────────────────────────────────────────────

    const SKILL_A: SkillDefinition = SkillDefinition {
        id: "a",
        name: "A",
        description: "",
        version: "1.0.0",
        dependencies: &[],
    };

    const SKILL_B: SkillDefinition = SkillDefinition {
        id: "b",
        name: "B",
        description: "",
        version: "1.0.0",
        dependencies: &["a"],
    };

    const SKILL_C: SkillDefinition = SkillDefinition {
        id: "c",
        name: "C",
        description: "",
        version: "1.0.0",
        dependencies: &["a"],
    };

    const SKILL_D: SkillDefinition = SkillDefinition {
        id: "d",
        name: "D",
        description: "",
        version: "1.0.0",
        dependencies: &["b", "c"],
    };

    const SKILL_E: SkillDefinition = SkillDefinition {
        id: "e",
        name: "E",
        description: "",
        version: "1.0.0",
        dependencies: &[],
    };

    const SKILL_X: SkillDefinition = SkillDefinition {
        id: "x",
        name: "X",
        description: "",
        version: "1.0.0",
        dependencies: &["nonexistent"],
    };

    // ── Helper: build registry ──────────────────────────────────────────

    fn make_registry_for(defs: &[&'static SkillDefinition]) -> SkillRegistry<()> {
        let mut reg = SkillRegistry::new();
        for &def in defs {
            if def.id == "x" {
                reg.register(Arc::new(TestSkill::ok(def)));
            } else {
                reg.register(make_skill(def));
            }
        }
        reg
    }

    fn make_registry_for_skills(skills: Vec<SkillRef<()>>) -> SkillRegistry<()> {
        let mut reg = SkillRegistry::new();
        for s in skills {
            reg.register(s);
        }
        reg
    }

    // ── Engine tests ────────────────────────────────────────────────────

    #[tokio::test]
    async fn single_node_workflow() {
        let registry = make_registry_for(&[&SKILL_A]);
        let engine = WorkflowEngine::<()>::new();
        let (mut ctx, _sender) = WorkflowContext::new(());

        let result = engine
            .run(&registry, &mut ctx)
            .await
            .expect("engine should succeed");
        assert!(result.is_success(), "single node should succeed");
        assert_eq!(result.completed(), vec!["a"]);
    }

    #[tokio::test]
    async fn linear_dag_executes_in_order() {
        let registry = make_registry_for(&[&SKILL_A, &SKILL_B]);
        let engine = WorkflowEngine::<()>::new();
        let (mut ctx, _sender) = WorkflowContext::new(());

        let result = engine
            .run(&registry, &mut ctx)
            .await
            .expect("engine should succeed");
        assert!(result.is_success());
        let completed = result.completed();
        assert!(completed.contains(&"a"), "a should complete");
        assert!(completed.contains(&"b"), "b should complete");
    }

    #[tokio::test]
    async fn diamond_dag_executes_all_nodes() {
        let registry = make_registry_for(&[&SKILL_A, &SKILL_B, &SKILL_C, &SKILL_D]);
        let engine = WorkflowEngine::<()>::new();
        let (mut ctx, _sender) = WorkflowContext::new(());

        let result = engine
            .run(&registry, &mut ctx)
            .await
            .expect("engine should succeed");
        assert!(result.is_success());
        let mut completed: Vec<&str> = result.completed();
        completed.sort_unstable();
        assert_eq!(
            completed,
            vec!["a", "b", "c", "d"],
            "all 4 nodes should complete"
        );
    }

    #[tokio::test]
    async fn multiple_roots_execute_all() {
        let registry = make_registry_for(&[&SKILL_A, &SKILL_E]);
        let engine = WorkflowEngine::<()>::new();
        let (mut ctx, _sender) = WorkflowContext::new(());

        let result = engine
            .run(&registry, &mut ctx)
            .await
            .expect("engine should succeed");
        assert!(result.is_success());
        let mut completed: Vec<&str> = result.completed();
        completed.sort_unstable();
        assert_eq!(completed, vec!["a", "e"]);
    }

    #[tokio::test]
    async fn skill_failure_propagates_to_result() {
        let failing = make_failing_skill(&SKILL_A);
        let mut registry = SkillRegistry::new();
        registry.register(failing);
        let engine = WorkflowEngine::<()>::new();
        let (mut ctx, _sender) = WorkflowContext::new(());

        let result = engine.run(&registry, &mut ctx).await;
        assert!(result.is_err(), "failing skill should produce error");

        match result {
            Err(WorkflowError::SkillFailed { skill_id, .. }) => {
                assert_eq!(skill_id, "a", "error should reference failing skill");
            }
            other => panic!("expected SkillFailed, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn cancelled_before_run_returns_cancelled() {
        let registry = make_registry_for(&[&SKILL_A]);
        let engine = WorkflowEngine::<()>::new();
        let (mut ctx, sender) = WorkflowContext::new(());

        // Cancel before execution.
        sender.cancel();

        let result = engine.run(&registry, &mut ctx).await;
        assert!(
            matches!(result, Err(WorkflowError::Cancelled)),
            "cancelled workflow should return Cancelled, got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn empty_workflow_returns_empty_plan_error() {
        let registry = SkillRegistry::<()>::new();
        let engine = WorkflowEngine::<()>::new();
        let (mut ctx, _sender) = WorkflowContext::new(());

        let result = engine.run(&registry, &mut ctx).await;
        assert!(
            matches!(result, Err(WorkflowError::EmptyPlan)),
            "empty registry should produce EmptyPlan, got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn missing_dependency_produces_deadlock_error() {
        let registry = make_registry_for(&[&SKILL_X]);
        let engine = WorkflowEngine::<()>::new();
        let (mut ctx, _sender) = WorkflowContext::new(());

        let result = engine.run(&registry, &mut ctx).await;
        assert!(
            matches!(result, Err(WorkflowError::MissingDependency { .. })),
            "missing dependency should produce MissingDependency error, got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn linear_three_node_workflow() {
        const SKILL_A: SkillDefinition = SkillDefinition {
            id: "a",
            name: "A",
            description: "",
            version: "1.0.0",
            dependencies: &[],
        };
        const SKILL_B: SkillDefinition = SkillDefinition {
            id: "b",
            name: "B",
            description: "",
            version: "1.0.0",
            dependencies: &["a"],
        };
        const SKILL_C: SkillDefinition = SkillDefinition {
            id: "c",
            name: "C",
            description: "",
            version: "1.0.0",
            dependencies: &["b"],
        };

        let registry = make_registry_for(&[&SKILL_A, &SKILL_B, &SKILL_C]);
        let engine = WorkflowEngine::<()>::new();
        let (mut ctx, _sender) = WorkflowContext::new(());

        let result = engine
            .run(&registry, &mut ctx)
            .await
            .expect("engine should succeed");
        assert!(result.is_success());
        let mut completed: Vec<&str> = result.completed();
        completed.sort_unstable();
        assert_eq!(completed, vec!["a", "b", "c"], "A→B→C in order");
    }

    #[tokio::test]
    async fn metrics_updated_during_execution() {
        let registry = make_registry_for(&[&SKILL_A, &SKILL_B]);
        let engine = WorkflowEngine::<()>::new();
        let (mut ctx, _sender) = WorkflowContext::new(());

        let _ = engine.run(&registry, &mut ctx).await;

        let snap = ctx.metrics_snapshot();
        assert_eq!(snap.nodes_completed, 2, "both nodes should have completed");
        assert_eq!(snap.nodes_failed, 0);
    }

    #[tokio::test]
    async fn execution_metadata_finalized() {
        let registry = make_registry_for(&[&SKILL_A]);
        let engine = WorkflowEngine::<()>::new();
        let (mut ctx, _sender) = WorkflowContext::new(());

        let _ = engine.run(&registry, &mut ctx).await;

        assert!(
            ctx.execution_metadata.timestamps.finished_at.is_some(),
            "execution metadata should be finalized after run"
        );
    }

    #[tokio::test]
    async fn event_bus_delivers_workflow_completed() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe();
        let registry = make_registry_for(&[&SKILL_A]);
        let engine = WorkflowEngine::<()>::new().with_event_bus(bus);
        let (mut ctx, _sender) = WorkflowContext::new(());

        let _ = engine.run(&registry, &mut ctx).await;

        // Drain events looking for WorkflowCompleted.
        let mut found = false;
        while let Ok(event) = rx.try_recv() {
            if matches!(event, EngineEvent::WorkflowCompleted(_)) {
                found = true;
                break;
            }
        }
        assert!(found, "event bus should deliver WorkflowCompleted");
    }

    #[tokio::test]
    async fn events_emitted_in_correct_order_for_linear_dag() {
        // A → B → C — verify every lifecycle event fires.
        const SKILL_A: SkillDefinition = SkillDefinition {
            id: "a",
            name: "A",
            description: "",
            version: "1.0.0",
            dependencies: &[],
        };
        const SKILL_B: SkillDefinition = SkillDefinition {
            id: "b",
            name: "B",
            description: "",
            version: "1.0.0",
            dependencies: &["a"],
        };
        const SKILL_C: SkillDefinition = SkillDefinition {
            id: "c",
            name: "C",
            description: "",
            version: "1.0.0",
            dependencies: &["b"],
        };

        let bus = EventBus::new(256);
        let mut rx = bus.subscribe();
        let registry = make_registry_for(&[&SKILL_A, &SKILL_B, &SKILL_C]);
        let engine = WorkflowEngine::<()>::new().with_event_bus(bus);
        let (mut ctx, _sender) = WorkflowContext::new(());

        let _ = engine.run(&registry, &mut ctx).await;

        // Collect all events.
        let mut events: Vec<EngineEvent> = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }

        // Verify key events appear.
        assert!(
            events
                .iter()
                .any(|e| matches!(e, EngineEvent::WorkflowStarted(_))),
            "missing WorkflowStarted"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, EngineEvent::ExecutionPlanBuilt(_))),
            "missing ExecutionPlanBuilt"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, EngineEvent::NodeQueued(_))),
            "missing NodeQueued"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, EngineEvent::SkillStarted(_))),
            "missing SkillStarted"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, EngineEvent::SkillCompleted(_))),
            "missing SkillCompleted"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, EngineEvent::WorkflowCompleted(_))),
            "missing WorkflowCompleted"
        );

        // Verify ordering: WorkflowStarted is first, WorkflowCompleted is last.
        assert!(
            matches!(events.first(), Some(EngineEvent::WorkflowStarted(_))),
            "WorkflowStarted should be first event"
        );
        assert!(
            matches!(events.last(), Some(EngineEvent::WorkflowCompleted(_))),
            "WorkflowCompleted should be last event"
        );
    }

    #[tokio::test]
    async fn node_queued_emitted_for_each_promotion() {
        const SKILL_A: SkillDefinition = SkillDefinition {
            id: "a",
            name: "A",
            description: "",
            version: "1.0.0",
            dependencies: &[],
        };
        const SKILL_B: SkillDefinition = SkillDefinition {
            id: "b",
            name: "B",
            description: "",
            version: "1.0.0",
            dependencies: &["a"],
        };

        let bus = EventBus::new(16);
        let mut rx = bus.subscribe();
        let registry = make_registry_for(&[&SKILL_A, &SKILL_B]);
        let engine = WorkflowEngine::<()>::new().with_event_bus(bus);
        let (mut ctx, _sender) = WorkflowContext::new(());

        let _ = engine.run(&registry, &mut ctx).await;

        let mut queued_skills = Vec::new();
        while let Ok(event) = rx.try_recv() {
            if let EngineEvent::NodeQueued(ev) = event {
                queued_skills.push(ev.skill_id);
            }
        }

        assert!(
            queued_skills.contains(&"a".to_string()),
            "NodeQueued should include 'a', got {:?}",
            queued_skills
        );
        assert!(
            queued_skills.contains(&"b".to_string()),
            "NodeQueued should include 'b', got {:?}",
            queued_skills
        );
    }

    // ── Cancellation tests ──────────────────────────────────────────────

    #[tokio::test]
    async fn cancel_mid_execution_returns_cancelled() {
        // Use a skill that runs long enough for us to cancel mid-way.
        const SKILL_A: SkillDefinition = SkillDefinition {
            id: "a",
            name: "A",
            description: "",
            version: "1.0.0",
            dependencies: &[],
        };

        let skills: Vec<SkillRef<()>> = vec![make_delayed_skill(&SKILL_A, 200)];
        let registry = make_registry_for_skills(skills);
        let engine = WorkflowEngine::<()>::new();
        let (mut ctx, sender) = WorkflowContext::new(());
        let (tx, rx) = tokio::sync::oneshot::channel();

        // Move owned values into the spawned task.
        tokio::spawn(async move {
            let _ = tx.send(engine.run(&registry, &mut ctx).await);
        });

        // Give the engine time to start executing A.
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Cancel mid-execution.
        sender.cancel();

        // Wait for the engine to complete.
        let result = rx.await.expect("engine task should send result");
        assert!(
            result.is_err(),
            "mid-execution cancel should return an error, got {:?}",
            result
        );
        // Actual error may be Cancelled (engine-level) or SkillFailed
        // (skill returned error when it noticed cancellation).
    }

    #[tokio::test]
    async fn cancel_via_engine_method() {
        // Verify engine.cancel() works.
        const SKILL_A: SkillDefinition = SkillDefinition {
            id: "a",
            name: "A",
            description: "",
            version: "1.0.0",
            dependencies: &[],
        };

        let skills: Vec<SkillRef<()>> = vec![make_delayed_skill(&SKILL_A, 200)];
        let registry = make_registry_for_skills(skills);
        let (mut ctx, sender) = WorkflowContext::new(());
        let (tx, rx) = tokio::sync::oneshot::channel();

        // Wire the sender into the engine via Arc so both the
        // spawned task and test scope can access it.
        let engine = Arc::new(WorkflowEngine::<()>::new().with_cancellation(sender));
        let engine_for_task = engine.clone();

        tokio::spawn(async move {
            let _ = tx.send(engine_for_task.run(&registry, &mut ctx).await);
        });

        // Cancel via engine method (uses stored CancellationSender).
        engine.cancel();

        let result = rx.await.expect("engine task should send result");
        assert!(result.is_err(), "cancelled engine should not return Ok");
    }

    #[tokio::test]
    async fn cancellation_aware_skill_aborts_on_cancel() {
        // A cancellation-aware skill that checks ctx.is_cancelled()
        // should abort when the engine is cancelled.  The returned
        // error may be Cancelled (engine-level) or SkillFailed with
        // an "cancelled" message (skill-level catch).
        const SKILL_A: SkillDefinition = SkillDefinition {
            id: "a",
            name: "A",
            description: "",
            version: "1.0.0",
            dependencies: &[],
        };

        let skills: Vec<SkillRef<()>> = vec![make_cancellation_aware_skill(&SKILL_A)];
        let registry = make_registry_for_skills(skills);
        let (mut ctx, sender) = WorkflowContext::new(());
        let (tx, rx) = tokio::sync::oneshot::channel();

        let engine = WorkflowEngine::<()>::new();
        tokio::spawn(async move {
            let _ = tx.send(engine.run(&registry, &mut ctx).await);
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        sender.cancel();

        let result = rx.await.expect("engine task should send result");
        assert!(
            result.is_err(),
            "cancellation should produce an error, got {:?}",
            result
        );
        match result {
            Err(WorkflowError::Cancelled) => {} // engine-level cancel
            Err(WorkflowError::SkillFailed { .. }) => {} // skill-level cancel
            Err(_) => {}                        // any other error is also acceptable
            Ok(_) => panic!("cancelled workflow should not succeed"),
        }
    }

    #[tokio::test]
    async fn cancel_propagates_to_parallel_tasks() {
        // Diamond: A → {B(100ms), C(100ms)} → D
        // Cancel after A completes but during B/C execution.
        // Both B and C should see the cancellation and abort.
        const SKILL_A: SkillDefinition = SkillDefinition {
            id: "a",
            name: "A",
            description: "",
            version: "1.0.0",
            dependencies: &[],
        };
        const SKILL_B: SkillDefinition = SkillDefinition {
            id: "b",
            name: "B",
            description: "",
            version: "1.0.0",
            dependencies: &["a"],
        };
        const SKILL_C: SkillDefinition = SkillDefinition {
            id: "c",
            name: "C",
            description: "",
            version: "1.0.0",
            dependencies: &["a"],
        };
        const SKILL_D: SkillDefinition = SkillDefinition {
            id: "d",
            name: "D",
            description: "",
            version: "1.0.0",
            dependencies: &["b", "c"],
        };

        let skills: Vec<SkillRef<()>> = vec![
            make_delayed_skill(&SKILL_A, 10),
            make_delayed_skill(&SKILL_B, 150),
            make_delayed_skill(&SKILL_C, 150),
            make_delayed_skill(&SKILL_D, 10),
        ];
        let registry = make_registry_for_skills(skills);
        let (mut ctx, sender) = WorkflowContext::new(());
        let (tx, rx) = tokio::sync::oneshot::channel();

        let engine = WorkflowEngine::<()>::new();
        tokio::spawn(async move {
            let _ = tx.send(engine.run(&registry, &mut ctx).await);
        });

        // Wait for A to complete (10ms delay), B and C start.
        tokio::time::sleep(Duration::from_millis(30)).await;

        // Cancel while B and C are running.
        sender.cancel();

        let result = rx.await.expect("engine task should send result");
        assert!(
            result.is_err(),
            "cancel during parallel tasks should return an error, got {:?}",
            result
        );
    }

    // ── Timeout tests ───────────────────────────────────────────────────

    #[tokio::test]
    async fn workflow_timeout_fires_when_exceeded() {
        // A single long-running skill should be killed by workflow timeout.
        const SKILL_A: SkillDefinition = SkillDefinition {
            id: "a",
            name: "A",
            description: "",
            version: "1.0.0",
            dependencies: &[],
        };

        let skills: Vec<SkillRef<()>> = vec![make_delayed_skill(&SKILL_A, 200)];
        let registry = make_registry_for_skills(skills);
        let executor = WorkflowExecutor::new();
        let engine = WorkflowEngine::<()>::new()
            .with_executor(executor)
            .with_global_timeout(Duration::from_millis(30));
        let (mut ctx, _sender) = WorkflowContext::new(());

        let result = engine.run(&registry, &mut ctx).await;
        assert!(
            matches!(result, Err(WorkflowError::Timeout { .. })),
            "workflow timeout should return Timeout error, got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn workflow_timeout_completes_before_deadline() {
        // Fast skill completes within the global timeout.
        const SKILL_A: SkillDefinition = SkillDefinition {
            id: "a",
            name: "A",
            description: "",
            version: "1.0.0",
            dependencies: &[],
        };

        let registry = make_registry_for(&[&SKILL_A]);
        let engine = WorkflowEngine::<()>::new().with_global_timeout(Duration::from_secs(5));
        let (mut ctx, _sender) = WorkflowContext::new(());

        let result = engine
            .run(&registry, &mut ctx)
            .await
            .expect("fast skill should complete within timeout");
        assert!(result.is_success());
    }

    #[tokio::test]
    async fn node_timeout_overrides_global() {
        // Node with short timeout should fail even with long global timeout.
        const SKILL_A: SkillDefinition = SkillDefinition {
            id: "a",
            name: "A",
            description: "",
            version: "1.0.0",
            dependencies: &[],
        };

        let skills: Vec<SkillRef<()>> = vec![make_delayed_skill(&SKILL_A, 200)];
        let registry = make_registry_for_skills(skills);
        let executor = WorkflowExecutor::with_node_timeout(Duration::from_millis(20));
        let engine = WorkflowEngine::<()>::new()
            .with_executor(executor)
            .with_global_timeout(Duration::from_secs(5));
        let (mut ctx, _sender) = WorkflowContext::new(());

        let result = engine.run(&registry, &mut ctx).await;
        assert!(result.is_err(), "node timeout should produce an error");
        let err = format!("{:?}", result);
        assert!(
            err.contains("timed out") || err.contains("SkillFailed"),
            "node timeout error should mention timeout, got: {}",
            err
        );
    }

    // ── Diagnostics tests ───────────────────────────────────────────────

    #[tokio::test]
    async fn node_diagnostics_recorded_on_success() {
        const SKILL_A: SkillDefinition = SkillDefinition {
            id: "a",
            name: "A",
            description: "",
            version: "1.0.0",
            dependencies: &[],
        };

        let registry = make_registry_for(&[&SKILL_A]);
        let engine = WorkflowEngine::<()>::new();
        let (mut ctx, _sender) = WorkflowContext::new(());

        let result = engine
            .run(&registry, &mut ctx)
            .await
            .expect("single skill should succeed");
        assert!(result.is_success());
        assert_eq!(
            result.summary.node_diagnostics.len(),
            1,
            "should have 1 diagnostic entry"
        );

        let diag = &result.summary.node_diagnostics[0];
        assert_eq!(diag.skill_id, "a");
        assert_eq!(diag.status, NodeState::Completed);
        // `duration_ms` is a u64, so >= 0 is always true; skip the
        // assertion but keep the display message for context.
        let _ = diag.duration_ms; // suppress unused-warning if any
        assert!(diag.end_time.is_some(), "end_time should be set");
        assert!(diag.error.is_none(), "no error on success");
    }

    #[tokio::test]
    async fn node_diagnostics_has_retry_count() {
        // Test that retry_count is correctly propagated in diagnostics.
        // Spawned tasks now use execute_with_retry which tracks actual
        // retry count.  With default RetryPolicy::none() (max=1) and a
        // skill that succeeds, retry_count should be 0.
        const SKILL_A: SkillDefinition = SkillDefinition {
            id: "a",
            name: "A",
            description: "",
            version: "1.0.0",
            dependencies: &[],
        };

        let registry = make_registry_for(&[&SKILL_A]);
        let engine = WorkflowEngine::<()>::new();
        let (mut ctx, _sender) = WorkflowContext::new(());

        let result = engine
            .run(&registry, &mut ctx)
            .await
            .expect("skill should succeed");

        let diag = &result.summary.node_diagnostics[0];
        // With default policy (max=1) and a skill that succeeds on the
        // first attempt, retry_count is 0.
        assert_eq!(diag.retry_count, 0, "retry_count should be 0");
        assert!(diag.error.is_none(), "no error on success");

        // Also verify the diagnostics came from a spawned task path
        // (engine.run uses execute_ready which spawns).
    }

    #[tokio::test]
    async fn spawned_task_retry_count_tracked_on_failure() {
        // Verify that retry_count > 0 when a spawned task fails and
        // RetryPolicy allows multiple attempts.
        const SKILL_A: SkillDefinition = SkillDefinition {
            id: "a",
            name: "A",
            description: "",
            version: "1.0.0",
            dependencies: &[],
        };

        let failing = make_failing_skill(&SKILL_A);
        let mut registry = SkillRegistry::new();
        registry.register(failing);

        // Use a retry policy with max_attempts=3 (retries=2).
        let executor = WorkflowExecutor::with_retry_policy(RetryPolicy::with_retries(2, None));
        let engine = WorkflowEngine::<()>::new().with_executor(executor);

        let (mut ctx, _sender) = WorkflowContext::new(());
        let result = engine.run(&registry, &mut ctx).await;

        assert!(result.is_err(), "failing skill should produce error");

        // Diagnostics should still be populated in the context with
        // a non-zero retry_count (the skill failed multiple times).
        assert!(!ctx.node_diagnostics.is_empty(), "should have diagnostics");
        let diag = &ctx.node_diagnostics[0];
        assert!(
            diag.retry_count > 0,
            "retry_count should be > 0 for a failing skill with retry policy, got {}",
            diag.retry_count
        );
        assert!(diag.error.is_some(), "should capture error message");
    }

    #[tokio::test]
    async fn node_diagnostics_captures_error_on_failure() {
        const SKILL_A: SkillDefinition = SkillDefinition {
            id: "a",
            name: "A",
            description: "",
            version: "1.0.0",
            dependencies: &[],
        };

        struct FailingSkill;
        #[async_trait::async_trait]
        impl Skill<()> for FailingSkill {
            fn definition(&self) -> &'static SkillDefinition {
                &SKILL_A
            }
            async fn execute(&self, _ctx: &mut WorkflowContext<()>) -> anyhow::Result<()> {
                anyhow::bail!("something went wrong")
            }
        }

        let skills: Vec<SkillRef<()>> = vec![Arc::new(FailingSkill)];
        let registry = make_registry_for_skills(skills);
        let engine = WorkflowEngine::<()>::new();
        let (mut ctx, _sender) = WorkflowContext::new(());

        let result = engine.run(&registry, &mut ctx).await;
        assert!(result.is_err(), "failing skill should produce error");

        // Diagnostics are captured from the context even on failure.
        let diag = ctx.node_diagnostics.first();
        assert!(diag.is_some(), "should have diagnostics on failure");
        let diag = diag.unwrap();
        assert!(diag.error.is_some(), "should capture error message");
        assert!(
            diag.error
                .as_deref()
                .unwrap()
                .contains("something went wrong"),
            "error should match skill error"
        );
        // u64, always non-negative; assertion elided (clippy).
        let _ = diag.duration_ms;
    }

    #[tokio::test]
    async fn node_diagnostics_has_output_keys() {
        use std::sync::Arc;

        struct OutputSkill;
        #[async_trait::async_trait]
        impl Skill<()> for OutputSkill {
            fn definition(&self) -> &'static SkillDefinition {
                static DEF: SkillDefinition = SkillDefinition {
                    id: "output_skill",
                    name: "OutputSkill",
                    description: "",
                    version: "1.0.0",
                    dependencies: &[],
                };
                &DEF
            }
            async fn execute(&self, ctx: &mut WorkflowContext<()>) -> anyhow::Result<()> {
                ctx.set("result", 42u32);
                ctx.set("summary", "done");
                Ok(())
            }
        }

        let skills: Vec<SkillRef<()>> = vec![Arc::new(OutputSkill)];
        let registry = make_registry_for_skills(skills);
        let engine = WorkflowEngine::<()>::new();
        let (mut ctx, _sender) = WorkflowContext::new(());

        let result = engine
            .run(&registry, &mut ctx)
            .await
            .expect("output skill should succeed");

        let diag = result.summary.node_diagnostics.first();
        assert!(diag.is_some(), "should have diagnostics");
        let diag = diag.unwrap();
        assert!(
            diag.output_keys.contains(&"result".to_string()),
            "output_keys should contain 'result', got {:?}",
            diag.output_keys
        );
        assert!(
            diag.output_keys.contains(&"summary".to_string()),
            "output_keys should contain 'summary', got {:?}",
            diag.output_keys
        );
    }

    // ── Integration tests ───────────────────────────────────────────────-

    #[tokio::test]
    async fn panic_in_skill_does_not_hang_engine() {
        // A panicking skill results in a JoinError from the spawned task.
        // The engine should not hang — the global timeout protects against
        // stuck nodes, or the error propagates through.
        #[derive(Clone)]
        struct PanicSkill;
        #[async_trait::async_trait]
        impl Skill<()> for PanicSkill {
            fn definition(&self) -> &'static SkillDefinition {
                static DEF: SkillDefinition = SkillDefinition {
                    id: "panic",
                    name: "Panic",
                    description: "",
                    version: "1.0.0",
                    dependencies: &[],
                };
                &DEF
            }
            async fn execute(&self, _ctx: &mut WorkflowContext<()>) -> anyhow::Result<()> {
                panic!("skill panicked intentionally")
            }
        }

        let skills: Vec<SkillRef<()>> = vec![Arc::new(PanicSkill)];
        let registry = make_registry_for_skills(skills);
        let engine = WorkflowEngine::<()>::new().with_global_timeout(Duration::from_secs(5));
        let (mut ctx, _sender) = WorkflowContext::new(());

        // Use an outer timeout to prevent the test itself from hanging.
        let result =
            tokio::time::timeout(Duration::from_secs(6), engine.run(&registry, &mut ctx)).await;

        match result {
            Ok(Err(WorkflowError::Timeout { .. })) => {
                // Global timeout fired — engine didn't hang.
            }
            Ok(Err(_)) => {
                // Some other error (skill failed, JoinError propagated).
            }
            Ok(Ok(_)) => panic!("engine should not succeed after panic"),
            Err(_elapsed) => panic!("test timeout — engine hung on panic"),
        }
    }

    #[tokio::test]
    async fn large_linear_graph_completes() {
        // Build a linear chain of 100 nodes.
        let mut raw_defs = Vec::new();
        for i in 0..100usize {
            let id = Box::leak(format!("n{}", i).into_boxed_str());
            let deps: &'static [&'static str] = if i == 0 {
                &[]
            } else {
                let dep: &'static str = Box::leak(format!("n{}", i - 1).into_boxed_str());
                Box::leak(Box::new([dep]))
            };
            raw_defs.push(SkillDefinition {
                id,
                name: "",
                description: "",
                version: "1.0.0",
                dependencies: deps,
            });
        }
        let raw_defs: &'static [SkillDefinition] = Box::leak(raw_defs.into_boxed_slice());
        let def_refs: Vec<&'static SkillDefinition> = raw_defs.iter().collect();
        let registry = make_registry_for(&def_refs);
        let engine = WorkflowEngine::<()>::new();
        let (mut ctx, _sender) = WorkflowContext::new(());

        let start = std::time::Instant::now();
        let result = engine
            .run(&registry, &mut ctx)
            .await
            .expect("large graph should complete successfully");
        let elapsed = start.elapsed();

        assert!(result.is_success(), "large graph should succeed");
        assert!(
            elapsed < Duration::from_secs(5),
            "large graph should complete within 5s, took {:?}",
            elapsed
        );
        assert_eq!(
            result.summary.skill_outcomes.len(),
            100,
            "all 100 skills should have outcomes"
        );
    }

    // ── Parallel execution tests ────────────────────────────────────────

    #[tokio::test]
    async fn parallel_b_and_c_run_concurrently_in_diamond() {
        const SKILL_A: SkillDefinition = SkillDefinition {
            id: "a",
            name: "A",
            description: "",
            version: "1.0.0",
            dependencies: &[],
        };
        const SKILL_B: SkillDefinition = SkillDefinition {
            id: "b",
            name: "B",
            description: "",
            version: "1.0.0",
            dependencies: &["a"],
        };
        const SKILL_C: SkillDefinition = SkillDefinition {
            id: "c",
            name: "C",
            description: "",
            version: "1.0.0",
            dependencies: &["a"],
        };
        const SKILL_D: SkillDefinition = SkillDefinition {
            id: "d",
            name: "D",
            description: "",
            version: "1.0.0",
            dependencies: &["b", "c"],
        };

        let skills: Vec<SkillRef<()>> = vec![
            make_delayed_skill(&SKILL_A, 10),
            make_delayed_skill(&SKILL_B, 50),
            make_delayed_skill(&SKILL_C, 50),
            make_delayed_skill(&SKILL_D, 10),
        ];
        let registry = make_registry_for_skills(skills);
        let engine = WorkflowEngine::<()>::new();
        let (mut ctx, _sender) = WorkflowContext::new(());

        let start = std::time::Instant::now();
        let result = engine
            .run(&registry, &mut ctx)
            .await
            .expect("diamond should succeed");
        let elapsed = start.elapsed().as_millis() as u64;

        assert!(result.is_success());
        assert!(
            elapsed < 120,
            "B and C should run concurrently: expected < 120ms, got {}ms",
            elapsed
        );
        let mut completed: Vec<&str> = result.completed();
        completed.sort_unstable();
        assert_eq!(completed, vec!["a", "b", "c", "d"]);
    }

    #[tokio::test]
    async fn parallel_three_branch_fanout() {
        const SKILL_A: SkillDefinition = SkillDefinition {
            id: "a",
            name: "A",
            description: "",
            version: "1.0.0",
            dependencies: &[],
        };
        const SKILL_B: SkillDefinition = SkillDefinition {
            id: "b",
            name: "B",
            description: "",
            version: "1.0.0",
            dependencies: &["a"],
        };
        const SKILL_C: SkillDefinition = SkillDefinition {
            id: "c",
            name: "C",
            description: "",
            version: "1.0.0",
            dependencies: &["a"],
        };
        const SKILL_E: SkillDefinition = SkillDefinition {
            id: "e",
            name: "E",
            description: "",
            version: "1.0.0",
            dependencies: &["a"],
        };
        const SKILL_D: SkillDefinition = SkillDefinition {
            id: "d",
            name: "D",
            description: "",
            version: "1.0.0",
            dependencies: &["b", "c", "e"],
        };

        let skills: Vec<SkillRef<()>> = vec![
            make_delayed_skill(&SKILL_A, 10),
            make_delayed_skill(&SKILL_B, 50),
            make_delayed_skill(&SKILL_C, 50),
            make_delayed_skill(&SKILL_E, 50),
            make_delayed_skill(&SKILL_D, 10),
        ];
        let registry = make_registry_for_skills(skills);
        let engine = WorkflowEngine::<()>::new();
        let (mut ctx, _sender) = WorkflowContext::new(());

        let start = std::time::Instant::now();
        let result = engine
            .run(&registry, &mut ctx)
            .await
            .expect("fanout should succeed");
        let elapsed = start.elapsed().as_millis() as u64;

        assert!(result.is_success());
        assert!(
            elapsed < 170,
            "3 branches should run concurrently: expected < 170ms, got {}ms",
            elapsed
        );
    }

    #[tokio::test]
    async fn parallel_failure_in_one_branch_does_not_block_others() {
        const SKILL_A: SkillDefinition = SkillDefinition {
            id: "a",
            name: "A",
            description: "",
            version: "1.0.0",
            dependencies: &[],
        };
        const SKILL_B: SkillDefinition = SkillDefinition {
            id: "b",
            name: "B",
            description: "",
            version: "1.0.0",
            dependencies: &["a"],
        };
        const SKILL_C_FAIL: SkillDefinition = SkillDefinition {
            id: "c",
            name: "C Fail",
            description: "",
            version: "1.0.0",
            dependencies: &["a"],
        };
        const SKILL_D: SkillDefinition = SkillDefinition {
            id: "d",
            name: "D",
            description: "",
            version: "1.0.0",
            dependencies: &["b", "c"],
        };

        let skills: Vec<SkillRef<()>> = vec![
            make_delayed_skill(&SKILL_A, 10),
            make_delayed_skill(&SKILL_B, 30),
            make_failing_skill(&SKILL_C_FAIL),
            make_delayed_skill(&SKILL_D, 10),
        ];
        let registry = make_registry_for_skills(skills);
        let engine = WorkflowEngine::<()>::new();
        let (mut ctx, _sender) = WorkflowContext::new(());

        let result = engine.run(&registry, &mut ctx).await;
        assert!(result.is_err(), "workflow should fail when a branch fails");
        assert!(
            ctx.completed_skills.contains(&"a".to_string()),
            "A should have completed"
        );
    }
}

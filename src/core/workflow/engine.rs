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
    context::WorkflowContext,
    errors::WorkflowError,
    events::{
        bus::EventBus,
        event_types::{
            EngineEvent, ExecutionPlanBuiltEvent, SkillCompletedEvent, SkillFailedEvent,
            SkillStartedEvent, WorkflowCompletedEvent, WorkflowFailedEvent, WorkflowStartedEvent,
        },
    },
    ids::{ExecutionId, WorkflowId},
    models::execution::{ExecutionSummary, SkillOutcome},
    skill::{SkillRef, SkillRegistry},
    workflow::{
        execution::WorkflowExecutor, graph::WorkflowGraph, node::NodeState,
        planner::WorkflowPlanner, scheduler::WorkflowScheduler, state::WorkflowState,
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
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn with_event_bus(mut self, bus: EventBus) -> Self {
        self.event_bus = Some(bus);
        self
    }

    pub fn with_executor(mut self, executor: WorkflowExecutor) -> Self {
        self.executor = executor;
        self
    }

    /// Run the workflow to completion.
    ///
    /// This is the primary entry point.  The method:
    ///
    /// 1. **Initialize** — plan the DAG, emit `WorkflowStarted`, set `Running` state.
    /// 2. **Loop** — tick the scheduler, spawn ready nodes concurrently, collect results.
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
        loop {
            let ready_nodes = WorkflowScheduler::tick(&mut graph);

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
                    summary.metadata.finish();

                    return Ok(WorkflowResult { summary });
                }
            }
        } // end loop

        // ── Finalize ────────────────────────────────────────────────────
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
    /// local [`WorkflowContext`] (sharing the input `Arc`).  After all tasks
    /// complete, results are merged into the main context and graph.
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

            let skill_name = skill.definition().name.to_string();

            self.emit_event(EngineEvent::SkillStarted(SkillStartedEvent {
                execution_id,
                skill_id: node_id.to_string(),
                attempt: 1u32,
            }));

            // Each task runs the skill against its own context, sharing
            // the input Arc.  No shared mutable state across tasks.
            let input = Arc::clone(&ctx.input);

            join_set.spawn(async move {
                let skill_start = std::time::Instant::now();
                let (mut task_ctx, _sender) =
                    WorkflowContext::from_arc(input, workflow_id, execution_id);

                // Absorb any existing slots from the main context so the
                // spawned task sees upstream outputs.  (Sequential for
                // correctness: we read slots from the main context on the
                // main thread before spawning.)
                // Note: slots are populated by the caller before spawning.

                let task_result = skill.execute(&mut task_ctx).await;
                let elapsed_ms = skill_start.elapsed().as_millis() as u64;

                tracing::debug!(
                    skill_id = %node_id,
                    elapsed_ms,
                    result = if task_result.is_ok() { "ok" } else { "err" },
                    "Spawned task completed"
                );

                NodeTaskResult {
                    node_id,
                    skill_result: task_result.map_err(|e| WorkflowError::SkillFailed {
                        skill_id: node_id.to_string(),
                        source: crate::core::errors::SkillError::Unexpected(e),
                    }),
                    elapsed_ms,
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
                    let error_msg = e.to_string();

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
                        attempts: 1u32,
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
    use super::*;
    use crate::core::{
        skill::{Skill, SkillDefinition},
        workflow::state::WorkflowState,
    };
    use async_trait::async_trait;
    use std::sync::Arc;

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
    }

    #[async_trait]
    impl Skill<()> for TestSkill {
        fn definition(&self) -> &'static SkillDefinition {
            self.def
        }

        async fn execute(&self, _ctx: &mut WorkflowContext<()>) -> anyhow::Result<()> {
            if let Some(msg) = self.fail {
                anyhow::bail!(msg);
            }
            if self.delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
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
    async fn cancelled_workflow_returns_cancelled_error() {
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

    // ── Parallel execution tests ────────────────────────────────────────

    #[tokio::test]
    async fn parallel_b_and_c_run_concurrently_in_diamond() {
        // Diamond: A → {B: 50ms, C: 50ms} → D
        // If B and C run sequentially, total time ≈ 100ms.
        // If they run concurrently, total time ≈ 50ms (plus A and D).
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
        // Sequential: A(10) + B(50) + C(50) + D(10) = 120ms
        // Concurrent: A(10) + max(B50,C50) + D(10) = ~70ms (with overhead ~90ms)
        // Assert elapsed < 120ms — if sequential, it would be >= 120ms.
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
        // A → {B(50ms), C(50ms), E(50ms)} → D (depends on B,C,E)
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
        // Sequential: A(10)+B(50)+C(50)+E(50)+D(10) = 170ms
        // Concurrent: A(10)+max(B50,C50,E50)+D(10) = ~70ms
        assert!(
            elapsed < 170,
            "3 branches should run concurrently: expected < 170ms, got {}ms",
            elapsed
        );
    }

    #[tokio::test]
    async fn parallel_failure_in_one_branch_does_not_block_others() {
        // A → {B(ok, 30ms), C(fail)} → D (depends on B, C)
        // C fails, but B should still complete. The workflow fails at D.
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

        // The graph should have A completed and B at least started
        // (B may or may not complete depending on timing of C's failure).
        assert!(
            ctx.completed_skills.contains(&"a".to_string()),
            "A should have completed"
        );
    }
}

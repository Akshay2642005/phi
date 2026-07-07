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
//! one via [`WorkflowEngine::with_event_bus`].

use std::time::Duration;

use anyhow::Result;

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
    skill::SkillRegistry,
    workflow::{
        execution::WorkflowExecutor, graph::WorkflowGraph, node::NodeState,
        planner::WorkflowPlanner, scheduler::WorkflowScheduler, state::WorkflowState,
    },
};

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
    /// 2. **Loop** — tick the scheduler, execute ready nodes, check completion.
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

            // Execute all ready nodes.
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

    /// Execute a batch of ready nodes, sequentially, updating the graph and
    /// emitting events for each.
    ///
    /// Returns the list of skill outcomes, or the first fatal error.
    async fn execute_ready(
        &self,
        ready_nodes: &[&'static str],
        graph: &mut WorkflowGraph<I>,
        ctx: &mut WorkflowContext<I>,
    ) -> Result<Vec<SkillOutcome>, WorkflowError> {
        let execution_id = ctx.execution_id;
        let mut outcomes = Vec::with_capacity(ready_nodes.len());

        for &node_id in ready_nodes {
            // Look up the skill.
            let Some(skill) = graph.skill(node_id).cloned() else {
                tracing::warn!(
                    execution_id = %execution_id,
                    node_id = %node_id,
                    "Ready node not found in graph — skipping"
                );
                continue;
            };

            let attempt = 1u32;

            self.emit_event(EngineEvent::SkillStarted(SkillStartedEvent {
                execution_id,
                skill_id: node_id.to_string(),
                attempt,
            }));

            let skill_start = std::time::Instant::now();

            match self.executor.execute_node(&skill, graph, ctx).await {
                Ok(()) => {
                    let elapsed_ms = skill_start.elapsed().as_millis() as u64;

                    ctx.increment_completed();

                    self.emit_event(EngineEvent::SkillCompleted(SkillCompletedEvent {
                        execution_id,
                        skill_id: node_id.to_string(),
                        elapsed_ms,
                    }));

                    outcomes.push(SkillOutcome::success(node_id, elapsed_ms));

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

                    ctx.increment_failed();

                    self.emit_event(EngineEvent::WorkflowFailed(WorkflowFailedEvent {
                        execution_id,
                        elapsed_ms,
                        error: "cancelled".to_string(),
                        failing_skill_id: Some(node_id.to_string()),
                    }));

                    ctx.set_workflow_state(WorkflowState::Cancelled);
                    return Err(WorkflowError::Cancelled);
                }
                Err(e) => {
                    let elapsed_ms = skill_start.elapsed().as_millis() as u64;
                    let error_msg = e.to_string();

                    ctx.increment_failed();

                    self.emit_event(EngineEvent::SkillFailed(SkillFailedEvent {
                        execution_id,
                        skill_id: node_id.to_string(),
                        attempts: attempt,
                        error: error_msg.clone(),
                    }));

                    self.emit_event(EngineEvent::WorkflowFailed(WorkflowFailedEvent {
                        execution_id,
                        elapsed_ms,
                        error: error_msg.clone(),
                        failing_skill_id: Some(node_id.to_string()),
                    }));

                    outcomes.push(SkillOutcome::failure(node_id, elapsed_ms, &error_msg));
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
    use crate::core::skill::{Skill, SkillDefinition, SkillRef};
    use async_trait::async_trait;
    use std::sync::Arc;

    // ── Test Skills ─────────────────────────────────────────────────────

    #[derive(Clone)]
    struct TestSkill {
        def: &'static SkillDefinition,
        fail: Option<&'static str>,
    }

    impl TestSkill {
        fn ok(def: &'static SkillDefinition) -> Self {
            Self { def, fail: None }
        }

        fn always_fails(def: &'static SkillDefinition) -> Self {
            Self {
                def,
                fail: Some("intentional failure"),
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
            Ok(())
        }
    }

    fn make_skill(def: &'static SkillDefinition) -> SkillRef<()> {
        Arc::new(TestSkill::ok(def))
    }

    fn make_failing_skill(def: &'static SkillDefinition) -> SkillRef<()> {
        Arc::new(TestSkill::always_fails(def))
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

    fn build_registry(skills: Vec<SkillRef<()>>) -> SkillRegistry<()> {
        let mut reg = SkillRegistry::new();
        for s in skills {
            reg.register(s);
        }
        reg
    }

    fn make_registry_for(defs: &[&'static SkillDefinition]) -> SkillRegistry<()> {
        let mut reg = SkillRegistry::new();
        for &def in defs {
            // For missing-dependency test, register as-is (dependency graph
            // will fail when it resolves the "nonexistent" dep).
            if def.id == "x" {
                reg.register(Arc::new(TestSkill::ok(def)));
            } else {
                reg.register(make_skill(def));
            }
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
    async fn missing_dependency_produces_scheduler_deadlock() {
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
        const SKILL_A_TO_B: SkillDefinition = SkillDefinition {
            id: "a",
            name: "A",
            description: "",
            version: "1.0.0",
            dependencies: &[],
        };
        const SKILL_B_TO_C: SkillDefinition = SkillDefinition {
            id: "b",
            name: "B",
            description: "",
            version: "1.0.0",
            dependencies: &["a"],
        };
        const SKILL_C_LEAF: SkillDefinition = SkillDefinition {
            id: "c",
            name: "C",
            description: "",
            version: "1.0.0",
            dependencies: &["b"],
        };

        let registry = make_registry_for(&[&SKILL_A_TO_B, &SKILL_B_TO_C, &SKILL_C_LEAF]);
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
        // elapsed_ms may be 0 on very fast runs with coarse system timer (Windows).
        // elapsed_ms may be 0 on very fast runs with coarse system timer (Windows).
        // The real assertion is finished_at is set.
        assert!(
            ctx.execution_metadata.timestamps.finished_at.is_some(),
            "execution metadata should have finished_at set after run"
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
}

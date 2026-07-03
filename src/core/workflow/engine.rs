//! Workflow engine — the top-level orchestrator.
//!
//! [`WorkflowEngine`] ties together the planner, scheduler, and executor into
//! a single entry point.  Callers build an engine, hand it a registry and a
//! context, and receive a [`WorkflowResult`] when the run completes.
//!
//! # Pipeline
//! ```text
//! WorkflowPlanner::plan(registry)
//!         │
//!         ▼
//!  ExecutionPlan (ordered skills + DAG)
//!         │
//!         ▼
//!  WorkflowScheduler::tick (promote Pending → Ready)
//!         │
//!         ▼
//!  for each Ready node:
//!    WorkflowExecutor::execute_node
//!         │
//!         ├─ Ok  → Completed → Scheduler::tick → next Ready nodes
//!         └─ Err → Failed    → abort or continue per policy
//!         │
//!         ▼
//!  WorkflowResult
//! ```
//!
//! # Events (stub)
//! The engine emits lifecycle events via a channel.  Currently a no-op — use
//! [`WorkflowEngine::with_event_sink`] to attach a listener when you add one.

use anyhow::Result;

use crate::core::{
    context::WorkflowContext,
    errors::WorkflowError,
    skill::SkillRegistry,
    workflow::{
        events::{WorkflowEvent, WorkflowEventSink},
        execution::WorkflowExecutor,
        node::NodeState,
        planner::WorkflowPlanner,
        scheduler::WorkflowScheduler,
    },
};

// ── Workflow result ───────────────────────────────────────────────────────────

/// The outcome of a completed workflow run.
#[derive(Debug)]
pub struct WorkflowResult {
    /// `true` if all skills completed without error.
    pub success: bool,
    /// IDs of skills that completed successfully.
    pub completed: Vec<String>,
    /// IDs of skills that failed.
    pub failed: Vec<String>,
    /// Total elapsed time in milliseconds.
    pub elapsed_ms: u64,
    /// Any errors that caused the workflow to stop early.
    pub errors: Vec<String>,
}

impl WorkflowResult {
    /// `true` when every planned skill succeeded.
    pub fn is_success(&self) -> bool {
        self.success
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
            event_sink: None,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Attach an event sink for workflow lifecycle events.
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
    /// # Flow
    /// 1. Plan: resolve dependencies, build DAG.
    /// 2. Prime: promote all root nodes (no dependencies) to `Ready`.
    /// 3. Loop: for each `Ready` node, execute it, then tick the scheduler.
    /// 4. Return [`WorkflowResult`].
    ///
    /// # Error policy
    /// Currently fails fast on the first skill error.  A future `continue_on_failure`
    /// flag can be added to `WorkflowEngine` without changing the loop structure.
    pub async fn run(
        &self,
        registry: &SkillRegistry<I>,
        ctx: &mut WorkflowContext<I>,
    ) -> Result<WorkflowResult, WorkflowError> {
        let run_id = ctx.run_id;

        tracing::info!(
            run_id = %run_id,
            "Workflow run started"
        );

        self.emit(WorkflowEvent::WorkflowStarted { run_id: run_id.to_string() });

        // ── Step 1: Plan ──────────────────────────────────────────────────────
        let plan = WorkflowPlanner::plan(registry)?;

        tracing::info!(
            run_id = %run_id,
            skill_count = plan.skill_count(),
            "Execution plan ready"
        );

        let mut graph = plan.graph;

        // ── Step 2: Prime the scheduler (find root nodes) ─────────────────────
        let _initial_ready = WorkflowScheduler::tick(&mut graph);

        // ── Step 3: Execute ───────────────────────────────────────────────────
        // Walk the topologically-sorted list; the scheduler keeps the graph
        // state in sync so a future parallel implementation can use `ready_nodes()`
        // instead of the ordered list.
        let mut errors: Vec<String> = Vec::new();

        for skill in &plan.ordered_skills {
            let def = skill.definition();

            // Check if this node was already promoted to Ready by the scheduler.
            // If not, tick once more to pick up any newly-ready nodes.
            if graph.node_by_id(def.id).map(|n| &n.state) != Some(&NodeState::Ready) {
                WorkflowScheduler::tick(&mut graph);
            }

            self.emit(WorkflowEvent::SkillStarted {
                run_id: run_id.to_string(),
                skill_id: def.id.to_string(),
            });

            match self.executor.execute_node(skill, &mut graph, ctx).await {
                Ok(()) => {
                    self.emit(WorkflowEvent::SkillCompleted {
                        run_id: run_id.to_string(),
                        skill_id: def.id.to_string(),
                        success: true,
                    });

                    // Advance the scheduler so subsequent skills can be promoted.
                    WorkflowScheduler::tick(&mut graph);
                }
                Err(WorkflowError::Cancelled) => {
                    tracing::warn!(run_id = %run_id, "Workflow cancelled");
                    self.emit(WorkflowEvent::WorkflowFailed {
                        run_id: run_id.to_string(),
                        reason: "cancelled".to_string(),
                    });
                    return Err(WorkflowError::Cancelled);
                }
                Err(e) => {
                    let msg = e.to_string();
                    errors.push(msg.clone());

                    self.emit(WorkflowEvent::SkillCompleted {
                        run_id: run_id.to_string(),
                        skill_id: def.id.to_string(),
                        success: false,
                    });
                    self.emit(WorkflowEvent::WorkflowFailed {
                        run_id: run_id.to_string(),
                        reason: msg,
                    });

                    // Fail fast.
                    let elapsed_ms = ctx.elapsed().as_millis() as u64;
                    return Ok(WorkflowResult {
                        success: false,
                        completed: ctx.completed_skills.clone(),
                        failed: ctx.failed_skills.clone(),
                        elapsed_ms,
                        errors,
                    });
                }
            }
        }

        // ── Step 4: Deadlock guard ────────────────────────────────────────────
        WorkflowScheduler::check_deadlock(&graph)?;

        // ── Step 5: Summarise ─────────────────────────────────────────────────
        let elapsed_ms = ctx.elapsed().as_millis() as u64;
        let (done, failed, _skipped) = graph.completion_summary();
        let success = failed == 0 && errors.is_empty();

        tracing::info!(
            run_id = %run_id,
            completed = done,
            failed = failed,
            elapsed_ms,
            "Workflow run finished"
        );

        self.emit(WorkflowEvent::WorkflowCompleted {
            run_id: run_id.to_string(),
            success,
        });

        Ok(WorkflowResult {
            success,
            completed: ctx.completed_skills.clone(),
            failed: ctx.failed_skills.clone(),
            elapsed_ms,
            errors,
        })
    }

    // ── Event emission ────────────────────────────────────────────────────────

    fn emit(&self, event: WorkflowEvent) {
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

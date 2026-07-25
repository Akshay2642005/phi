//! Runtime layer — wraps the workflow engine in a ReAct reasoning loop.
//!
//! The runtime owns:
//! - Workflow lifecycle (start, step, cancel)
//! - The ReAct reasoning loop
//! - Execution coordination between the planner and tool skills
//! - Cancellation, progress tracking
//!
//! # Modules
//! - [`RuntimeInput`]  — the domain input type shared across all runtime skills.
//! - [`planner`]       — `PlannerSkill`: LLM selects which tool skills to run.
//! - [`plugin`]        — `RuntimePlugin`: registers planner + all tool skills.

pub mod planner;
pub mod plugin;

#[derive(Debug, Clone)]
pub struct RuntimeInput {
    pub goal: String,
    pub context: Option<String>,
}

pub type WorkerInput = RuntimeInput;

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use async_trait::async_trait;

    use crate::core::{
        context::WorkflowContext,
        events::bus::EventBus,
        events::event_types::{
            EngineEvent, SkillCompletedEvent, SkillStartedEvent, WorkflowCompletedEvent,
            WorkflowStartedEvent,
        },
        llm::service::mock::MockLlmService,
        llm::LlmServiceRef,
        skill::{Skill, SkillDefinition, SkillRegistry},
        workflow::engine::WorkflowEngine,
    };
    use crate::runtime::{
        planner::{PlannerSkill, SkillSummary},
        RuntimeInput,
    };

    // ── Mock tool skill ───────────────────────────────────────────────

    const MOCK_TOOL_DEF: SkillDefinition = SkillDefinition {
        id: "mock_tool",
        name: "Mock Tool",
        description: "A test tool that records invocations.",
        version: "1.0.0",
        dependencies: &[],
    };

    struct MockToolSkill {
        invoke_count: Arc<AtomicUsize>,
    }

    impl MockToolSkill {
        fn new() -> (Self, Arc<AtomicUsize>) {
            let count = Arc::new(AtomicUsize::new(0));
            (Self {
                invoke_count: Arc::clone(&count),
            }, count)
        }
    }

    #[async_trait]
    impl Skill<RuntimeInput> for MockToolSkill {
        fn definition(&self) -> &'static SkillDefinition {
            &MOCK_TOOL_DEF
        }

        async fn execute(&self, ctx: &mut WorkflowContext<RuntimeInput>) -> anyhow::Result<()> {
            self.invoke_count.fetch_add(1, Ordering::SeqCst);
            let input = &ctx.input;
            ctx.set(
                MOCK_TOOL_DEF.id,
                format!("processed goal: {}", input.goal),
            );
            Ok(())
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────

    fn build_plan_json(
        reasoning: &str,
        skills: &[&str],
        complete: bool,
        answer: Option<&str>,
    ) -> String {
        let skills_json: Vec<String> = skills.iter().map(|s| format!("\"{s}\"")).collect();
        let answer_field = match answer {
            Some(a) => format!("\"{}\"", a),
            None => "null".to_string(),
        };
        format!(
            r#"{{"reasoning":"{reasoning}","skills_to_run":[{}],"goal_complete":{},"final_answer":{}}}"#,
            skills_json.join(","),
            complete,
            answer_field,
        )
    }

    // ── Tests ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn react_integration_mock_llm_calls_skills() {
        // The engine runs the planner and tool skills in a single DAG
        // pass.  The planner writes PlanOutput to context; tools run
        // concurrently (no deps).  This test verifies the full pipeline:
        // LLM → parse → skill execution → context slots.
        let responses = vec![
            build_plan_json(
                "Need to run mock_tool first",
                &["mock_tool"],
                true,
                Some("Final result: 42"),
            ),
        ];

        let mock_llm = Arc::new(MockLlmService::new(responses));
        let llm_ref: LlmServiceRef = mock_llm.clone();

        let (tool_skill, tool_count) = MockToolSkill::new();

        let catalogue = vec![SkillSummary {
            id: MOCK_TOOL_DEF.id,
            description: MOCK_TOOL_DEF.description,
        }];

        let planner = PlannerSkill::new(llm_ref, catalogue);

        let mut registry = SkillRegistry::new();
        registry.register(planner);
        registry.register(Arc::new(tool_skill));

        let engine = WorkflowEngine::new();
        let (mut ctx, _tx) = WorkflowContext::new(RuntimeInput {
            goal: "test the mock tool".to_string(),
            context: None,
        });

        let result = engine.run(&registry, &mut ctx).await;

        assert!(result.is_ok(), "engine should succeed, got {:?}", result);
        let result = result.unwrap();
        assert!(result.is_success(), "workflow should be successful");

        // Planner should have been called once.
        assert_eq!(mock_llm.call_count(), 1, "planner called 1 time");

        // Mock tool should have been called once.
        assert_eq!(tool_count.load(Ordering::SeqCst), 1, "tool called 1 time");

        // Plan output should be in context.
        let plan_output = ctx
            .get::<crate::runtime::planner::PlanOutput>("plan")
            .expect("plan output should exist");
        assert!(
            plan_output.goal_complete,
            "plan should mark complete"
        );
        assert_eq!(
            plan_output.final_answer.as_deref(),
            Some("Final result: 42"),
            "final answer should match"
        );
    }

    #[tokio::test]
    async fn events_emitted_in_react_loop() {
        // Verify that a single-pass ReAct workflow emits the expected
        // lifecycle events on the EventBus.
        let responses = vec![
            build_plan_json(
                "Need to run mock_tool first",
                &["mock_tool"],
                true,
                Some("done"),
            ),
        ];

        let mock_llm = Arc::new(MockLlmService::new(responses));
        let llm_ref: LlmServiceRef = mock_llm.clone();

        let (tool_skill, _tool_count) = MockToolSkill::new();

        let catalogue = vec![SkillSummary {
            id: MOCK_TOOL_DEF.id,
            description: MOCK_TOOL_DEF.description,
        }];

        let planner = PlannerSkill::new(llm_ref, catalogue);

        let mut registry = SkillRegistry::new();
        registry.register(planner);
        registry.register(Arc::new(tool_skill));

        let bus = EventBus::new(64);
        let mut rx = bus.subscribe();

        let engine = WorkflowEngine::new().with_event_bus(bus);
        let (mut ctx, _tx) = WorkflowContext::new(RuntimeInput {
            goal: "test events".to_string(),
            context: None,
        });

        // Collect events while running.
        let events_handle = tokio::spawn(async move {
            let mut events: Vec<String> = Vec::new();
            use tokio::time::{timeout, Duration};
            loop {
                match timeout(Duration::from_secs(2), rx.recv()).await {
                    Ok(Ok(event)) => {
                        let name = match &event {
                            EngineEvent::WorkflowStarted(_) => "WorkflowStarted",
                            EngineEvent::ExecutionPlanBuilt(_) => "ExecutionPlanBuilt",
                            EngineEvent::SkillStarted(_) => "SkillStarted",
                            EngineEvent::SkillCompleted(_) => "SkillCompleted",
                            EngineEvent::WorkflowCompleted(_) => "WorkflowCompleted",
                            _ => continue,
                        };
                        events.push(name.to_string());
                        if matches!(event, EngineEvent::WorkflowCompleted(_)) {
                            break;
                        }
                    }
                    _ => break,
                }
            }
            events
        });

        let result = engine.run(&registry, &mut ctx).await;
        assert!(result.is_ok(), "engine should succeed");

        let events = events_handle.await.expect("events task completed");

        // Verify key lifecycle events fired during the run.
        assert!(
            events.contains(&"WorkflowStarted".to_string()),
            "should emit WorkflowStarted, got: {:?}",
            events
        );
        assert!(
            events.contains(&"WorkflowCompleted".to_string()),
            "should emit WorkflowCompleted, got: {:?}",
            events
        );
        assert!(
            events.contains(&"SkillStarted".to_string()),
            "should emit SkillStarted, got: {:?}",
            events
        );
        assert!(
            events.contains(&"SkillCompleted".to_string()),
            "should emit SkillCompleted, got: {:?}",
            events
        );
        assert!(
            events.contains(&"ExecutionPlanBuilt".to_string()),
            "should emit ExecutionPlanBuilt, got: {:?}",
            events
        );
    }
}

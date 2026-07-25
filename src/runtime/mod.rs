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
pub mod react;

use serde::{Deserialize, Serialize};

pub use react::ReActRuntime;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeInput {
    pub goal: String,
    pub context: Option<String>,
}

pub type WorkerInput = RuntimeInput;

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;

    use crate::core::{
        context::WorkflowContext,
        events::bus::EventBus,
        events::event_types::{
            EngineEvent, SkillCompletedEvent, SkillStartedEvent, WorkflowCompletedEvent,
            WorkflowStartedEvent,
        },
        llm::LlmServiceRef,
        llm::service::mock::MockLlmService,
        skill::{Skill, SkillDefinition, SkillRegistry},
        workflow::engine::WorkflowEngine,
    };
    use crate::runtime::{
        ReActRuntime, RuntimeInput,
        planner::{PlannerSkill, SkillSummary},
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
            (
                Self {
                    invoke_count: Arc::clone(&count),
                },
                count,
            )
        }
    }

    #[async_trait]
    impl Skill<RuntimeInput> for MockToolSkill {
        fn definition(&self) -> &'static SkillDefinition {
            &MOCK_TOOL_DEF
        }

        async fn execute(&self, ctx: &mut WorkflowContext<RuntimeInput>) -> anyhow::Result<()> {
            self.invoke_count.fetch_add(1, Ordering::SeqCst);
            // Check for pre-populated input from planner (skill_parameters).
            let input_src = ctx
                .get::<String>(MOCK_TOOL_DEF.id)
                .map(|s| s.as_ref().clone())
                .unwrap_or_else(|| ctx.input.goal.clone());
            let processed = format!("processed: {}", input_src);
            ctx.set_with_display(MOCK_TOOL_DEF.id, processed.clone(), processed);
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
            r#"{{"reasoning":"{reasoning}","skills_to_run":[{}],"skill_parameters":{{}},"goal_complete":{},"final_answer":{}}}"#,
            skills_json.join(","),
            complete,
            answer_field,
        )
    }

    /// Like `build_plan_json` but with per-skill parameters.
    fn build_plan_json_with_params(
        reasoning: &str,
        skills: &[&str],
        params: &[(&str, &str)],
        complete: bool,
        answer: Option<&str>,
    ) -> String {
        let skills_json: Vec<String> = skills.iter().map(|s| format!("\"{s}\"")).collect();
        let answer_field = match answer {
            Some(a) => format!("\"{}\"", a),
            None => "null".to_string(),
        };
        let params_json: Vec<String> = params
            .iter()
            .map(|(k, v)| format!("\"{k}\":\"{v}\""))
            .collect();
        format!(
            r#"{{"reasoning":"{reasoning}","skills_to_run":[{}],"skill_parameters":{{{}}},"goal_complete":{},"final_answer":{}}}"#,
            skills_json.join(","),
            params_json.join(","),
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
        let responses = vec![build_plan_json(
            "Need to run mock_tool first",
            &["mock_tool"],
            true,
            Some("Final result: 42"),
        )];

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
        assert!(plan_output.goal_complete, "plan should mark complete");
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
        let responses = vec![build_plan_json(
            "Need to run mock_tool first",
            &["mock_tool"],
            true,
            Some("done"),
        )];

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
            use tokio::time::{Duration, timeout};
            while let Ok(Ok(event)) = timeout(Duration::from_secs(2), rx.recv()).await {
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

    // ── ReActRuntime multi-step tests ────────────────────────────────

    #[tokio::test]
    async fn react_runtime_completes_in_one_step() {
        // Planner completes immediately with goal_complete=true.
        let responses = vec![build_plan_json(
            "Already know the answer",
            &[],
            true,
            Some("42"),
        )];
        let mock_llm = Arc::new(MockLlmService::new(responses));
        let llm_ref: LlmServiceRef = mock_llm.clone();

        let catalogue = vec![];
        let planner = PlannerSkill::new(llm_ref, catalogue);

        let mut registry = SkillRegistry::new();
        registry.register(planner);

        let runtime = ReActRuntime::<RuntimeInput>::new();
        let (mut ctx, _tx) = WorkflowContext::new(RuntimeInput {
            goal: "the meaning of life".to_string(),
            context: None,
        });

        let result = runtime.run(&registry, &mut ctx).await;
        assert!(result.is_ok(), "react runtime should succeed");
        let react_result = result.unwrap();
        assert_eq!(react_result.iterations, 1, "one iteration");
        assert_eq!(react_result.final_answer, "42", "correct answer");
    }

    #[tokio::test]
    async fn react_runtime_multi_step_loop() {
        // Planner: run mock_tool → Planner again: complete.
        let responses = vec![
            build_plan_json("Need to run mock_tool", &["mock_tool"], false, None),
            build_plan_json(
                "Got the result, now complete",
                &[],
                true,
                Some("final answer after tool"),
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

        let runtime = ReActRuntime::<RuntimeInput>::new();
        let (mut ctx, _tx) = WorkflowContext::new(RuntimeInput {
            goal: "multi-step test".to_string(),
            context: None,
        });

        let result = runtime.run(&registry, &mut ctx).await;
        assert!(result.is_ok(), "react runtime should succeed");
        let react_result = result.unwrap();
        assert_eq!(
            react_result.iterations, 2,
            "two iterations (plan + execute)"
        );
        assert!(
            react_result
                .final_answer
                .contains("processed: multi-step test"),
            "should contain tool output, got: {}",
            react_result.final_answer
        );
        assert_eq!(tool_count.load(Ordering::SeqCst), 1, "tool called once");
        assert_eq!(
            mock_llm.call_count(),
            1,
            "planner called once (no confirmation re-prompt)"
        );
    }

    #[tokio::test]
    async fn react_runtime_multi_step_two_tool_runs() {
        // Planner: run mock_tool → complete.
        // This exercises the multi-step loop with tool execution.
        let responses = vec![
            build_plan_json("First tool run", &["mock_tool"], false, None),
            build_plan_json("Tool done, complete", &[], true, Some("done with tool run")),
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

        let runtime = ReActRuntime::<RuntimeInput>::new();
        let (mut ctx, _tx) = WorkflowContext::new(RuntimeInput {
            goal: "two-tool-run test".to_string(),
            context: None,
        });

        let result = runtime.run(&registry, &mut ctx).await;
        assert!(result.is_ok(), "react runtime should succeed");
        let react_result = result.unwrap();
        assert_eq!(
            react_result.iterations, 2,
            "two iterations (plan + execute)"
        );
        assert!(
            react_result
                .final_answer
                .contains("processed: two-tool-run test"),
            "should contain tool output, got: {}",
            react_result.final_answer
        );
        assert_eq!(tool_count.load(Ordering::SeqCst), 1, "tool called once");
        assert_eq!(
            mock_llm.call_count(),
            1,
            "planner called once (no confirmation re-prompt)"
        );
    }

    #[tokio::test]
    async fn react_runtime_events_emitted_multi_step() {
        // Verify that EventBus receives lifecycle events across
        // plan + execute phases.
        let responses = vec![
            build_plan_json("First step", &["mock_tool"], false, None),
            build_plan_json("Done", &[], true, Some("multi-step with events")),
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

        let runtime = ReActRuntime::<RuntimeInput>::new().with_event_bus(bus);
        let (mut ctx, _tx) = WorkflowContext::new(RuntimeInput {
            goal: "multi-step events".to_string(),
            context: None,
        });

        // Collect events while running.
        let events_handle = tokio::spawn(async move {
            let mut events: Vec<String> = Vec::new();
            let mut workflow_completed_count = 0usize;
            use tokio::time::{Duration, timeout};
            #[allow(clippy::while_let_loop)]
            // Complex condition needs loop+match, not while-let.
            loop {
                match timeout(Duration::from_secs(3), rx.recv()).await {
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
                            workflow_completed_count += 1;
                            if workflow_completed_count >= 2 {
                                break;
                            }
                        }
                    }
                    _ => break,
                }
            }
            events
        });

        let result = runtime.run(&registry, &mut ctx).await;
        assert!(result.is_ok(), "react runtime should succeed");
        let react_result = result.unwrap();
        assert_eq!(
            react_result.iterations, 2,
            "two iterations (plan + execute)"
        );

        let events = events_handle.await.expect("events task completed");
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
    }

    #[tokio::test]
    async fn react_runtime_missing_planner_returns_error() {
        // Registry without a "plan" skill should fail immediately.
        let registry = SkillRegistry::<RuntimeInput>::new();
        let runtime = ReActRuntime::<RuntimeInput>::new();
        let (mut ctx, _tx) = WorkflowContext::new(RuntimeInput {
            goal: "no planner".to_string(),
            context: None,
        });

        let result = runtime.run(&registry, &mut ctx).await;
        assert!(result.is_err(), "should fail with missing planner");
        let err = result.unwrap_err();
        let err_str = format!("{:#}", err);
        assert!(
            err_str.contains("no planner skill"),
            "error should mention missing planner, got: {}",
            err_str
        );
    }

    #[tokio::test]
    async fn react_runtime_empty_skills_to_run_without_complete() {
        // Planner says goal_complete=false but skills_to_run is empty.
        // The runtime now treats this as an error (no skills to execute).
        let responses = vec![build_plan_json(
            "No skills needed but not done?",
            &[],
            false,
            None,
        )];
        let mock_llm = Arc::new(MockLlmService::new(responses));
        let llm_ref: LlmServiceRef = mock_llm.clone();

        let catalogue = vec![];
        let planner = PlannerSkill::new(llm_ref, catalogue);

        let mut registry = SkillRegistry::new();
        registry.register(planner);

        let runtime = ReActRuntime::<RuntimeInput>::new();
        let (mut ctx, _tx) = WorkflowContext::new(RuntimeInput {
            goal: "ambiguous".to_string(),
            context: None,
        });

        let result = runtime.run(&registry, &mut ctx).await;
        assert!(
            result.is_err(),
            "should error when planner has no skills to run"
        );
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("no skills to run"),
            "should mention empty skills"
        );
    }

    #[tokio::test]
    async fn react_runtime_passes_skill_parameters_to_tools() {
        // The planner returns skill_parameters with goal_complete=false
        // so the ReActRuntime runs the tool with the parameter value.
        let responses = vec![
            build_plan_json_with_params(
                "Need to calculate 25*4+10",
                &["mock_tool"],
                &[("mock_tool", "25 * 4 + 10")],
                false,
                None,
            ),
            build_plan_json("Done", &[], true, Some("110")),
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

        let runtime = ReActRuntime::<RuntimeInput>::new();
        let (mut ctx, _tx) = WorkflowContext::new(RuntimeInput {
            goal: "calculate 25 * 4 + 10, then search".to_string(),
            context: None,
        });

        let result = runtime.run(&registry, &mut ctx).await;
        assert!(result.is_ok(), "should succeed");

        // The tool should have been called once (iteration 1), then
        // the planner completes (iteration 2).
        assert_eq!(tool_count.load(Ordering::SeqCst), 1, "tool called once");

        // The tool's output slot should contain the pre-populated
        // parameter value "25 * 4 + 10", not the raw goal.
        let slot_value = ctx
            .get::<String>(MOCK_TOOL_DEF.id)
            .map(|s| s.as_ref().clone())
            .expect("tool's output slot should exist after execution");
        assert!(
            slot_value.contains("25 * 4 + 10"),
            "tool should have received parameter value '25 * 4 + 10', got: {}",
            slot_value
        );
        // It should NOT contain the raw goal text.
        assert!(
            !slot_value.contains("then search"),
            "tool should NOT have received the full raw goal"
        );
    }
}

### Story e02s02: CLI Run Command — Implementation Steps

**type:** feat
**risk:** P0
**context:** infra
**BCPs:** 8

**Context:** Phase 1 built the engine internals but nothing wires them together.
`cargo run` builds AppContainer and logs "ready" without executing any workflow.
This story adds a CLI `run` command that accepts a goal string, wires
RuntimePlugin into the engine, attaches EventBus to capture lifecycle events,
executes a full ReAct workflow, and displays the final answer (or error).

## Steps

1. Add CLI arg parsing in main.rs — read first positional arg as goal string
   → verify: `cargo run -- "test" 2>&1 | head -3 | grep -q 'Running workflow' && echo OK || echo FAIL`

2. Wire RuntimePlugin into AppContainer.build() — register planner + tools during
   container construction so the engine has skills to run
   → verify: `grep -q 'RuntimePlugin' src/bootstrap/container.rs && echo OK || echo MISSING`

3. Create EventBus in main() and attach to WorkflowEngine — the bus captures
   engine lifecycle events so a console subscriber can print them
   → verify: `grep -q 'EventBus' src/main.rs && echo OK || echo MISSING`

4. Add console event subscriber — print human-readable lines for WorkflowStarted,
   SkillStarted, SkillCompleted, WorkflowCompleted, WorkflowFailed, WorkflowCancelled
   → verify: `grep -q 'WorkflowStarted' src/main.rs && echo OK subscriber || echo MISSING`

5. Build run_workflow async fn: RuntimePlugin::register → SkillRegistry →
   WorkflowEngine::new → engine.run → display summary and final answer
   → verify: `grep -q 'fn run_workflow' src/main.rs && echo OK || echo MISSING`

6. Handle all exit paths: success (print final answer + summary), failure (print
   error + diagnostics), cancellation (print cancelled + partial results)
   → verify: `grep -q 'WorkflowResult' src/main.rs && echo OK || echo MISSING`

7. Unit test for CLI arg parsing (empty args, goal string, multiple words)
   → verify: `cargo test -- e02s02 2>&1 | grep -E '^(test result|ok|FAILED)'`

8. Integration test: run_workflow with mock skills returns expected summary
   → verify: `cargo test -- run_workflow 2>&1 | grep -E '^(test result|ok|FAILED)'`

9. Full test suite passes, update execution-status.yaml, push branch
   → verify: `cargo test --lib 2>&1 | tail -3 | grep '135 passed' && echo OK || echo CHECK COUNT`

## Verification Script (Step-by-Step)

1. `cargo run -- "what's the weather?"` — expect console output showing:
   - WorkflowStarted event
   - PlannerSkill starts (LLM plan step)
   - Tool skills execute
   - WorkflowCompleted or final answer displayed

2. `cargo run` (no args) — expect usage message

3. `cargo test --lib` — all existing 135 tests still pass

## Out of scope

- Pretty-printing / colored output (future ergonomic improvement)
- Interactive REPL (deferred to post-Phase 2)
- CLI flags beyond the single positional goal arg
- WebSocket or streaming output

## Risks

- **RuntimePlugin requires LlmService**: Without a running Ollama instance,
  `cargo run` will fail with a connection error. This is expected — the test
  suite uses mock skills, not real LLM calls.
- **EventBus subscriber blocks**: If subscriber is too slow, it could slow the
  engine loop. Use a separate async task for the subscriber to avoid blocking.

# ADR-0001: Multi-step ReAct loop orchestrator

- **Status:** Accepted (retroactively registered)
- **Date:** 2026-07-25
- **Decided in:** commits `95fe3a5` / `50f0c29` / `15b15e7` (merged to `master` at `1fb6c83`)

## Context

The engine is a generic DAG orchestrator: register skills, plan a DAG, run it
once. But a ReAct agent needs an iterative Plan → Act → Observe cycle where
the LLM decides *which* skills to run, sees their outputs, and re-plans until
the goal is complete. Phase 2 delivered the pieces (planner skill, tool
skills, CLI) but the run was single-shot: plan once, execute once, answer.

Two design questions drove the decision:

1. **Where does the loop live?** The core `WorkflowEngine` must stay generic —
   no ReAct coupling in `core/`. The loop belongs in the runtime layer.
2. **How does the loop terminate reliably?** An LLM planner under format
   pressure will re-nominate already-completed skills (see
   `specs/bugs/BUG-20260725T1619-infinite-react-loop.md`), so termination
   cannot rely on prompt discipline alone.

## Decision

Add `ReActRuntime<I>` in `src/runtime/react.rs` — a thin orchestrator that
wraps `WorkflowEngine` and drives a two-phase loop:

1. **Plan** — run the planner once via the engine; get the ordered
   `skills_to_run` list from `PlanOutput`.
2. **Act** — execute each nominated tool skill in its own `engine.run()`
   call, feeding results back into the shared `WorkflowContext`.
3. **Observe** — call the planner again; if `goal_complete`, return the
   final answer; otherwise loop back to Act.

Termination guards (the hard-won part):

- **Completed-skill filtering** — the runtime filters out already-completed
  skills before executing, so each skill runs at most once per run.
- **Deduplicated planner prompt** — the completed-skills list shown to the
  LLM contains each skill exactly once.
- **`MAX_ITERATIONS = 20`** — a hard cap; on exhaustion the run stops and
  returns the best accumulated answer with `max_reached: true`.

Supporting change: `SkillRegistry::filter_to_ids()` builds a tool
sub-registry from the planner's `skills_to_run` without rebuilding the DAG
from scratch.

## Consequences

- **Positive:** true multi-step reasoning (facts gathered in one step can be
  used in the next); the loop terminates deterministically even with a
  misbehaving planner; core engine stays provider- and ReAct-agnostic.
- **Negative:** a goal needing many tools may exhaust `MAX_ITERATIONS` on
  small local models — surfaced gracefully via `max_reached` rather than
  failing silently.
- **Trade-off accepted:** the loop is sequential (one skill at a time) even
  though the engine supports parallel execution — parallel tool runs inside
  a ReAct step are future work.

## References

- `src/runtime/react.rs`, `src/runtime/planner.rs`, `src/core/skill/mod.rs`
- `docs/architecture.md` §Layer 2 (ReAct Runtime)

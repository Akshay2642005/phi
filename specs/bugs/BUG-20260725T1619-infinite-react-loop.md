# BUG-20260725T1619: Infinite ReAct loop — planner re-nominates completed skills

**Status:** resolved (retroactively registered)
**Found:** 2026-07-25 · **Fixed:** `f23c775` (fixup webserach)
**Severity:** High · **Priority:** High
**Scope:** ReAct runtime (`runtime/`)

## Problem

A goal run through the ReAct runtime could loop forever.

- **What happens (actual):** The LLM planner keeps returning the same
  already-executed skill in `skills_to_run` on every iteration. The runtime
  re-executed it each time, so the run never reached `goal_complete = true`
  and produced no final answer — an unbounded loop.
- **What should happen (expected):** Each Plan→Act→Observe cycle makes
  progress. A skill is executed at most once per run, and the loop terminates
  with a final answer within a bounded number of iterations.
- **How to reproduce:** Run a goal requiring tool use with a small/local LLM.
  The planner's next-iteration prompt still listed completed skills, so the
  model frequently re-nominated the first skill (e.g. `web_search`) forever.

## Root Cause Analysis

- **The code path involved:** The planner prompt listed the full available
  skill catalogue plus a `completed_skills` section. The runtime loop executed
  whatever `skills_to_run` named without checking whether a skill had already
  completed; the planner had no guard preventing re-selection either.
- **Why the current code failed:** There was no enforcement of the
  "execute each skill once" invariant at the runtime boundary — only a
  prompt-level suggestion. Small models ignore prompt suggestions under
  format pressure, so the same skill ID was re-nominated and re-executed
  indefinitely.
- **Contributing factors:** (1) The completed-skills list was rendered with
  possible duplicates, blurring the signal; (2) no iteration cap existed, so
  a misbehaving planner had nothing to stop it; (3) the display of results
  did not distinguish "new result" from "already-known result", giving the
  planner no feedback that it was repeating itself.
- **Risk level:** High — any deployed goal run could hang forever.

## TDD Fix Plan

Implemented in `f23c775` (registered retroactively; the cycles below describe
what was done):

1. **RED**: Write a multi-step ReAct test where the fake planner returns the
   same skill ID on consecutive iterations; assert the run terminates with a
   final answer and the skill executes exactly once.
   **GREEN**: Filter out already-completed skills in the runtime loop before
   executing; skip and log `"Skill already completed — skipping"` when the
   planner re-nominates one.
   **verify**: `cargo test runtime::react`

2. **RED**: Write a test asserting the run returns `max_reached: true` and a
   graceful final answer when the planner never declares completion.
   **GREEN**: Add `MAX_ITERATIONS = 20` safety cap; when the cap is hit,
   stop the loop and emit the best accumulated answer instead of looping.
   **verify**: `cargo test runtime::react`

3. **RED**: Write a test asserting the planner prompt shows each completed
   skill exactly once.
   **GREEN**: Deduplicate the completed-skills list when building the prompt.
   **verify**: `cargo test runtime::planner`

**REFACTOR**: Extract the completed-skill snapshot into an owned `BTreeSet`
so the filter is deterministic across loop iterations.

## Acceptance Criteria

- [x] A goal run terminates within `MAX_ITERATIONS` iterations.
- [x] No skill executes more than once per run.
- [x] Planner prompt lists each completed skill exactly once.
- [x] `max_reached` is surfaced in the result when the cap is hit.
- [x] All new regression tests pass.
- [x] Existing tests still pass (161 total).

## Resolution

Fixed and merged via `f23c775` (part of the e03 multi-step ReAct work,
merged to `master` at `1fb6c83`). Verification: `cargo test` → 161 passed,
0 failed; `cargo clippy` → clean. Regression coverage lives in
`runtime/react.rs` tests (`iterations` / `max_reached` assertions).

## References

<!-- story: e03s04 -->

- `src/runtime/react.rs` — loop orchestrator, `MAX_ITERATIONS`, completed-skill filter
- `src/runtime/planner.rs` — prompt builder with deduplicated completed-skill list

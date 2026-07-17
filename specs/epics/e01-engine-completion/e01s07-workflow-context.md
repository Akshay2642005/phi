# Story e01s07: WorkflowContext — Artifacts, Metadata, Event Bus, Metrics

**type:** feat
**risk:** P0 (core engine internals, 5 BCPs)
**context:** infra
**BCPs:** 5
**status:** planned

## Context

`WorkflowContext<I>` is the shared memory for a single workflow run. It is passed
(mutably) through every skill execution. Currently it holds: `workflow_id`,
`execution_id`, `started_at`, `workflow_state`, `input` (Arc), `slots` (SlotMap),
`variables` (DashMap), `cancellation` (CancellationToken), `diagnostics`,
and `execution_tracker`. It is **missing** three things every workflow engine needs:

1. **Artifact storage** — Skills produce outputs (artifacts). The context should
   track them so the execution summary can report what each skill produced.
2. **ExecutionMetadata** — Lifecycle timestamps (created_at, started_at, finished_at)
   are currently on `ExecutionMetadata` in `models/execution.rs` but the context
   doesn't hold a reference to it. The context should manage the lifecycle.
3. **EventBus** — Currently the engine holds the EventBus and emits events. But
   skills also need to emit events (e.g., ToolInvokedEvent). Putting an optional
   EventBus on the context lets any skill emit without an extra constructor param.
4. **Metrics counters** — Runtime counters: nodes_completed, nodes_failed,
   total_retries, elapsed_ms snapshots enable diagnostics without parsing events.

### Zoom-Out Check

**Purpose:** WorkflowContext is the single source of truth for a running workflow's
shared state — it's what every skill, the scheduler, the executor, and the engine
read and write during execution.

**Callers:**
- `WorkflowEngine::run()` — creates and mutates context
- `WorkflowExecutor::execute_node()` — reads cancellation, writes diagnostics
- `Skill<I>::execute()` — reads input/slots, writes slots/diagnostics
- `PlannerSkill` (runtime) — reads/writes plan output to slots

**Contracts:**
- `WorkflowContext<I>` is generic over `I: Send + Sync + Debug + 'static`
- Created via `WorkflowContext::new(input)` returning `(Self, CancellationSender)`
- Slots are type-erased `Arc<dyn Any + Send + Sync>` keyed by String
- CancellationToken is a `watch::Receiver<bool>` — thread-safe, no borrows
- All public methods are `&self` or `&mut self` — no `Arc<RwLock<>>` needed
- `ExecutionTracker` is `pub(crate)` — engine internals only

### Prior Art

- **Temporal** — WorkflowContext carries headers, memo, search attributes
- **Prefect** — Context holds task run ID, flow run ID, parameters, result storage
- **LangGraph** — State is the user-defined typed state; no engine-managed context
- **metalcraft** — Reducer trait defines state mutations; no separate context

## Implementation Steps

### Step 1: Add `artifacts: Vec<ArtifactRef>` to WorkflowContext

- Add `pub artifacts: Vec<ArtifactRef>` field
- Add `pub fn add_artifact(&mut self, artifact: ArtifactRef)` that pushes to vec
- Add `pub fn artifact_count(&self) -> usize`
- Export `ArtifactRef` from context module if needed

→ verify: `grep -q 'pub artifacts:' src/core/context/workflow_context.rs && grep -q 'pub fn add_artifact' src/core/context/workflow_context.rs`

### Step 2: Add `execution_metadata: ExecutionMetadata` field

- Add `pub execution_metadata: ExecutionMetadata` field
- In `WorkflowContext::new()`, create with `ExecutionMetadata::new(execution_id, workflow_id)`
- In `set_workflow_state(Completed|Failed|Cancelled)`, call `metadata.finish()`
- Add `pub fn mark_started(&mut self)` that calls `metadata.timestamps.mark_started()`

→ verify: `grep -q 'execution_metadata' src/core/context/workflow_context.rs && grep -q 'ExecutionMetadata' src/core/context/workflow_context.rs`

### Step 3: Add optional `event_bus: Option<EventBus>` field

- Add `pub event_bus: Option<EventBus>` field
- Update `WorkflowContext::new()` to optionally accept EventBus
- Add constructor `WorkflowContext::with_event_bus(input, bus)` or change `new()` signature
- Add `pub fn emit(&self, event: EngineEvent)` that forwards to event_bus if Some
- `EngineEvent` must be imported in context module

→ verify: `grep -q 'event_bus' src/core/context/workflow_context.rs && grep -q 'pub fn emit' src/core/context/workflow_context.rs`

### Step 4: Add metrics counters and snapshot

- Add `pub nodes_completed: u64`, `pub nodes_failed: u64`, `pub total_retries: u64` fields
- Add `pub fn metrics_snapshot(&self) -> MetricsSnapshot` returning a struct with all counters
- Define `MetricsSnapshot` struct with elapsed_ms, nodes_completed, nodes_failed, total_retries
- Increment counters from engine loop (engine sets them, context stores them)

→ verify: `grep -q 'nodes_completed' src/core/context/workflow_context.rs && grep -q 'MetricsSnapshot' src/core/context/workflow_context.rs`

### Step 5: Update module exports

- In `src/core/context/mod.rs`, add `ArtifactRef`, `ExecutionMetadata`, `MetricsSnapshot` to pub use if needed
- Ensure `EngineEvent` re-export is accessible

→ verify: `cargo check 2>&1 | grep -q 'Finished'`

### Step 6: Write tests

- Test: artifact storage — create context, add artifact, verify count
- Test: metadata lifecycle — create, mark_started, complete, verify timestamps set
- Test: event_bus — create context with bus, emit event, subscriber receives it
- Test: metrics — increment counters, snapshot returns correct values
- Test: backward compat — existing test patterns still compile and pass

→ verify: `cargo test -- workflow_context 2>&1 | tail -5 | grep -q 'test result: ok'`

## Verification Script (Step-by-Step)

1. Run `cargo check` — no compilation errors in modified modules
2. Run `cargo test` — all existing tests + new WorkflowContext tests pass
3. Run `cargo clippy` — no new warnings

## Out of Scope

- Wiring EventBus through the engine loop (that's e01s08)
- Adding metrics counters to the ExecutionSummary (that's e01s13)
- Persisting artifacts (that's Phase 5)
- Changing the WorkflowContext::new() public API signature (preserve backward compat)

## Risks

- Changing `WorkflowContext::new()` signature could break downstream callers
  → Mitigation: keep existing `new()` and add builder or `with_*` methods
- Artifact type constraints: Vec<ArtifactRef> uses Arc<Box<dyn Artifact>> which is
  already the established pattern; no risk

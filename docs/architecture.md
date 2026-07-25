# Architecture

The AI Workflow Engine is built in three layers: **Core** (engine internals), **Runtime** (ReAct loop), and **Tools** (skill implementations).

## Layer 1: Core Engine

The core is a generic, DAG-driven execution engine parameterized by an input type `I`.

### Key Types

| Type | Role |
| ------ | ------ |
| `Workflow<I>` | An immutable DAG definition — a named set of registered skills |
| `WorkflowBuilder<I>` | Builder pattern for constructing a `Workflow` |
| `WorkflowGraph<I>` | petgraph-backed DAG used at execution time |
| `WorkflowEngine<I>` | Top-level orchestrator — plans, schedules, executes |
| `WorkflowExecutor` | Runs individual skill nodes with timeout and retry |
| `WorkflowScheduler` | Ticks the DAG, identifies ready nodes |
| `WorkflowPlanner` | Builds an `ExecutionPlan` from a `SkillRegistry` |
| `WorkflowContext<I>` | Shared mutable state for a single run |
| `SkillRegistry<I>` | Stores registered skills, resolves dependencies |
| `Skill<I>` | Trait that all skills must implement |

### Execution Flow

```
1. WorkflowEngine::run(registry, ctx)
2.   WorkflowPlanner::plan(registry)  →  ExecutionPlan (DAG)
3.   LOOP:
4.     WorkflowScheduler::tick(graph) → ready_nodes
5.     execute_ready(ready_nodes)     → concurrent execution via JoinSet
6.     check cancellation / timeout
7.     if graph.complete → break
8.   Return WorkflowResult { summary }
```

### State Machine

Each node in the DAG follows:

```
Pending → Ready → Running → Completed
                         → Failed
                         → Cancelled
```

Transitions are managed by `WorkflowGraph::transition()`.

### Retry Engine

The `RetryPolicy` supports three back-off strategies:

| Strategy | Formula | Use Case |
|----------|---------|----------|
| `Linear` | `base_delay × attempt` | Predictable pacing |
| `Exponential` | `base_delay × 2ⁿ⁻¹` | Aggressive back-off |
| `Jitter` | exponential + fuzz | Thundering herd prevention |

Retries are enforced by `execute_with_retry()` — a standalone function used by both sequential (`run_with_policy`) and parallel (spawned task) code paths.

## Layer 2: ReAct Runtime

The runtime layer adds LLM-driven planning on top of the core engine.

### PlannerSkill

Implements the **Reason** half of ReAct:

```
PlannerSkill::execute()
  → Builds prompt from goal + available skills + prior results
  → Calls LlmService::prompt_typed::<PlanOutput>(prompt)
  → Stores PlanOutput in ctx slot "plan"
  → Engine reads PlanOutput.skills_to_run
  → Only those skills execute
  → Loop until PlanOutput.goal_complete = true
```

### PlanOutput (LLM-generated JSON)

```json
{
  "reasoning": "Need to search the web for current stock prices",
  "skills_to_run": ["web_search"],
  "goal_complete": false,
  "final_answer": null
}
```

### Event Bus

The `EventBus` is a broadcast channel that emits strongly typed `EngineEvent` variants:

- `WorkflowStarted`
- `ExecutionPlanBuilt`
- `NodeQueued`
- `SkillStarted`
- `SkillCompleted`
- `SkillFailed`
- `WorkflowCancelled`
- `WorkflowCompleted`
- `WorkflowFailed`

## Layer 3: Tool Skills

Tool skills implement `Skill<RuntimeInput>` and are registered in `RuntimePlugin`. Each tool:

1. Reads input from `ctx.input.goal` (or context slots)
2. Performs its action (HTTP call, file I/O, shell exec, etc.)
3. Writes output to `ctx.set(SKILL_ID, output)`
4. Returns `Ok(())` on success

### Safety

- **File tools** enforce CWD-restricted access with `resolve_safe_path()` / `resolve_safe_parent()`
- **Shell commands** have a configurable timeout (default 30s)
- **Calculator** uses a hand-written recursive-descent parser — no `eval()`

## Plugin System

Plugins bundle related skills and configuration:

```rust
pub trait Plugin<I>: Send + Sync {
    fn metadata(&self) -> &'static PluginMetadata;
    fn register(&self, ctx: &mut PluginContext<I>);
}
```

`RuntimePlugin` registers the `PlannerSkill` and all tool skills. Custom plugins can register additional skills for domain-specific workflows.

## Error Types

| Error | Source |
| ------- | -------- |
| `WorkflowError::Cancelled` | User-initiated cancellation |
| `WorkflowError::Timeout` | Workflow-level deadline exceeded |
| `WorkflowError::Deadlock` | No ready nodes but graph not complete |
| `WorkflowError::EmptyPlan` | No skills registered |
| `WorkflowError::SkillFailed` | A skill returned an error |
| `WorkflowError::EngineError` | Internal engine failure |

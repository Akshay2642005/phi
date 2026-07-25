# AI Workflow Engine — OpenCode

Read CONVENTIONS.md before any GitHub or git operation.

## Project

A generic, DAG-driven orchestration framework for building AI agent skills, with a ReAct runtime on top.
Stack: Rust 2024 edition, cargo, tokio

## Commands

| Action | Command |
| -------- | --------- |
| Run | `cargo run` |
| Test | `cargo test` |
| Build | `cargo build` |
| Lint | `cargo clippy` |
| Format | `cargo fmt` |

## Architecture

Entry point → AppContainer → Plugin/SkillRegistry → WorkflowEngine (planner, scheduler, executor).
ReAct runtime: PlannerSkill calls LLM, engine runs tool skills, loops until goal complete.

## Conventions

- Rust 2024 edition, single crate
- tokio async, serde Serialize/Deserialize on all data structs
- anyhow::Result for fallible, thiserror for error enums, async_trait for Skill<I>
- cargo fmt (default), cargo clippy (no custom config)
- Unit tests inline #[cfg(test)] mod tests { ... } in each module

## Never

- Do not directly edit .env
- Do not refactor outside task scope
- Do not add HTTP servers (src/api/ reserved)

## Agent Rules

- **Workflow Mandate:** You MUST use the bigpowers skills (e.g. `plan-work`, `develop-tdd`) to perform tasks.
- Read specs/ before writing code.
- All planning MUST be written to `specs/` before any code is generated.
- Write the minimum code that solves the stated problem.
- Run tests after every change. Show evidence before declaring done.

---

# AI Workflow Engine — AGENTS.md

**Generated:** 2026-07-04
**Commit:** (current working tree)
**Branch:** `main`

> **Note:** This project was originally a RAG vector search demo. It has been
> completely rewritten into a generic AI Workflow Engine — a DAG-driven
> orchestration framework for AI agent skills, with a ReAct runtime on top.
> The old `AGENTS.md` is obsolete.

---

## WHERE TO LOOK

| Task | Location | Notes |
| ------ | ---------- | ------- |
| Entry point / CLI | `src/main.rs` | Bootstraps `AppContainer`, starts tracing |
| Application container | `src/bootstrap/container.rs` | Constructs config, Rig runtime, `LlmServiceRef` |
| Config (env vars) | `src/setup/config.rs` | `OLLAMA_API_KEY`, `OLLAMA_BASE_URL`, `OLLAMA_MODEL` |
| Rig/Ollama runtime | `src/setup/rig.rs` | Wraps `rig::providers::ollama::Client` |
| **Workflow engine** | `src/core/workflow/` | Planner, scheduler, executor, DAG graph |
| **Workflow definition** | `src/core/workflow/definition.rs` | `Workflow<I>` — immutable workflow |
| **Workflow builder** | `src/core/workflow/builder.rs` | `Workflow::builder().name(…).register(…)…build()` |
| **Skill trait** | `src/core/skill/mod.rs` | `Skill<I>` — trait all skills implement |
| **Skill registry** | `src/core/skill/mod.rs` | `SkillRegistry<I>` — stores + topological sort |
| **LLM abstraction** | `src/core/llm/service.rs` | `LlmService` trait, `OllamaLlmService`, `LlmServiceExt` |
| **ReAct runtime** | `src/runtime/` | Planner skill + tool skills (`RuntimeInput`) |
| **Planner skill** | `src/runtime/planner.rs` | LLM decides which skills to run (ReAct Reason step) |
| **Runtime plugin** | `src/runtime/plugin.rs` | Registers `PlannerSkill` + tool skills |
| **Tool skills** | `src/tools/` | Currently: `web_search` (DuckDuckGo) |
| **Events system** | `src/core/events/` | `EventBus` (broadcast channel), strongly typed events |
| **Error types** | `src/core/errors/mod.rs` | `WorkflowError`, `SkillError`, `EngineError` |
| **ID types** | `src/core/ids.rs` | `WorkflowId`, `ExecutionId`, `NodeId`, etc. |
| **Capabilities** | `src/core/capability.rs` | Declarative requirement/provision contracts |
| **State machines** | `src/core/workflow/state.rs` | `WorkflowState`, `NodeState` enums |
| **Dependency resolver** | `src/core/workflow/dependency.rs` | Validates + builds DAG from `SkillRegistry` |
| **Example** | `examples/basic_workflow.rs` | Workflow definition API demo (no AI, no execution) |
| Tests | Inline `#[cfg(test)]` in each module | e.g. `definition.rs` has 20+ tests |

---

## CODE MAP

### Entry point & bootstrap

| Symbol | File | Role |
| -------- | ------ | ------ |
| `main()` | `src/main.rs` | Init tracing, load `.env`, build `AppContainer` |
| `AppContainer` | `src/bootstrap/container.rs` | Owns `AppConfig`, `RigRuntime`, `LlmServiceRef` |
| `AppConfig` | `src/setup/config.rs` | Reads `OLLAMA_*` env vars |
| `RigRuntime` | `src/setup/rig.rs` | Builds `ollama::Client` |

### Core engine — `src/core/`

| Layer | Key Types | File |
| ------- | ----------- | ------ |
| **Workflow definition** | `Workflow<I>`, `WorkflowBuilder<I>` | `workflow/definition.rs`, `workflow/builder.rs` |
| **Workflow metadata** | `WorkflowMetadata` | `workflow/metadata.rs` |
| **Workflow graph** | `WorkflowGraph<I>` (petgraph-backed DAG) | `workflow/graph.rs` |
| **Workflow planner** | `WorkflowPlanner`, `ExecutionPlan<I>` | `workflow/planner.rs` |
| **Workflow scheduler** | `WorkflowScheduler` | `workflow/scheduler.rs` |
| **Workflow executor** | `WorkflowExecutor` | `workflow/execution.rs` |
| **Workflow engine** | `WorkflowEngine<I>`, `WorkflowResult` | `workflow/engine.rs` |
| **Workflow node** | `WorkflowNode<I>`, `ExecutionState` | `workflow/node.rs` |
| **State machine** | `WorkflowState`, `NodeState` | `workflow/state.rs` |
| **Dependency resolver** | `DependencyResolver` | `workflow/dependency.rs` |
| **Policies** | `RetryPolicy`, `TimeoutPolicy` | `workflow/policy.rs` |
| **Skill trait + registry** | `Skill<I>`, `SkillRegistry<I>`, `SkillDefinition` | `skill/mod.rs` |
| **LLM service** | `LlmService`, `OllamaLlmService`, `LlmServiceExt` | `llm/service.rs` |
| **Plugin system** | `Plugin<I>`, `PluginContext<I>`, `PluginMetadata` | `plugin/mod.rs` |
| **Artifact system** | `Artifact`, `ArtifactRef`, `ArtifactMeta` | `artifact/mod.rs` |
| **Event bus** | `EventBus` (broadcast channel) | `events/bus.rs` |
| **Event types** | `EngineEvent` enum + 11 event structs | `events/event_types.rs` |
| **Error types** | `WorkflowError`, `SkillError`, `EngineError` | `errors/mod.rs` |
| **Strong IDs** | `WorkflowId`, `ExecutionId`, `NodeId`, etc. | `ids.rs` |
| **Capabilities** | `Capability` enum, `CapabilitySet` | `capability.rs` |
| **Models** | `ExecutionSummary`, `WorkflowDiagnostic`, `SkillOutcome` | `models/` |

### ReAct runtime — `src/runtime/`

| Symbol | File | Role |
| -------- | ------ | ------ |
| `RuntimeInput` | `runtime/mod.rs` | Domain input: `goal: String` + optional `context` |
| `PlannerSkill` | `runtime/planner.rs` | LLM-driven planner — decides which skills to run |
| `PlanOutput` | `runtime/planner.rs` | JSON output from planner: reasoning, skills, complete flag |
| `RuntimePlugin` | `runtime/plugin.rs` | Registers `PlannerSkill` + all tool skills |
| `WorkerPlugin` | `runtime/plugin.rs` | Alias for `RuntimePlugin` |

### Tools — `src/tools/`

| Symbol | File | Description |
|--------|------|-------------|
| `WebSearchSkill` | `tools/web_search.rs` | DuckDuckGo Instant Answer API search tool |

---

## PROJECT OVERVIEW

Rust-based AI Workflow Engine — a generic, DAG-driven orchestration framework
for building AI agent workflows. Skills (units of work) are registered into a
workflow, the engine builds a dependency graph, schedules ready nodes, and
executes them. A ReAct runtime sits on top, where an LLM planner decides which
tool skills to invoke to accomplish a user's goal.

```
                        ┌──────────────────┐
                        │   AppContainer   │
                        │  (config, llm)   │
                        └──────┬───────────┘
                               │
               ┌───────────────┴───────────────┐
               ▼                               ▼
     ┌──────────────────┐            ┌──────────────────────┐
     │  RuntimePlugin   │            │  (Future: domain     │
     │  (PlannerSkill   │            │   plugins)           │
     │   + tools)       │            └──────────────────────┘
     └────────┬─────────┘
              │ register
              ▼
     ┌─────────────────┐
     │  SkillRegistry  │──► WorkflowPlanner ──► WorkflowGraph (DAG)
     └─────────────────┘
                                │
                                ▼
                          WorkflowEngine
                          ┌──────────────────┐
                          │  Scheduler::tick │──► ready nodes
                          │  Executor::run   │──► execute skills
                          │  loop until done │
                          └──────────────────┘
```

### ReAct runtime loop

The `PlannerSkill` implements the **Reason** half of ReAct:

```
LOOP:
  1. PlannerSkill calls Ollama (via LlmService::prompt_typed<PlanOutput>)
     → LLM returns JSON: { reasoning, skills_to_run, goal_complete, final_answer }
  2. WorkflowEngine runs only the skills in skills_to_run
  3. Results stored in WorkflowContext.slots
  4. Repeat from step 1
  5. When goal_complete=true, the final_answer is returned
```

---

## KEY COMMANDS

```bash
cargo build                    # compile with debug profile
cargo build --release          # compile with optimizations
cargo run                      # build and run (requires OLLAMA_* env vars)
cargo test                     # run all inline tests (154 tests)
cargo fmt                      # format all Rust source files
cargo clippy                   # lint all code
cargo check                    # fast compilation check (no binary output)
cargo doc --open               # build and open crate docs
cargo clean                    # remove target/ directory
cargo run --example basic_workflow   # run the workflow definition example
```

No Makefile, no justfile, no pre-commit hooks.

---

## CONVENTIONS

- **Rust** edition 2024, `cargo` package manager
- **Single crate** in `Cargo.toml` (no workspace)
- **`cargo fmt`** for formatting (rustfmt defaults)
- **`cargo clippy`** for linting (no custom config)
- **`#[tokio::main]`** on async entry points
- **`serde::Serialize` / `serde::Deserialize`** for all data structs
- **`anyhow::Result`** for fallible functions (`Result<(), anyhow::Error>`)
- **`thiserror`** for structured error enums
- **`async_trait`** for all `Skill<I>` implementations
- **Imports organized** by std → external → crate (rustfmt default)
- **No pre-commit hooks**, no CI pipeline
- **No Docker**

---

## LLM CONFIGURATION

Configured via environment variables (loaded from `.env` by `dotenvy`):

| Variable | Default | Description |
| ---------- | --------- | ------------- |
| `OLLAMA_API_KEY` | _(required)_ | Ollama API key (`Nothing` for local) |
| `OLLAMA_BASE_URL` | _(required)_ | e.g. `http://localhost:11434` |
| `OLLAMA_MODEL` | _(required)_ | e.g. `llama3.2` or `qwen2.5` |

The LLM is wrapped in `OllamaLlmService` (in `core/llm/service.rs`),
which uses Rig's `client.agent(&model).build().prompt(...)` internally.
Skills access the LLM through the `LlmService` trait — provider-agnostic.

---

## INPUT DATA FORMAT

### Workflow input (`I` generic parameter)

The workflow engine is generic over any `I: Send + Sync + Debug + 'static`.
The ReAct runtime uses:

```rust
pub struct RuntimeInput {
    pub goal: String,
    pub context: Option<String>,
}
```

### Planner output (LLM-generated JSON)

```json
{
  "reasoning": "Need to search the web for current stock prices",
  "skills_to_run": ["web_search"],
  "goal_complete": false,
  "final_answer": null
}
```

---

## API ENDPOINTS

None — this is a CLI-only binary. No HTTP server. The `src/api/` module is
empty (future: could add `axum` or `actix-web`).

---

## ARCHITECTURE

```
src/
├── main.rs                    # Entry point: bootstrap container, init tracing
├── lib.rs                     # Public crate root: re-exports all modules
├── api/                       # (empty) future HTTP API
├── bootstrap/
│   ├── mod.rs
│   └── container.rs           # AppContainer: builds config + runtime + llm
├── core/
│   ├── mod.rs                 # Module map
│   ├── ids.rs                 # Strongly typed IDs (WorkflowId, ExecutionId, ...)
│   ├── capability.rs          # Capability enum + CapabilitySet
│   ├── artifact/
│   │   └── mod.rs             # Artifact trait, ArtifactRef, ArtifactMeta
│   ├── context/
│   │   ├── mod.rs
│   │   └── workflow_context.rs  # WorkflowContext<I> — shared run state
│   ├── errors/
│   │   └── mod.rs             # WorkflowError, SkillError, EngineError
│   ├── events/
│   │   ├── mod.rs
│   │   ├── bus.rs             # EventBus (broadcast channel)
│   │   └── event_types.rs     # 11 event structs + EngineEvent enum
│   ├── llm/
│   │   ├── mod.rs
│   │   └── service.rs         # LlmService trait, OllamaLlmService, LlmServiceExt
│   ├── models/
│   │   ├── mod.rs
│   │   ├── diagnostics.rs     # WorkflowDiagnostic, DiagnosticLevel
│   │   └── execution.rs       # ExecutionSummary, SkillOutcome, ExecutionMetadata
│   ├── plugin/
│   │   └── mod.rs             # Plugin<I> trait, PluginContext<I>
│   ├── skill/
│   │   └── mod.rs             # Skill<I>, SkillRegistry<I>, SkillDefinition
│   └── workflow/
│       ├── mod.rs
│       ├── builder.rs          # WorkflowBuilder<I> + WorkflowBuildError
│       ├── definition.rs       # Workflow<I> — immutable definition
│       ├── dependency.rs       # DependencyResolver
│       ├── engine.rs           # WorkflowEngine<I> — top-level orchestrator
│       ├── events.rs           # WorkflowEventSink adapter
│       ├── execution.rs        # WorkflowExecutor — runs individual skills
│       ├── graph.rs            # WorkflowGraph<I> — petgraph-backed DAG
│       ├── metadata.rs         # WorkflowMetadata
│       ├── node.rs             # WorkflowNode<I>, ExecutionState
│       ├── planner.rs          # WorkflowPlanner, ExecutionPlan<I>
│       ├── policy.rs           # RetryPolicy, TimeoutPolicy
│       ├── scheduler.rs        # WorkflowScheduler
│       └── state.rs            # WorkflowState, NodeState enums
├── runtime/
│   ├── mod.rs                  # RuntimeInput, WorkerInput
│   ├── planner.rs              # PlannerSkill — the ReAct "brain"
│   └── plugin.rs               # RuntimePlugin — registers planner + tools
├── setup/
│   ├── mod.rs
│   ├── config.rs               # AppConfig from env vars
│   └── rig.rs                  # RigRuntime wrapper
└── tools/
    ├── mod.rs
    └── web_search.rs           # WebSearchSkill (DuckDuckGo)
```

---

## DATA FLOW

```
1. main()
   ├── AppContainer::build()
   │   ├── AppConfig::from_env()  (read OLLAMA_* vars)
   │   ├── RigRuntime::new()      (build ollama::Client)
   │   └── OllamaLlmService::new() (wrap in LlmService trait)
   └── tracing::info!("AI Workflow Engine ready")

2. (Future: CLI or API triggers a workflow run)

3. RuntimePlugin::register()
   ├── PlannerSkill (LLM decides which skills to run)
   └── WebSearchSkill (tool skill)

4. WorkflowEngine::run()
   ├── WorkflowPlanner::plan()       → ExecutionPlan (DAG)
   ├── LOOP:
   │   ├── WorkflowScheduler::tick() → ready_nodes
   │   ├── WorkflowExecutor::execute_node()
   │   │   └── Skill::execute(ctx)   → Result<()>
   │   └── (check complete/deadlock)
   └── WorkflowResult { summary }
```

### ReAct detail

```
PlannerSkill::execute()
  → Build prompt: goal + available skills + prior results
  → llm.prompt_typed::<PlanOutput>(prompt)
  → Parse JSON: { reasoning, skills_to_run, goal_complete }
  → ctx.set("plan", output)

  If goal_complete=false:
    → Engine runs skills from skills_to_run
    → Planner runs again with updated context
  If goal_complete=true:
    → final_answer is returned
```

---

## TEST CONVENTIONS

- Unit tests: `#[cfg(test)] mod tests { ... }` inline in each source file
- No integration tests directory yet
- No mocking framework yet (skills tested with real LLM or stubs)
- Ollama integration tests: not yet gated behind a feature flag
- Current test count: **154 tests** across all modules, including `definition.rs`, `builder.rs`,
  `metadata.rs`, `graph.rs`, `policy.rs`, `execution.rs`, `engine.rs`,
  `runtime`, and `tools/`
  `metadata.rs`, `graph.rs`

---

## CLIPPY (LINTING)

- No custom `clippy.toml` — uses default clippy rules
- Run with: `cargo clippy`
- `#![allow(dead_code, unused)]` at crate root — engine framework has
  intentionally unused items until domain workflows are registered

---

## IMPORTANT QUIRKS & GOTCHAS

- **No execution path wired yet**: `main()` builds `AppContainer` and logs —
  but does not trigger any workflow run. The engine, planner, scheduler, and
  executor are fully implemented but no CLI command or API endpoint invokes
  them yet.
- **Ollama dependency**: Requires a running Ollama instance at
  `OLLAMA_BASE_URL` with the model specified in `OLLAMA_MODEL`.
- **`api_key(Nothing)`**: For local Ollama, use an empty string or any value
  as the API key. For cloud providers, use a real key.
- **`LlmService` abstraction**: Only `OllamaLlmService` is implemented.
  Switching to OpenAI/Groq/Azure requires implementing `LlmService` for a new
  provider.
- **`examples/basic_workflow.rs`**: Demonstrates the `Workflow::builder()` API
  only — no LLM calls, no execution, purely definition + inspection.
- **`RuntimePlugin` not wired into `main()`**: The ReAct runtime (`PlannerSkill`
  - `WebSearchSkill`) is defined but not connected to `AppContainer` yet.
- **Event bus plumbed but unused**: `EventBus` is implemented but `main()`
  does not attach it to the engine.
- **Single-file ReAct loop**: The `WorkflowEngine::run()` DAG loop is
  sequential (one skill at a time). Parallel execution is prepared for but
  not enabled.
- **Plugin system is a stub**: `Plugin<I>` trait exists and `RuntimePlugin`
  implements it, but there's no plugin loader, discovery, or dynamic linking.
- **`petgraph` dependency**: Used internally by `WorkflowGraph`. No other
  module imports it — it is fully encapsulated behind the `WorkflowGraph` API.

---

## TROUBLESHOOTING

| Problem | Likely cause | Fix |
| --------- | ------------- | ----- |
| `OLLAMA_API_KEY not set` | Missing `.env` file | Create `.env` with `OLLAMA_API_KEY=...`, `OLLAMA_BASE_URL=...`, `OLLAMA_MODEL=...` |
| `connection refused` | Ollama not running | Start `ollama serve` |
| `model not found` | Model not pulled | Run `ollama pull <model>` |
| Slow first build | Rust compiling deps | Use `cargo build` once, then `cargo run` reuses artifacts |
| `dead_code` warnings | Framework with no domain workflows yet | See `#![allow(dead_code, unused)]` in `main.rs` and `lib.rs` |

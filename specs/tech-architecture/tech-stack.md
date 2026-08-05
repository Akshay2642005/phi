# Tech Stack — AI Workflow Engine

## Stack

- **Language:** Rust (edition 2024)
- **Runtime:** tokio (async)
- **Package manager:** cargo (single crate, no workspace)
- **Build:** cargo build

## Key Dependencies

| Crate | Purpose |
| ------- | --------- |
| `serde` + `serde_json` | Serialization for all data structs, planner I/O |
| `tokio` | Async runtime (full features) |
| `petgraph` | DAG graph engine (WorkflowGraph) |
| `rig` | Ollama LLM client |
| `dashmap` | Concurrent map for WorkflowContext slots |
| `uuid` | Strong IDs (WorkflowId, ExecutionId, NodeId) |
| `clap` | CLI argument parsing |
| `reqwest` | HTTP client (web search tool) |
| `tracing` + `tracing-subscriber` | Structured logging |

## Architecture

```
main.rs → AppContainer → Plugin → SkillRegistry → WorkflowPlanner
                                                       ↓
                                              WorkflowGraph (DAG)
                                                       ↓
                                              WorkflowEngine
                                              ├── Scheduler::tick() → ready nodes
                                              └── Executor::run() → execute skills
```

### ReAct runtime (on top of engine)

```
PlannerSkill → LLM prompt → PlanOutput → Engine runs tool skills → loop until complete
```

## Modules

- `src/bootstrap/` — AppContainer wiring (config + runtime + LLM)
- `src/core/workflow/` — DAG engine (planner, scheduler, executor, graph, nodes, state machine)
- `src/core/skill/` — Skill<I> trait, SkillRegistry
- `src/core/llm/` — LlmService trait (provider-agnostic), OllamaLlmService
- `src/core/events/` — EventBus (broadcast channel), strongly typed events
- `src/core/errors/` — WorkflowError, SkillError, EngineError
- `src/core/context/` — WorkflowContext<I> shared run state
- `src/core/models/` — ExecutionSummary, SkillOutcome, WorkflowDiagnostic
- `src/runtime/` — ReAct loop: PlannerSkill, RuntimePlugin
- `src/tools/` — Tool skills: WebSearchSkill (Tavily)
- `src/setup/` — AppConfig from env vars, Rig runtime wrapper

## Key Design Decisions

- Strongly typed IDs (WorkflowId, ExecutionId, NodeId) via uuid + newtype pattern
- petgraph encapsulated behind WorkflowGraph API — no other module imports petgraph
- LlmService trait is provider-agnostic; only Ollama impl exists
- #![allow(dead_code, unused)] — intentional; framework items unused until domain workflows registered
- Inline #[cfg(test)] unit tests in each module (60+ tests)

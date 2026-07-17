# AI Workflow Engine — Claude Code

Read CONVENTIONS.md before any GitHub or git operation.

## Project
A generic, DAG-driven orchestration framework for building AI agent skills, with a ReAct runtime on top.
Stack: Rust 2024 edition, cargo, tokio

## Commands
| Action  | Command |
|---------|---------|
| Run     | `cargo run` |
| Test    | `cargo test` |
| Build   | `cargo build` |
| Lint    | `cargo clippy` |
| Format  | `cargo fmt` |
| Check   | `cargo check` |
| Doc     | `cargo doc --open` |
| Example | `cargo run --example basic_workflow` |

## Architecture
Entry point (`main.rs`) → `AppContainer` (config, Ollama LLM) → Plugin/SkillRegistry → `WorkflowEngine` (planner, scheduler, executor). ReAct runtime: `PlannerSkill` calls LLM, engine runs tool skills, loops until goal complete.

```
src/
├── main.rs              # Entry point
├── bootstrap/           # AppContainer, config, Rig/Ollama runtime
├── core/                # Engine: workflow, skill, LLM, events, errors, IDs
├── runtime/             # ReAct loop: PlannerSkill, RuntimePlugin
├── tools/               # Tool skills: web_search (DuckDuckGo)
├── setup/               # Config from env vars
├── api/                 # (empty) future HTTP API
└── examples/            # basic_workflow.rs
```

## Conventions
- Rust edition 2024, single crate in Cargo.toml (no workspace)
- `#[tokio::main]` async entry points
- `serde::Serialize` / `serde::Deserialize` on all data structs
- `anyhow::Result` for fallible functions; `thiserror` for error enums
- `async_trait` for all `Skill<I>` implementations
- Imports: std → external → crate (rustfmt default)
- `cargo fmt` (default rustfmt), `cargo clippy` (no custom config)
- Unit tests inline: `#[cfg(test)] mod tests { ... }` in each module
- `#![allow(dead_code, unused)]` at crate root — intentional for framework

## Never
- Do not directly edit `.env` — provide `.env.example` changes instead
- Do not touch `AGENTS.md` generated content without asking
- Do not refactor outside the task scope
- Do not add HTTP servers (`src/api/` is reserved for future)
- Do not bypass `#![allow(dead_code, unused)]` — it's intentional

## Agent Rules
- **Workflow Mandate:** You MUST use the bigpowers skills (e.g. `plan-work`, `develop-tdd`, `orchestrate-project`) to perform tasks. DO NOT write code directly in response to a user prompt like "build this feature".
- Read specs/ before writing code.
- All planning and specifications MUST be written to `specs/` before any code is generated.
- Write the minimum code that solves the stated problem. Nothing extra.
- Never refactor, rename, or reorganize code outside the task scope.
- Run tests after every change. Show evidence before declaring done.
- One clarifying question beats a wrong assumption baked into 200 lines.

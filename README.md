# AI Workflow Engine

[![Rust](https://img.shields.io/badge/Rust-2024-edition?logo=rust)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-154%20passing-brightgreen)](#)

A generic, DAG-driven orchestration framework for building AI agent **skills**, with a **ReAct** (Reason + Act) runtime on top. Written in Rust 2024 edition.

```bash
cargo run -- "what is the capital of France?"
# → Planner searches the web, extracts the answer, returns "Paris"
```

## Key Concepts

| Concept | Description |
| --------- | ------------- |
| **Skill** | A unit of work implementing `Skill<I>` — like a tool in an agent toolkit |
| **Workflow** | An immutable DAG of skills that defines execution order |
| **Engine** | The orchestrator — plans, schedules, and executes the DAG |
| **ReAct Runtime** | An LLM-driven loop that decides *which* skills to run to accomplish a goal |

## Quick Start

### 1. Start Ollama

```bash
ollama serve
ollama pull llama3.2  # or qwen2.5
```

### 2. Configure

Create a `.env` file:

```env
VAULT_ADDRESS=http://localhost:8200
VAULT_ROLE_ID=your-approle-role-id
VAULT_SECRET_ID=your-approle-secret-id
```

Or if using env vars directly (no Vault):

```env
OLLAMA_API_KEY=nothing
OLLAMA_BASE_URL=http://localhost:11434
OLLAMA_MODEL=llama3.2
```

### 3. Run

```bash
cargo run -- "calculate 2 + 2"
# → Planner runs CalculatorSkill, returns 4

cargo run -- "what is the weather in Tokyo?"
# → Planner searches the web, returns weather
```

## Examples

Define a custom workflow without the ReAct runtime:

```rust
use rust_agent::core::workflow::Workflow;

let workflow = Workflow::<()>::builder()
    .name("my-workflow")
    .version("1.0.0")
    .register(MyCustomSkill::new())
    .build()?;
```

The workflow definition API is demonstrated in [`examples/basic_workflow.rs`](examples/basic_workflow.rs).

## Architecture

```
main.rs → AppContainer → Plugin → SkillRegistry → WorkflowEngine
                                                       │
                                           ┌───────────┴───────────┐
                                           ▼                       ▼
                                    WorkflowPlanner         WorkflowScheduler
                                           │                       │
                                           ▼                       ▼
                                    WorkflowGraph (DAG)      WorkflowExecutor
                                                                (timeout,
                                                                 retries)
```

The **ReAct runtime** adds an LLM planner on top:

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│  PlannerSkill │────►│  Tool Skill  │────►│  PlannerSkill │
│  (Reason)     │     │  (Act)       │     │  (check done) │
└──────────────┘     └──────────────┘     └──────────────┘
        │                                        │
        └────────── loop until ──────────────────┘
                    goal_complete=true
```

## Tool Skills

| Skill | ID | Description |
| ------- | ---- | ------------- |
| Web Search | `web_search` | DuckDuckGo Instant Answers |
| File Read | `file_read` | Read files (CWD-restricted) |
| File Write | `file_write` | Write files (traversal-safe) |
| Shell Command | `shell_command` | Execute shell commands (timeout) |
| Calculator | `calculator` | Evaluate arithmetic expressions |

## Configuration

| Env Variable | Required | Default | Description |
| ------------- | ---------- | --------- | ------------- |
| `VAULT_ADDRESS` | Yes* | — | HashiCorp Vault address |
| `VAULT_ROLE_ID` | Yes* | — | Vault AppRole role ID |
| `VAULT_SECRET_ID` | Yes* | — | Vault AppRole secret ID |
| `VAULT_KV_MOUNT` | No | `secrets` | Vault KV v2 mount path |
| `VAULT_SECRET_PATH` | No | `rust-agent` | Vault secret path |
| `OLLAMA_API_KEY` | No** | `nothing` | Ollama API key |
| `OLLAMA_BASE_URL` | No** | — | Ollama server URL |
| `OLLAMA_MODEL` | No** | — | Model name |

\* Required when using Vault.  
\** Can be set in Vault at `secret/ollama` or directly in `.env` when not using Vault.

## Development

```bash
cargo build              # compile
cargo test               # run 154 tests
cargo clippy --lib       # lint (zero warnings)
cargo fmt                # format
cargo doc --open         # build docs
```

### Windows note

Git's `/usr/bin/link.exe` can interfere with MSVC's `link.exe`. Rename it during builds:

```bash
mv "/c/Program Files/Git/usr/bin/link.exe" link.exe.bak
cargo build
mv link.exe.bak "/c/Program Files/Git/usr/bin/link.exe"
```

## License

MIT

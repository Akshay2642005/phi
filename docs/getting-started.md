# Getting Started

This guide walks through setting up the AI Workflow Engine from scratch — from installing dependencies to running your first ReAct workflow.

## Prerequisites

- **Rust** (edition 2024, toolchain via rustup)
- **Ollama** (for LLM calls — `ollama serve` must be running)
- **HashiCorp Vault** (optional — for secrets management)
- **Git Bash** or equivalent Unix-like shell on Windows

## Step 1: Install Ollama

```bash
# macOS / Linux
curl -fsSL https://ollama.com/install.sh | sh

# Windows
# Download from https://ollama.com/download
```

Start Ollama and pull a model:

```bash
ollama serve        # start the server (background)
ollama pull llama3.2  # or qwen2.5, mistral, etc.
```

## Step 2: Configure Secrets

### Option A: HashiCorp Vault (recommended)

1. Start Vault in dev mode:

   ```bash
   vault server -dev
   ```

2. Create a `.env` file:

   ```env
   VAULT_ADDRESS=http://localhost:8200
   VAULT_ROLE_ID=<your-approle-role-id>
   VAULT_SECRET_ID=<your-approle-secret-id>
   ```

3. Store Ollama secrets in Vault:

   ```bash
   vault kv put secret/rust-agent \
     OLLAMA_API_KEY=nothing \
     OLLAMA_BASE_URL=http://localhost:11434 \
     OLLAMA_MODEL=llama3.2
   ```

### Option B: Direct environment variables

Create a `.env` file:

```env
OLLAMA_API_KEY=nothing
OLLAMA_BASE_URL=http://localhost:11434
OLLAMA_MODEL=llama3.2
```

> **Note:** `OLLAMA_API_KEY` can be any value for local Ollama. For cloud providers, use a real API key.

## Step 3: Build and Test

```bash
cargo build              # compile the project
cargo test               # run 154 tests
cargo clippy --lib       # lint — should be zero warnings
```

## Step 4: Run Your First ReAct Workflow

```bash
# Simple calculation
cargo run -- "calculate 2 + 2"
# → 4

# Web search
cargo run -- "what is the capital of France?"
# → Paris

# File operations
cargo run -- "read Cargo.toml"
# → [package]
# → name = "rust-agent"
# → ...

# Shell commands
cargo run -- "run echo hello world"
# → hello world
```

## Step 5: Explore the Workflow Example

```bash
cargo run --example basic_workflow
```

This demonstrates the workflow definition API without executing anything — purely building and inspecting a workflow graph.

## Troubleshooting

### "OLLAMA_API_KEY not set"

Your `.env` file is missing or not loaded. Create one with the required variables.

### "connection refused: localhost:11434"

Ollama is not running. Start `ollama serve` in another terminal.

### "model not found"

Run `ollama pull <model-name>` to download the model specified in your config.

### Git `link.exe` interferes with MSVC (Windows)

```bash
mv "/c/Program Files/Git/usr/bin/link.exe" link.exe.bak
cargo build
mv link.exe.bak "/c/Program Files/Git/usr/bin/link.exe"
```

### Dead code warnings

These are expected — `#![allow(dead_code)]` is set at the crate root. Framework items may be unused until domain-specific workflows are registered.

## Next Steps

- [Architecture overview](architecture.md) — how the engine works
- [Skill development guide](skill-development.md) — creating custom skills
- Read `AGENTS.md` for the full code map
- Browse `src/` for implementation details

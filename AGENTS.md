# RAG Vector Agent — AGENTS.md

**Generated:** 2026-06-26
**Commit:** `82d81d8`
**Branch:** `main`

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Entry point / CLI | `src/main.rs` | `#[tokio::main]` async entry, rig vector search demo |
| Vector search logic | `src/main.rs` | Embed + InMemoryVectorStore + top-n search |
| LLM / embedding config | `src/main.rs` | Ollama client builder: `nomic-embed-text` model |
| Embedding model | `rig` crate | `EmbeddingsBuilder::documents()` + `.build().await?` |
| Vector store | `rig::vector_store::in_memory_store::InMemoryVectorStore` | In-memory store with ID-based lookup |
| Document types | `src/main.rs` | `WordDefinition` struct with `#[derive(Embed)]` |
| Dependencies | `Cargo.toml` | `rig`, `tokio`, `serde`, `anyhow` |
| Lockfile | `Cargo.lock` | Pinned dependency tree |
| Tests | `src/` (not yet present) | Future: `#[cfg(test)] mod tests` |
| Sample data | `src/main.rs` | `sample_documents()` → 3 `WordDefinition` entries |
| Output | stdout | CLI prints top-n matches with scores |

## CODE MAP

| Symbol | Type | Location | Role |
|--------|------|----------|------|
| `main()` | async fn | `src/main.rs` | Entry point: build Ollama client, embed docs, search |
| `WordDefinition` | struct | `src/main.rs` | `#[derive(Embed)]` document with `id`, `word`, `definitions` |
| `sample_documents()` | fn | `src/main.rs` | Returns `Vec<WordDefinition>` of 3 sample entries |
| `print_matches()` | fn | `src/main.rs` | Pretty-print `(score, id, word)` tuples |
| `print_id_matches()` | fn | `src/main.rs` | Pretty-print `(score, id)` tuples |
| `SearchMatch` | type alias | `src/main.rs` | `(f64, String, String)` for ranked results |
| `EmbeddingsBuilder` | struct | `rig::embeddings` | Builds embeddings from documents |
| `InMemoryVectorStore` | struct | `rig::vector_store::in_memory_store` | In-memory vector index |
| `VectorSearchRequest` | struct | `rig::vector_store::request` | Query builder with `.samples(n)` |
| `VectorStoreIndex` | trait | `rig::vector_store` | Index trait with `top_n()` and `top_n_ids()` |
| `ollama::Client` | struct | `rig::providers::ollama` | Ollama API client |
| `ollama::ClientBuilder` | struct | `rig::providers::ollama` | Builder with `.api_key()`, `.base_url()` |

## CONVENTIONS

- **Rust** edition 2024, `cargo` package manager
- **No workspace** — single crate in `Cargo.toml`
- **`cargo fmt`** for formatting (rustfmt defaults)
- **`cargo clippy`** for linting (no custom config)
- **`#[tokio::main]`** on all async entry points
- **`serde::Serialize` / `serde::Deserialize`** for all data structs
- **`#[derive(Embed)]`** from rig for vector-embedding fields
- **`anyhow::Result`** for fallible functions (`Result<(), anyhow::Error>`)
- **Imports organized** by std → external → crate (rustfmt default)
- **No pre-commit hooks**, no CI pipeline
- **No Docker** — deployed via `cargo run`
- **Ollama local** required at `http://localhost:11434` with `nomic-embed-text` model

## ANTI-PATTERNS (THIS PROJECT)

- **Everything in `main.rs`**: All logic is in a single file — no module separation yet
- **No tests**: Zero `#[cfg(test)]` modules, no integration tests
- **No error handling beyond `anyhow`**: All `?` propagates up; no structured error types
- **Hardcoded sample data**: `sample_documents()` returns static data; no file/DB ingestion
- **No config file**: Ollama URL and model name are hardcoded strings
- **No CLI args**: No `clap` or `structopt` — embedding query is hardcoded
- **No Persistence**: `InMemoryVectorStore` is ephemeral — everything lost on restart
- **Single provider**: Only Ollama supported; no OpenAI / Groq / Azure abstraction
- **`Embed` derive macro on `Vec<String>`**: `definitions` field is a `Vec<String>` marked `#[embed]`, which embeds the whole vector at once

## Project overview

Rust-based RAG (Retrieval-Augmented Generation) agent that ingests documents, embeds them via a local Ollama instance, and performs semantic vector search. Built with the `rig` crate (Rust LLM framework). Output is printed to stdout.

## Package manager & Rust

- **cargo** (required). Never use other Rust package managers.
- Rust **edition 2024** via `Cargo.toml`.
- Dependencies in `Cargo.toml`, lockfile at `Cargo.lock`.

## Key commands

```bash
cargo build                    # compile with debug profile
cargo build --release          # compile with optimizations
cargo run                      # build and run the agent
cargo test                     # run all tests (none yet)
cargo fmt                      # format all Rust source files
cargo clippy                   # lint all code
cargo check                    # fast compilation check (no binary output)
cargo doc --open               # build and open crate docs
cargo clean                    # remove target/ directory
```

No Makefile, no justfile, no pre-commit hooks.

## Architecture

```
src/main.rs                    # Entry point: Ollama client → embeddings → vector search
```

### RAG pipeline (linear, 4 steps)

```
Ollama client setup
  → EmbeddingsBuilder::documents(sample_documents())
  → InMemoryVectorStore::from_documents_with_id_f()
  → index.top_n(query) / index.top_n_ids(query)
  → Print results to stdout
```

Each step is a `?`-propagated fallible call in an async `main()`. No LangGraph — the pipeline is a simple sequential Rust program.

## LLM configuration

The pipeline uses the `rig` crate with an Ollama provider. Configured inline in `src/main.rs`:

```rust
let client = ollama::Client::builder()
    .api_key(Nothing)
    .base_url("http://localhost:11434")
    .build()?;
```

Ollama must be running locally with the `nomic-embed-text` model pulled:

```bash
ollama pull nomic-embed-text
```

**Key**: `rig` abstracts providers behind a common trait — switching to OpenAI/Groq/Azure requires changing the provider module and `base_url`.

## Pipeline entry points

| Symbol | Source | Description |
|---|---|---|
| `main()` | `src/main.rs` | Builds Ollama client, creates embeddings, runs vector search, prints results |

Only one entry point. Output is printed to stdout (no file written to disk).

## API endpoints

None — this is a CLI-only binary. No HTTP server. Future work could add an `axum` or `actix-web` API layer.

## Data flow

```
sample_documents() → Vec<WordDefinition>
  → EmbeddingsBuilder::documents() → build().await → VectorStore
  → VectorStoreIndex::top_n(query) → Vec<(f64, String, WordDefinition)>
  → print_matches() → stdout
```

## Input data format (sample documents)

```rust
#[derive(Embed, Clone, Deserialize, Debug, Serialize, Eq, PartialEq, Default)]
struct WordDefinition {
    id: String,
    word: String,
    #[embed]
    definitions: Vec<String>,
}
```

Sample data is hardcoded in `sample_documents()` as a `Vec<WordDefinition>` with 3 entries covering fictional words (`flurbo`, `glarb-glarb`, `linglingdong`).

## Database extraction

Not applicable — the project does not perform database extraction. All data is in-memory samples. Future work could add SQLite/PostgreSQL extraction via `sqlx` or `diesel`.

## Important quirks & gotchas

- **Single-file project**: All code lives in `src/main.rs`. Splitting into modules (`src/vector_store.rs`, `src/models.rs`) is a natural next step.
- **Hardcoded query**: The search query `"I need to buy something in a fictional universe. What type of money can I use for this?"` is a string literal. No CLI argument parsing.
- **In-memory only**: `InMemoryVectorStore` drops all data on exit. No persistence to disk or database.
- **Ollama dependency**: Requires a running Ollama instance at `http://localhost:11434`. No graceful fallback if unavailable.
- **`Nothing` API key**: `ollama::Client::builder().api_key(Nothing)` — the `Nothing` type signals no API key for Ollama's local mode. For cloud providers, use a real key.
- **`nomic-embed-text` model**: Small, fast embedding model (137M parameters). Works well for short text. For production, consider `text-embedding-3-large` or `bge-m3`.
- **Vector search returns `AnyhowError`**: `top_n()` and `top_n_ids()` return `Result<_, anyhow::Error>`, which propagates to `main()`'s `Result<(), anyhow::Error>`.
- **`#[embed]` on `Vec<String>`**: The `Embed` derive macro flattens the `Vec<String>` into a single embedding vector. Each `WordDefinition` produces one embedding, not one per definition string.

## Test conventions

- No tests exist yet. Future convention:
  - Unit tests: `#[cfg(test)] mod tests { ... }` in each source file
  - Integration tests: `tests/` directory for e2e scenarios
  - Mocking: Use `mockall` for trait mocking or `wiremock` for HTTP-level testing
  - Ollama integration tests: `#[cfg(feature = "integration")]` gated behind Cargo feature flag

## Clippy (linting)

- No custom `clippy.toml` — uses default clippy rules
- Run with: `cargo clippy`
- Common warnings to expect: `too_many_arguments`, `type_complexity`, `large_enum_variant`
- All clippy warnings are currently allowed (no `#![deny(clippy::all)]` in crate root)

## Troubleshooting

| Problem | Likely cause | Fix |
|---------|-------------|-----|
| `connection refused` | Ollama not running | Start `ollama serve` or `ollama pull nomic-embed-text` |
| `model not found` | `nomic-embed-text` not pulled | Run `ollama pull nomic-embed-text` |
| `api_key` error | Using cloud provider with no key | Set `OPENAI_API_KEY` env var or use `.api_key("ollama")` |
| Slow first run | Rust compiling dependencies | Use `cargo build` once, then `cargo run` reuses artifacts |

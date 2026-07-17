# Test Plan — AI Workflow Engine

## Test Strategy

- **Unit tests:** Inline `#[cfg(test)]` in each module — primary test layer
- **Integration tests:** Not yet set up (no `tests/` directory)
- **Mocking:** No framework yet; skills tested with real LLM or stubs

## Running Tests

```bash
cargo test                    # all tests
cargo test <module>           # specific module
```

## Coverage Targets

- Each public function: at least one test
- Each error variant: error-path test
- Bug fixes: regression test

## Current Test Count

60+ tests across `definition.rs`, `builder.rs`, `metadata.rs`, `graph.rs`

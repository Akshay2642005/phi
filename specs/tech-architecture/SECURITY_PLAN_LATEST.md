# Security Plan — AI Workflow Engine

## Assumptions

- Ollama runs locally or on a trusted network (OLLAMA_BASE_URL env var)
- No HTTP server — CLI-only binary
- No user authentication or multi-tenancy in scope

## Risks

| Risk | Mitigation |
| ------ | ------------ |
| API key exposure | OLLAMA_API_KEY in .env (not committed) |
| Prompt injection (LLM) | Planner output is JSON-schema constrained; skill_names validated against registry |
| ReDoS in parsing | serde_json deserialization is bounded |

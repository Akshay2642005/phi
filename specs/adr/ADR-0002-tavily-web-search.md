# ADR-0002: Tavily as the web search provider

- **Status:** Accepted
- **Date:** 2026-08-05
- **Decided in:** working-tree draft on `master`; lands via e04 feature branch

## Context

`WebSearchSkill` calls the DuckDuckGo Instant Answer API — a free, no-auth
endpoint built for keyword lookups, not conversational queries. Two failures
compound:

1. **Thin signal for the ReAct planner.** Instant Answers return a short
   Wikipedia-backed abstract or nothing. Conversational queries ("interesting
   fact about the number 42") return zero results, so the planner searches,
   finds nothing, and loops (see `BUG-20260725T1619` — weak search signal
   feeds planner repetition).
2. **No content for reasoning.** Even when a result exists, the snippet is
   too short for the LLM to reason over; there is no AI-generated answer.

Options considered: keep DuckDuckGo, Brave Search API, SerpAPI, Tavily.

## Decision

Adopt the **Tavily Search API** (`POST https://api.tavily.com/search`) as the
web search provider:

- **Payload:** `{ api_key, query, search_depth: "basic", include_answer: true,
  max_results: 5 }`.
- **Response:** `answer` (AI-generated, when `include_answer` is true) plus
  `results[]` with `title`, `url`, `content` — full-text snippets the planner
  can reason over.
- **Key plumbing:** `TAVILY_API_KEY` env var → `AppConfig.tavily_api_key`
  (optional at startup, matching graceful-degradation conventions; enforced at
  search time with a clear bail message). Vault-backed `OllamaSecrets` gains
  the field with a serde default. `RuntimePlugin` constructor passes the key
  to `WebSearchSkill`.
- **Version:** `SkillDefinition` bumps to `2.0.0` (behavior change, interface
  unchanged).

Why Tavily over the alternatives:

- **DuckDuckGo (rejected):** free but produces thin/empty answers for
  conversational queries — the exact failure this ADR fixes.
- **Brave / SerpAPI (deferred):** viable but require more setup (multiple env
  vars, SDKs) and offer no AI-answer field; revisit if Tavily caps become a
  problem.
- **Tavily (chosen):** free tier (1,000 credits/month, 1 per basic search) is
  ample for this project's usage; designed for AI agents; returns
  answer + content in one JSON; single API key.

## Consequences

- **Positive:** the planner gets real content (`answer` + snippets) to reason
  over; search works with conversational queries; one env var.
- **Negative:** requires an external API key; free-tier cap (1,000
  searches/month) could be hit with heavy use — acceptable for this project,
  revisit at ~800/month.
- **Trade-off accepted:** no retry/cache layer added here — the core engine's
  existing `RetryPolicy`/`TimeoutPolicy` covers transient failures.

## References

- `specs/epics/e04-tavily-search/` — epic capsule
- `specs/bugs/BUG-20260725T1619-infinite-react-loop.md` — motivating failure
- `docs/getting-started.md` §config reference

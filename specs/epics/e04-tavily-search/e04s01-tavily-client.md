### Story e04s01: WebSearchSkill — Tavily Search API Client

**type:** feat
**risk:** P0 (external API integration)
**context:** domain
**BCPs:** 3

**Context:** `WebSearchSkill` (src/tools/web_search.rs) called the DuckDuckGo
Instant Answer API, which returns thin snippets and fails on conversational
queries. This story swaps the provider to Tavily: `POST
https://api.tavily.com/search` with `{ api_key, query, search_depth: "basic",
include_answer: true, max_results: 5 }`, parsing the `answer` field plus
`results[]` into the existing `WebSearchOutput` shape. SkillDefinition bumps
to `2.0.0`. A draft of this implementation already sits in the working tree;
the steps below verify it and fix any gaps.

## Steps

1. Verify the Tavily client: POST /search with JSON body, HTTP status checked,
   response parsed into `TavilyResponse { answer, results[] }` mapped to
   `WebSearchOutput` (answer or top result as abstract; rest as related, take 5)
   → verify: `grep -q 'api.tavily.com/search' src/tools/web_search.rs && grep -q 'include_answer' src/tools/web_search.rs && echo OK || echo MISSING`

2. Verify the no-results path: when `results` is empty but Tavily returned an
   `answer`, output still carries the answer with source "Tavily"
   → verify: `grep -q 'No organic results' src/tools/web_search.rs && echo OK || echo MISSING`

3. Verify HTTP error handling: non-success status bails with the status code
   and body text (no silent empty results)
   → verify: `grep -q 'Tavily API returned HTTP' src/tools/web_search.rs && echo OK || echo MISSING`

4. Verify empty-key guard: `execute` bails with a clear actionable message when
   the API key is empty — no panic, no request
   → verify: `grep -q 'TAVILY_API_KEY not set' src/tools/web_search.rs && echo OK || echo MISSING`

5. Verify SkillDefinition version is 2.0.0 and description names Tavily
   → verify: `grep -q 'version: \"2.0.0\"' src/tools/web_search.rs && grep -q 'Tavily' src/tools/web_search.rs && echo OK || echo MISSING`

6. Verify module docs describe the Tavily contract (endpoint, payload, extraction)
   → verify: `grep -q 'Tavily API' src/tools/web_search.rs && echo OK || echo MISSING`

## Verification Script (Step-by-Step)

1. `cargo test --lib` — all 161 tests still pass with the new client compiled in.
2. `TAVILY_API_KEY= cargo run -- "test"` (unset key) — expect the clear
   "TAVILY_API_KEY not set" error, not a panic.
3. With a real key: `cargo run -- "Eiffel Tower height"` — expect a search
   result with content text in the final answer.

## Out of scope

- Crawl-depth searches, custom max_results, filters (stays basic depth)
- Response caching or rate-limit handling (covered by core retry policy)

## Risks

- Tavily API contract drift (field renames) — low; guarded by serde `default`s
  and the HTTP-status bail.
- Live verification needs a real API key; unit-level parsing tests must not
  hit the network.

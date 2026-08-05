### Story e04s04: Regression Tests — Tavily Parsing, Error Paths, Empty-Key Bail

**type:** test
**risk:** P1 (core feature logic coverage)
**context:** domain
**BCPs:** 1

**Context:** The Tavily rewrite removed the DuckDuckGo code and the file has no
`#[cfg(test)]` module at all (0 tests match `web_search` today). This story adds
offline unit tests — no network, no real API key — covering: response parsing
(answer + results), the no-results path, empty-key bail, and HTTP error
mapping. Tests exercise the pure logic via a refactored helper if needed; the
`reqwest::Client` stays out of tests.

## Steps

1. Extract (or keep) a pure parsing function for the Tavily JSON → `WebSearchOutput`
   mapping so tests can feed fixture JSON without a network client
   → verify: `cargo test -- web_search 2>&1 | grep -E '^(test result|running)' | grep -q 'running 0' && echo NO-TESTS-YET || echo HAS-TESTS`

2. RED: write tests for answer+results parsing, no-results-with-answer, empty
   results, and JSON parse failure — assert exact `WebSearchOutput` fields
   → verify: `grep -q '#\[test\]' src/tools/web_search.rs && echo OK || echo MISSING`

3. GREEN: implement/refactor so the tests pass (pure helper or test-only
   constructor on `WebSearchSkill`)
   → verify: `cargo test -- web_search 2>&1 | grep 'test result' | grep -q '0 failed' && echo OK || echo FAIL`

4. Add a test asserting the empty-key guard: calling execute with an empty key
   returns an error mentioning TAVILY_API_KEY before any request
   → verify: `cargo test -- web_search 2>&1 | grep 'test result' | grep -q '0 failed' && echo OK || echo FAIL`

5. Full suite green + clippy clean
   → verify: `cargo test 2>&1 | grep 'test result' | awk -F'[ ;]' '{p+=$4; f+=$6} END {print "passed:"p" failed:"f}' && cargo clippy --all-targets 2>&1 | grep -cE '^(warning|error)' | awk '{if($1==0) print "clippy OK"; else print "clippy issues: "$1}'`

## Verification Script (Step-by-Step)

1. `cargo test -- web_search` — parsing + guard tests run and pass offline.
2. `cargo test` — total count is 161 + new tests, 0 failed.
3. `cargo clippy --all-targets` — no warnings.

## Out of scope

- Live integration test against api.tavily.com (requires a real key + network;
  documented in e04s01 verification instead)
- Mocking the HTTP layer with a fake reqwest client — parsing is tested via
  pure functions, so no HTTP mocking framework is introduced

## Risks

- Refactoring the mapping for testability changes the execute path — keep the
  refactor minimal (extract pure fn, call from execute).

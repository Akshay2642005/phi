### Story e04s03: Docs & Env Example — TAVILY_API_KEY Everywhere

**type:** docs
**risk:** P3 (zero behavioral change)
**context:** domain
**BCPs:** 2

**Context:** Every place that tells a user how to configure or run the engine
must name Tavily, not DuckDuckGo. Docs edits are already in the tree for
AGENTS.md, CLAUDE.md, CONVENTIONS.md, README.md, and
specs/tech-architecture/tech-stack.md. Two gaps remain: `.env.example` has no
`TAVILY_API_KEY` entry, and `docs/getting-started.md` (env var reference +
setup section) does not document it.

## Steps

1. Add `TAVILY_API_KEY` to `.env.example` with a comment pointing at
   <https://tavily.com> (free tier, 1,000 credits/month)
   → verify: `grep -q 'TAVILY_API_KEY' .env.example && echo OK || echo MISSING`

2. Add `TAVILY_API_KEY` to the env-var reference in `docs/getting-started.md`
   (alongside the OLLAMA_* vars) with a note that it is required for
   `web_search` and optional otherwise
   → verify: `grep -q 'TAVILY_API_KEY' docs/getting-started.md && echo OK || echo MISSING`

3. Verify no DuckDuckGo references remain in src/, docs/, *.md docs, .env.example
   → verify: `grep -rni 'duckduckgo' src/ docs/ README.md AGENTS.md CLAUDE.md CONVENTIONS.md .env.example specs/ 2>/dev/null | wc -l | awk '{if($1==0) print OK; else print "LEFTOVERS: "$1}'`

## Verification Script (Step-by-Step)

1. `grep -n TAVILY_API_KEY .env.example docs/getting-started.md` — both present.
2. `grep -rni duckduckgo src/ docs/ README.md .env.example specs/` — zero results.

## Out of scope

- Rewriting tutorial prose beyond the env-var references
- docs/architecture.md (already documents the ReAct runtime; no provider detail)

## Risks

- Stale references hidden in generated files (e.g. cargo doc comments) — the
  grep sweep in step 3 covers tracked sources.

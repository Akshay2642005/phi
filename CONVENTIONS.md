# AI Workflow Engine — Conventions

## Conventional Commits & Semantic Versioning

All changes to this repository MUST follow the [Conventional Commits 1.0.0](https://www.conventionalcommits.org/en/v1.0.0/) specification. Versioning MUST strictly adhere to [Semantic Versioning 2.0.0](https://semver.org/).

### Commit Message Format

`<type>(<scope>): <description>` (Space after colon is MANDATORY)

### Types & Version Bumps

- `feat`: Minor (x.Y.z) — New feature
- `fix`: Patch (x.y.Z) — Bug fix
- `perf`: Patch (x.y.Z) — Performance improvement
- `docs`, `chore`, `style`, `refactor`, `test`: No bump (unless breaking)
- `BREAKING CHANGE:` (or `!` after type): Major (X.y.z)

## GitHub & Git Operations

- No direct work on `main` or `master`. Every task MUST start with a feature branch or worktree via `kickoff-branch`.
- **Integrate (team default):** Use `gh pr create` and `gh pr merge --squash` via `release-branch` (team-pr mode). Prefer `gh` over ad-hoc `git push` + manual PR UI.
- **Integrate (solo profile):** When `profiles/solo-git.md` or `specs/WORKFLOW-solo-git.md` is active, ship with `bash scripts/land-branch.sh <branch> "<conventional message>"` after `release-branch` gates.
- `git push origin <feature-branch>` is allowed for backup or CI; never push directly to `main`/`master`.
- Use `gh repo clone` not `git clone` for GitHub repos.
- Use `gh run view` / `gh run watch` for CI status.
- Verify auth with `gh auth status` before operations.
- **Git Attribution:** NEVER include `Co-authored-by`, `Co-Authored-By`, or any other footer that attributes code to an AI agent. All commits must appear as if authored solely by the human user.
- Never call GitHub REST API directly (curl, fetch, etc.).
- Never create GitHub issues from automated workflows — produce local .md files in specs/ instead.

## Agent Workflow Mandates

**AGENTS MUST NEVER BYPASS THE BIGPOWERS WORKFLOW.**
You are operating within the `bigpowers` spec-driven development methodology.

- **No Direct Coding:** When a user issues a directive like "build feature X" or "go epic 10", you MUST NOT execute the request by writing code directly.
- **Required Skills:** You MUST route all work through the appropriate bigpowers skills.
  - Start with `survey-context` if you lack context.
  - Use `plan-work` to flesh out tasks in `specs/epics/` (with `verify:` per task) before writing any feature code.
  - Use `develop-tdd` or `execute-plan` to implement the plan.
  - Use `investigate-bug` for bug reports before writing a fix.
- **Verification Mandate:** Every story implementation MUST end with a step-by-step manual verification script provided to the user. You must wait for the user to confirm behavioral correctness (UAT) before declaring the story done.
- **Verification:** You MUST verify every change with tests. Code generation without a corresponding plan in `specs/` is strictly forbidden.
- **Traceability Mandate:** Every story MUST have at least one `story: eNNsNN` tag in its implementing code or test file.
- **Stream Continuity:** When writing large files or long documents, you MUST output continuously in chunks of ~200 lines. Do not pause between sections.

## specs/ — All Planning Output Goes Here

Every skill that produces written output writes to `specs/` at the project root.

### YAML cockpit (runtime + delivery)

| Layer | File | Answers |
| ------- | ------ | --------- |
| Session | `specs/state.yaml` | Active flow, epic/bug, git, `handoff.next_skill` |
| Release index | `specs/release-plan.yaml` | Target semver, WSJF epic list |
| Progress | `specs/execution-status.yaml` | Flat status keys — sole SoT for story state |
| Planning UI | `specs/planning-status.yaml` | Discover-phase workflow checklist (optional) |

### Intent vs delivery vs execution

| Question | File | Format |
| ---------- | ------ | -------- |
| What should the product do? | `specs/product/SCOPE_LATEST.yaml` | YAML |
| North star / initiative | `specs/product/VISION_LATEST.yaml` | YAML |
| Glossary | `specs/product/GLOSSARY_LATEST.yaml` | YAML |
| What ships in this release? | `specs/release-plan.yaml` | YAML |
| How to implement an epic/story? | `specs/epics/eNN-*.yaml` or `specs/epics/eNN-*/stories/` | YAML + MD |
| Where are we in the session? | `specs/state.yaml` | YAML |

Epic IDs: `e01`, `e30`. Story IDs: `e01s01`.

### Guardrails and other artifacts

| Document | Path |
| ---------- | ------ |
| Stack / architecture | `specs/tech-architecture/tech-stack.md` |
| Security / test / design plans | `specs/tech-architecture/*_PLAN_LATEST.md` |
| Domain context + ADRs | `specs/tech-architecture/tech-stack.md` + `specs/adr/` |
| Bug investigation | `specs/bugs/BUG-*.md` + `specs/bugs/registry.yaml` |
| Refactor / impact | `specs/tech-architecture/REFACTOR_LATEST.md`, `specs/tech-architecture/IMPACT_LATEST.md` |

## Code Style

- Functions: 4–20 lines. Split if longer.
- Files: under 300 lines. Split by responsibility.
- One thing per function, one responsibility per module (SRP).
- Names: specific and unique. Avoid `data`, `handler`, `Manager`, `Service`.
- Types: explicit. No untyped public functions.
- No code duplication. Extract shared logic.
- Early returns over nested ifs. Max 2 levels of indentation.
- Conditionals: expressed as positives.
- The Stepdown Rule (G34): functions descend exactly one level of abstraction.
- Names describe side-effects (N7): `sendWelcomeEmail`, not `processUser`.
- No magic strings or numbers (G25): every bare literal must be a named constant.
- Boolean logic in named functions (G28): complex boolean expressions extracted to a named predicate.
- Remove dead code (G9/F4): unused functions and imports must be deleted.
- Boy Scout Rule: leave every file you touch at least as clean as you found it.
- **Law of Demeter:** call only immediate collaborators.
- **Rust-specific:** Use `Result<(), anyhow::Error>` for fallible functions; `thiserror` for structured enums. Use `Serde::Serialize/Deserialize` derives on all data structs. Use `async_trait` for async traits.
- Exception messages must include the offending value, expected shape, and an actionable hint.

## Comments

- Write WHY, not WHAT.
- Complex logic must include "Provenance" links (commit SHA, ADR, or issue).
- Docstrings on public functions: intent + one usage example.
- No obvious comments that restate the code.
- No commented-out code (C5): dead code must be deleted.

## Tests (F.I.R.S.T — Uncle Bob Ch 9)

- Tests run with `cargo test`.
- Every new function gets a test. Every bug fix gets a regression test.
- Mocks for external I/O are named fake classes, not inline stubs.
- Tests are **F**ast, **I**ndependent, **R**epeatable, **S**elf-Validating, **T**imely.
- Never skip a test without an explicit ambiguity note.
- Test boundary conditions (T5): cover exact edge values — empty, max, min, off-by-one.
- Test through public interfaces only (T8): assert on observable outcomes.

## Dependencies

- Inject dependencies through constructor/parameter, not global/import.
- Wrap third-party libs behind a thin project-owned interface.
- Rust: Cargo.toml dependencies are pinned via Cargo.lock.

## Structure

- Follow the existing `src/` layout: `bootstrap/`, `core/`, `runtime/`, `tools/`, `setup/`.
- `core/workflow/` holds the DAG engine modules.
- `core/skill/` holds the `Skill<I>` trait and `SkillRegistry`.
- `core/llm/` holds the `LlmService` abstraction.
- Predictable paths: module directory → `mod.rs` + individual files.

## Formatting

- `cargo fmt` (default rustfmt rules).

## Logging

- Structured via `tracing` crate for debugging/observability.
- Plain text only for user-facing CLI output (currently none).

## Defensive Code

- **Retry with backoff** — for API/network calls (policy types in `core/workflow/policy.rs`: `RetryPolicy`)
- **Timeout** — for long-running operations (`TimeoutPolicy`)
- **Graceful degradation** — when external services/dependencies fail (Ollama LLM, Tavily API)

The agent implements defensive code only for categories explicitly listed here.

### Story e04s02: TAVILY_API_KEY Wiring — Config, Vault, Plugin

**type:** feat
**risk:** P1 (config/DI plumbing, no data loss)
**context:** infra
**BCPs:** 2

**Context:** The API key must reach `WebSearchSkill` without breaking the
existing DI chain. `AppConfig` gains a `tavily_api_key` field populated from
`TAVILY_API_KEY` (Vault-backed `OllamaSecrets` gets the field with a serde
default; `from_env` treats it as optional via `unwrap_or_default`).
`RuntimePlugin::new` takes the key and passes it to `WebSearchSkill::new`;
`main.rs` supplies it from `container.config`. Draft is in the tree.

## Steps

1. Verify `AppConfig.tavily_api_key` field + population in both `from_vault` and
   `from_env` paths (optional in env path)
   → verify: `grep -q 'tavily_api_key' src/setup/config.rs && grep -q 'TAVILY_API_KEY' src/setup/config.rs && echo OK || echo MISSING`

2. Verify Vault secrets struct carries `TAVILY_API_KEY` with serde default
   → verify: `grep -q 'TAVILY_API_KEY' src/setup/vault.rs && echo OK || echo MISSING`

3. Verify `RuntimePlugin::new(llm, tavily_api_key)` passes the key into
   `WebSearchSkill::new`
   → verify: `grep -q 'tavily_api_key' src/runtime/plugin.rs && grep -q 'WebSearchSkill::new(self.tavily_api_key' src/runtime/plugin.rs && echo OK || echo MISSING`

4. Verify `main.rs` passes `container.config.tavily_api_key` to the plugin
   → verify: `grep -q 'config.tavily_api_key' src/main.rs && echo OK || echo MISSING`

5. Verify the crate compiles with the new constructor signature (no stale
   single-arg `RuntimePlugin::new` callers remain)
   → verify: `cargo check 2>&1 | tail -1 | grep -q 'Finished' && echo OK || echo FAIL`

## Verification Script (Step-by-Step)

1. `cargo check` — clean compile (validates the constructor signature change
   propagated to every caller).
2. `TAVILY_API_KEY= cargo test --lib` — 161 tests still pass (key optional at
   startup, enforced at search time).

## Out of scope

- Real Vault server integration test (requires running Vault + AppRole creds)
- Making the key required at startup

## Risks

- Missed caller of the old `RuntimePlugin::new` signature — caught by `cargo check`.
- `.env`/`.env.example` drift — covered by e04s03.

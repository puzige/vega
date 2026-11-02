# MCP configuration invalidation without sticky suspension

## Scope and evidence

Issue #848. User observed Exa enabled but excluded from a new run; disabling and explicitly re-enabling restored actual tool use. The original triggering user operation was not captured; this recovery alone does not identify it.

Current source baseline: `ffcb349e` (task worktree created from current origin/master).

Confirmed source-level trigger chains:

1. `McpServerSettingsService::store` encounters invalid configuration (including configuration-wide validation failures) → `mcp_config::reconcile` fails → `RegistryState::revoke` adds all known IDs to `suspended` as well as cancelling old authority.
2. Repairing configuration without changing the MCP declaration leaves its previously enabled durable row and revision intact. `ready_for_run` → `ensure_ready` returns `Conflict` before connection/discovery; this early return does not persist `last_error_code`. Only explicit successful `set_enabled(true)` removes the latch.
3. Successful reconciliation of new/changed MCP declarations also invokes `revoke`, but storage intentionally projects these rows as disabled (`write_projection`). This is an existing activation boundary, not stale enabled-state recovery; keep it unchanged.
4. `test_connection` does not gate on `suspended`, so a successful transient connection test does not establish run readiness.

These deterministic code paths explain a persistent failure class consistent with the user report, not proof of the unrecorded original action.

## Minimal design

In `crates/vega_conversation/src/mcp_settings.rs` split `RegistryState::invalidate_connections` from `RegistryState::revoke`:

- Invalidation removes step-up challenges and pending/prepared OAuth, cancels active OAuth and connecting tokens, increments epoch, revokes and removes old leases. It neither inserts nor removes persistent suspension.
- Explicit `revoke` first inserts suspension and delegates the remaining work to invalidation. All existing explicit edit/disable/remove/credential/OAuth callers remain unchanged.
- Only the reconciliation error branch in `store` uses invalidation. Successful changed-declaration reconciliation continues to use `revoke` and retain storage's forced disable. A reconciliation error still returns failure and cancels existing authority. It does not authorize any new operation. Subsequent success must re-read/validate the current unchanged declaration before any new connection.
- Never clear all suspension, resurrect a lease, cache an old client, change revision/epoch/admission checks, or replay a call.
- Explicit failed disable/remove/edit remains suspended even if durable configuration is still enabled; a later successful reconciliation cannot undo that safety latch. Explicit successful re-enable retains its existing semantics.

No production UI, schema, protocol, endpoint, credential-storage, generic telemetry or dependency changes. Development-only `mcp-test-support` exposes an inert in-memory ready handle to the conversation tests without a transport; remove this feature, constructor and dev-dependency edge together with the tests before final-head CI.

## Regression matrix and validation

Development-only Rust tests run on GitHub Actions; no local Vega execution, build or tests. Per repository policy retain accessible test source/SHA and actual results, then remove Rust test-only support before final head CI and merge. User application integration remains separate.

Use isolated temporary configuration/database fixtures, no real credentials, no network/process fixtures. Exercise production configuration reconciliation and service methods; use missing synthetic bearer credentials to distinguish reaching connection admission (`Credential`) from stale suspension (`Conflict`) without external I/O.

Required scenarios:

- Imported/changed YAML MCP declarations remain disabled and suspended until explicit activation; no network connection occurs.
- A normally explicitly enabled service reaches credential validation rather than stale Conflict. Changed MCP declarations cancel old attempts and advance epoch without silently activating replacement configuration.
- Malformed configuration invalidates existing authority and fails closed; restoring valid configuration allows a fresh attempt but does not revive old tokens.
- Successful explicit disable excludes service from subsequent readiness and keeps old tokens cancelled.
- Successful remove/deleting state excludes service; no automatic resurrection.
- OAuth disconnect disables and clears authorization; bearer rotation disables and revokes old authority; no implicit consent or escalation.
- Failed explicit disable/delete with stale revision keeps the suspension despite enabled durable state and later valid reconciliation.
- Explicit suspension survives unrelated accepted configuration changes and repaired configuration errors.
- Old epoch/token and revocation leases remain invalid after new authority is constructed. Register an inert handle's lease in the real registry, trigger the actual `store` failure, repair configuration and assert revoked clones/lease and epoch advance. Runtime-local lease tests additionally verify irreversible revocation.
- OAuth disconnect starts with a synthetic installed owner-only authorization slot/issuer; assert real clearing and secret cleanup, then configuration repair cannot re-enable it. Uncredentialed enabled OAuth after repair returns `AuthorizationRequired` without creating authorization flows.
- No historical call dispatch is added; all existing post-connect epoch/revision/current-enabled/deleting checks and per-dispatch admission checks remain unchanged.

Static checks: formatting, diff whitespace, change scope and absence of test-only artifacts in final head. Independent non-implementer review of stopped exact version; latest final-head required CI before merge. Issue remains open/In review pending user tool-call validation.

## Recorded development evidence

- Tested head: `9c901375a8d2d1beda63bfae947ff2cb854a45a5`.
- [Actions 41355066874](https://github.com/puzige/vega/actions/runs/41355066874): required check and Clippy succeeded; Nextest 15 discovered / 15 executed / 15 passed / 0 skipped. Python suites: 5, 15 and 40 passed.
- Original [conversation regression source](https://github.com/puzige/vega/blob/9c901375a8d2d1beda63bfae947ff2cb854a45a5/crates/vega_conversation/src/mcp_settings/suspension_tests.rs), Git blob `e1284937bdeb5a508fdcde1dd857dcbfea913552`.
- Original [runtime lease regression source](https://github.com/puzige/vega/blob/9c901375a8d2d1beda63bfae947ff2cb854a45a5/crates/vega_runtime/src/agent/mcp_registry/revocation_tests.rs), Git blob `26969c13d830cd2b08205c3b4a2a3845312e0f31`. Remote content IDs were read back and matched local files before removal.
- [Original development patch](https://github.com/puzige/vega/commit/9c901375a8d2d1beda63bfae947ff2cb854a45a5.patch). Earlier failed runs and fixture corrections are recorded in Issue #848; no business/security assertions were relaxed or dropped.
- Per repository policy, remove the two test modules, their declarations, inert constructor, feature and dev-dependency edge after preserving evidence. Final production change remains only the invalidation split and configuration-error call-site change; no runtime API/manifest diff remains.
- Local static checks: `cargo fmt --all -- --check`, `git diff --check`, `cargo metadata --no-deps --offline --locked --format-version 1` succeeded. No local code/test execution or compilation.
- Independent implementation and coverage review found no remaining issue after registered-lease and installed-authorization coverage was added. Final cleanup/head review and full final-head CI are separate gates, pending at this record's creation. Real application/tool-call acceptance remains pending user validation.

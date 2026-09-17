# A7-01 first-message preflight delivery

The first composer submit now performs provider/model and owner-only credential
readiness before draft materialization. A rejected submit keeps the draft and
input editable, creates no thread/message rows, and projects a typed,
actionable Settings → Providers repair message. Once readiness is repaired
through Vega's own provider settings authority, the retained draft follows the
existing single-materialization and normal post-start persistence path.

## Freeze

- verified_at_utc: `2026-09-17T08:41:49Z`
- verified_at_local: `2026-09-17T16:41:49+0800` (Asia/Shanghai)
- branch: `feat/a7-first-message-e2e`
- git_head_before_delivery_commit: `9e1305cc7737`
- implementation_diff_sha256: `ebd8c6e3275118d3f1d40d380706d7278706a2c5fc593f41e8794031b20a78f7` (tracked implementation/test diff, excluding this report)
- task_contract: `docs/vega-a7-first-message-preflight.md` A7-01; no credential import, installation, push, merge, migration, or unrelated UI work
- os_arch: `Darwin 24.6.0 arm64`
- rustc: `rustc 1.98.0 (88d9e12ae 2026-08-18) (Homebrew)`
- cargo: `cargo 1.98.0 (797e8a9bc 2026-08-05) (Homebrew)`
- git: `git version 2.55.0`

## Implementation

- `vega::app_agent::preflight_provider` reads the selected config and the
  owner-only keystore only from the bounded preflight worker. It requires one
  enabled provider with the selected model, a syntactically valid base URL, a
  non-empty credential reference, and a readable local credential. It returns
  only provider/model identifiers; no credential value is returned, logged, or
  copied.
- `TrustedActionKind::AgentPreflight` and the async submit path hold a
  single-flight lease while readiness is checked. The UI submit path performs
  no synchronous config or keystore IO. Stale route/settings callbacks release
  their exact lease and reject without materializing a task.
- `ProviderPreflightFailure` is shared through `vega_conversation::types` and
  projects provider-unavailable, ambiguous, malformed, and missing-credential
  failures with the visible repair path `设置 → Providers`. The original
  `执行未完成` post-start failure behavior remains unchanged for runs that
  already started.
- Production-path R69 fixtures now use owned temporary config/credential
  files. They do not read or modify user credentials or user configuration.

## Results

| requirement | evidence class | exact command | result | duration | bounded footer/hash |
| --- | --- | --- | --- | --- | --- |
| Disabled, unavailable-model, ambiguous, malformed-URL, missing-credential, repeated-submit, and repaired-submit behavior | E2E-REAL | `./scripts/cargo-lock.sh --wait test -p vega a7_ -- --nocapture` | PASS | 1.16s | 8 passed, 0 failed, 129 filtered out; owned temp store/config and production `VegaWindow` path |
| Existing lazy-draft regressions | E2E-REAL | `./scripts/cargo-lock.sh --wait test -p vega r69_ -- --nocapture` | PASS | 1.35s | 17 passed, 0 failed, 120 filtered out |
| Deliberate preflight bypass catches materialization | FAULT-INJECTION | `./scripts/cargo-lock.sh --wait test -p vega a7_first_submit_disabled_provider_rejects_before_materialization -- --nocapture` with only failure projection bypassed | EXPECTED FAIL | 0.26s | exit 101; 0 passed, 1 failed; assertion at `crates/vega/src/tests/r69.rs:314` reported `left: 1`, `right: 0` |
| Restored preflight rejection | E2E-REAL | `cargo fmt --all -- --check && ./scripts/cargo-lock.sh --wait test -p vega a7_first_submit_disabled_provider_rejects_before_materialization -- --nocapture` | PASS | 5.68s compile + 0.25s test | 1 passed, 0 failed, 136 filtered out; mutation restored before this run |
| Workspace tests and doctests | INTEGRATION | `./scripts/cargo-lock.sh --wait test --workspace` | PASS | ~98s (executor polling) | All executed tests/doctests passed; `vega` 137, `vega_conversation` 304 + 3 ignored, pagination 10, restart 4 + 2 ignored, S5 1, S6 2, S7 1, stop/resume 4, stream estimate 14, task cost 7, todo 1, usage pricing 1, markdown 32, runtime 96 + 4 ignored, store 95, UI 331, xtask 36; no failures |
| Formatting | UNIT/INTEGRATION | `cargo fmt --all -- --check` | PASS | <1s | no output |
| Strict workspace lint | INTEGRATION | `./scripts/cargo-lock.sh --wait clippy --workspace --all-targets -- -D warnings` | PASS | 9.08s | no lint errors; existing `block` future-incompatibility notice only |
| Diff hygiene | UNIT/INTEGRATION | `git diff --check` | PASS | <1s | no whitespace errors |

The workspace test suite retained its existing load-sensitive ignored tests;
they were not relabeled as passes. The mock provider/network boundary in the
production-path tests proves controller, persistence, and UI behavior only; it
does not prove a live provider response.

## Residuals

- **ACCEPTED LIMIT** — Live-provider/native acceptance was not run by this
  executor. No real Pi/Vega credential was read or transferred, and the live
  diagnostic available to the coordinating agent reported upstream quota/balance
  failure. This report makes no live-network success claim.
- **ACCEPTED LIMIT** — A provider or credential can change after preflight and
  before the existing worker's provider construction. Such a post-start or
  construction failure retains the materialized thread and retry context under
  the existing conversation rules; preflight only guarantees the rejection
  boundary before materialization for the observed readiness state.
- **NOT RUN** — No package installation, native app replacement, push, merge,
  release, or protected-branch operation was performed.
- **ACCEPTED** — No migration, historical empty-task cleanup, model pricing,
  permission policy, or unrelated UI behavior was changed.

## Changed paths

- `crates/vega/src/app_agent.rs`: worker-side provider/credential preflight.
- `crates/vega/src/window/agent.rs`: asynchronous preflight lease, route fence,
  and materialization gate.
- `crates/vega/src/trusted_action.rs`: preflight action ownership.
- `crates/vega_conversation/src/types/provider_settings.rs`: shared typed,
  secret-free preflight failures and repair messages.
- `crates/vega_ui/src/conversation_stream/{mod,core}.rs`: typed error
  projection while preserving draft/input.
- `crates/vega/src/tests/{r69,agent,composer_actions,history}.rs`: production
  handler fixtures, readiness regressions, recovery, and async-submit timing.

The `provider_settings.rs` change is additive and does not alter existing
provider-settings DTOs or credential import behavior; the coordinating agent
should resolve it explicitly with the parallel A7-02 importer change.

No push or merge was performed.

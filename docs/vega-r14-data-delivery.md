# R14-D provider settings delivery

Root integration update (2026-09-06): final application `0d7056a` passed 988/0/0 workspace gates and native acceptance. Root-owned checks listed below are historical executor handoff boundaries; final results and remaining limits are in [R14 acceptance](vega-r14-acceptance.md).

## Freeze

- verified_at_utc: 2026-09-06T07:32:35Z; verified_at_local: 2026-09-06T15:32:35+08:00
- branch: `codex/r14-provider-data`; git_head before evidence commit: `7612de6`
- tracked_diff_sha256 at verification (before this report): `786016cf2b5db72ff0c7c7071101bd7a434273c1169c0be352d967eead6cd609`
- task_contract: `docs/vega-r14-provider-management.md`, R14-D. Root review accepted completed nonempty reasoning output as connectivity evidence and a 128-token fixed probe budget.
- os_arch: Darwin arm64; rustc 1.98.0; cargo 1.98.0; git 2.55.0.

## Scope

`ProviderConfig.enabled` defaults true for old files and Rust defaults; Debug hides the URL. New-run resolver and session catalog use enabled providers only. Existing frozen clones remain immutable.

Shared operation identity, result and finite errors live in conversation types. The production service loads owned config, applies exact-provider and exact-order optimistic patches, merges only requested fields, serializes form/patch/delete authority, preserves existing enabled/key_ref on form edits, and restores previous credentials when a subsequent config save fails. Discovery returns candidates without writing config; import is explicit. A successful network response is checked against the current disk provider baseline again before delivery.

The runtime uses the existing chat body encoder and eventsource decoder, an actual reqwest request, explicit never-retry policy, redirect refusal, 5-second connection and 15-second total HTTP deadlines, and a 1 MiB body bound even without Content-Length. Discovery limits the incoming list to 1000 rows and validates IDs. Probe sends only `Reply with OK.`, no tools, and a 128-token cap. It requires nonempty text or reasoning, a valid terminal reason, and DONE; content is discarded. Invalid URLs are rejected before credential lookup. Credentials come only from the R10 store. HTTP error bodies are never surfaced, and successful discovery payloads cannot echo the key into a result.

## Results

All commands ran from the dedicated data worktree using the integration target directory via `CARGO_TARGET_DIR`; the target path is omitted here.

| requirement | evidence class | exact command | result | duration | bounded footer/hash |
|---|---|---|---|---|---|
| Service, runtime, store compile | build | `cargo check -p vega_conversation` | PASS | 5.59s | `Finished dev profile`; log SHA256 `1361b145e3b79a72d89eb452bd0134ee8eae86164d25bdc6ab95faa7e1475ecd` |
| Production config, local credentials, real loopback transport | E2E-REAL with owned HTTP fixture | `cargo test -p vega_conversation provider_settings -- --nocapture` | PASS | 3.32s compile + 15.66s tests | `10 passed; 0 failed; 0 ignored; 288 filtered out`; log SHA256 `c8d7c3cf776d9eae163f7e91dd51e3190cf48818d83292d90efc075218c2dd0f` |
| Strict scoped lint | static | `cargo clippy -p vega_conversation -p vega_runtime -p vega_store --all-targets -- -D warnings` | PASS | 5.22s | `Finished dev profile`; log SHA256 `e6f44b7442b774739c037ba5d0dad7652b55b24de89e4a783094dfea1f3c3de3` |
| Format | static | `cargo fmt --all -- --check` | PASS | 0.92s | exit 0, no output |

The 10 service tests verify actual GET path/header, separate POST fixed body, semantic success, malformed/empty/no-DONE/error payload rejection, HTTP authentication and redirect failures with one attempt, known-length and chunked oversized responses, a real 15-second timeout, in-flight cancellation, missing credentials, unsafe URLs, disabled/stale baselines, completion-time external disable, multiline SSE and reasoning-only completion, explicit import ordering, reloaded persistence, default compatibility, no partial config mutation, and actual credential rollback after a forced config write failure.

First failure logs remain under `/private/tmp` with names `vega-r14-d-first-tests.log`, `vega-r14-d-second-tests.log`, `vega-r14-d-final-tests.log`. Their hashes are respectively `964e4a1b7201de0f58308b169324ebe38d4a551d5d6e8467a08dd00c284d6f8b`, `3b8efc8e2a031efff781c10bbb184e490e61e0159e487ff9861c441fa2f2e84a`, and `1b986b5dab109f45d21890dcc27a91e70b9aa42c516705b27f317343a186fdc0`. Cause: the owned nonblocking listener's accepted socket inherited nonblocking mode on macOS, leading to `WouldBlock` before request bytes arrived. The first attempted textual fix missed the formatted statement; the explicit `socket.set_nonblocking(false)` fixture change fixed the cause. Production behavior and assertions were not weakened.

## Residuals

- NOT RUN here: workspace gates, native acceptance, UI tests and app resolver regression; these require U integration and are owned by root. The new app regression is `disabled_providers_do_not_resolve_or_make_enabled_models_ambiguous`.
- LIMIT: tests use actual owned loopback HTTP and synthetic credentials; no real provider, paid model, Keychain, or production credential was contacted. No DB migration or dependency was added.
- LIMIT: scoped tests do not reproduce a five-second TCP connect stall separately; the actual client explicitly sets that bound. The full 15-second deadline is exercised against the real transport.
- LIMIT: in-process service writers share one mutex and disk baselines are rechecked. Arbitrary external processes do not participate in that lock; atomic config rename prevents partial files, but this is not an OS-wide compare-and-swap transaction.
- LIMIT: credentials and config are two independent atomic files. Credential rollback after config-write failure is exercised; a simultaneous failure of the rollback filesystem write cannot be guaranteed recoverable and returns a finite credential error.
- SKIP: bench/soak as directed. Spec deviations: none; root-approved semantic clarification noted above.

## Integration follow-up: shared config writer authority

Root review authorized one additional narrow integration commit: provider background writes could race the legacy sidebar/default/theme load-then-save paths. The store now owns one edit mutex and `begin_edit(path)` returns a guard containing the latest `config`; `save()` writes while retaining that guard, including credential rollback. Provider patch/form/delete use it. `update_from(path, closure)` and global `update(closure)` merge only closure-owned fields. `AppConfig::save_to` also uses the same writer mutex to avoid the shared temporary-file collision. U and S own converting their respective callers; this follow-up changes no UI files or IO timing.

The deterministic concurrent regression starts provider-disable, default-model and sidebar-collapse workers from one barrier and verifies all three fields and untouched models/credential survive on real disk, without sleeps. Existing rollback tests pass under the common lock.

- verified_at_utc: 2026-09-06T07:37:46Z; verified_at_local: 2026-09-06T15:37:46+08:00
- pre-follow-up HEAD: `840efa8`; tracked diff SHA256 before report update: `016eed1fb303bae6444a9c1d347e35d35101d191b443b537985f9bcffcfe1841`
- `cargo check -p vega_conversation`: PASS, 1.39s; log SHA256 `fa044e73e76d2dd2c9582b92ea7c39678ca047817c6c89002196c54e507550ee`.
- `cargo test -p vega_conversation provider_settings -- --nocapture`: PASS, 8.62s build + 15.67s tests; `11 passed; 0 failed; 0 ignored; 288 filtered out`; log SHA256 `7d1df0c10ec8523c0763e13daa22f8a8c9997e130bfe48375a38327e383f7ef4`.
- `cargo test -p vega_store config::tests -- --nocapture`: PASS, 9.17s build/wait + 0.01s tests; `6 passed; 0 failed; 0 ignored; 87 filtered out`; log SHA256 `2bcac935e0303d68d0bef6b29db516357f8ce455bdc8c360ee69fc52aaa047bf`.
- `cargo clippy -p vega_conversation -p vega_runtime -p vega_store --all-targets -- -D warnings`: PASS, 3.71s including target lock wait; log SHA256 `09a79350c0ec0b7887572f1916697b0b2f032a7fdefb8a4abf0463e2364b3333`.
- All first follow-up runs passed. Full integrated UI/workspace gates remain root-owned; no assertion weakened and no additional dependencies.

# S6-T34 Verification Evidence

## Freeze

- verified_at_utc: `2026-08-30T23:00:01Z`
- verified_at_local: `2026-08-31T07:00:01+08:00`
- branch: `feat/s6-t34-commit-assistance`
- git_head: `d836df0037fb08d8463cb559416c8872ffc6add2` (existing SDD commit only)
- implementation commit / PR / squash: `PENDING / NOT CREATED / NOT CREATED`
- tracked_patch_sha256_before_this_evidence_file: `d1418218105974b4428edbb5e0fd6e7828d8198b842445658bbadaac61b62dd5`
- implementation_changed_file_content_sha256_excluding_this_evidence_file: `9504df0cc1af59d70e12ccea9d99624ec03a2aef9bacf79d4f95c84b1dee02e7`
- task_contract: `docs/vega-s6-tasks.md v0.16`; global verification contract: `docs/vega-exec-guide.md v0.6`
- verifier: local execution agent

The content hash above covers every modified or untracked implementation/spec file by sorted path and content, while deliberately excluding this self-referential evidence file. No raw diff body is committed.

## Environment

- os_arch: `macOS 27.0 / arm64`
- rustc: `rustc 1.97.1 (8bab26f4f 2026-07-14)`
- cargo: `cargo 1.97.1 (c980f4866 2026-06-30)`
- git: `git version 2.54.0 (Apple Git-156)`
- Git SHA-256 object-format fixture: `PASS`
- real provider / external network: `NOT USED`
- API key / Keychain mutation: `NOT USED`
- fixture policy: fresh owned temporary repositories with repo-local identity; no user repository mutation

## Results

| Requirement | Evidence class | Exact command | Result | Duration / bounded footer |
|---|---|---|---|---|
| Production headless Checklist → Prepare → Mock draft → Commit, real Git/repo terminal state | `E2E-REAL` | `cargo test -p vega_conversation --lib e2e_owned_repo_checklist_prepare_mock_draft_commit -- --nocapture` | `PASS` | v5.4.2 `3.29s`; `1 passed; 0 failed` |
| SHA-256 full-width authority and immutable proof | `INTEGRATION-DELEGATING` | `cargo test -p vega_conversation --lib sha256_repository_completes_checklist_prepare_and_commit -- --nocapture` | `PASS` | `7.30s`; real Git mutation delegated, Mock draft exact once |
| Raw rename/copy topology safety kernel | `UNIT/PROPERTY` | `cargo test -p vega_conversation --lib raw_rename_copy_topology_is_exact_and_fail_closed -- --nocapture` | `PASS` | `0.00s`; `1 passed; 0 failed` |
| Born zero/mixed OID service-entry rejection before mutation | `FAULT-INJECTION` | `cargo test -p vega_conversation --lib capture_head_service_rejects_bad_born_oids_before_any_mutation -- --nocapture` | `PASS` | `3.62s`; authoritative terminal workspace asserted |
| Current/rename-old `.gitattributes` fail-closed boundary | `INTEGRATION-DELEGATING` | `cargo test -p vega_conversation --lib selected_current_or_rename_old_gitattributes_is_zero_add_unsafe_filter -- --nocapture` | `PASS` | `2.44s`; delegating mutation recorder proves zero add |
| Production GPUI handler happy path: duplicate prepare/commit, close, real repo/provider-boundary wiring and consumer ordering | `E2E-REAL` | `cargo test -p vega --bin vega commit_app_production_handlers_reconcile_before_release_across_close_and_routes --no-fail-fast -- --nocapture` | `PASS` | v5.4.2 `6.89s`; `1 passed; 0 failed` |
| Commit result sender disconnect: production poll recovery, consumer→panel→successful release ordering | `FAULT-INJECTION` | same production GPUI handler command; one private post-worker/pre-send drop | `PASS` | real commit/recovery result; no forged terminal |
| Commit-focused regression | gate | `cargo test -p vega --bin vega commit_ --no-fail-fast -- --nocapture` | `PASS` | `11 passed; 0 failed`; `7.94s` |
| Workspace regression | gate | `cargo test --workspace --all-targets --no-fail-fast` | `PASS` | `677 passed; 0 failed; 1 ignored`; final-run slowest crate `73.16s` |
| Documentation examples | gate | `cargo test --workspace --doc --no-fail-fast` | `PASS` | `5 passed; 0 failed` |
| Formatting | gate | `cargo fmt --all -- --check` | `PASS` | exit `0` |
| Lints | gate | `cargo clippy --workspace --all-targets -- -D warnings` | `PASS` | final run `2.34s`; exit `0` |
| Build | gate | `cargo build --workspace --all-targets` | `PASS` | final run `2.39s`; exit `0` |
| Patch whitespace | gate | `git diff --check` | `PASS` | empty output; exit `0` |

## Production assertions exercised by the two primary E2Es

- Headless E2E uses `TrustedGitService::new` with no Git executable override: real attached branch, real filesystem, real `GitWorkspaceService`, real add/commit/proof/authoritative refresh, and a `MockProvider` only at the provider boundary.
- Headless terminal checks real porcelain-v2 clean state, exact one parent to the captured base, final tree membership, one provider request, `tools=[]`, `max_tokens=256`, and cleared owner authority.
- The app happy-path portion enters through real `CommitPanel` key events and production request handlers, with real repo mutation and MockProvider only at the provider boundary. The sender-disconnect portion is separately classified `FAULT-INJECTION`: one private seam drops the already-computed worker result before send, after which the real poll/recovery path re-reads workspace/branch/artifacts. UI consumers apply before one accepted panel terminal, and `lease_release` is recorded only after the coordinator's exact release succeeds.
- R/C raw topology, nonzero 40/64 OID codec/service entry, explicit filter values, and current/rename-old `.gitattributes` remain separate high-value safety regressions because a single real Git journey cannot generate every closed grammar or malformed authority shape.

## E2E-first prune record

- Interrupted `main.rs` branch delta was `+5610/-2100`; after removing the unverified generation-C/stale-route Cartesian expansion, dormant hooks/counters, hand-built recovered-result scenario, and a duplicate runtime-failure app test, v5.4.2 is `+4800/-2063`: `810` fewer added lines and `37` fewer baseline-deletion lines, net `847` fewer diff lines.
- App test inventory changed from `35` to `34`; the retained production-handler E2E now owns the real Commit sender-drop recovery assertion.
- Removed test-only decision seams: `drop_prepare_sender`, `before_workspace_final_hook`, `stale_before_result`, runtime/workspace failure counters, and the local forged recovery completion. Retained probe state is observation-only except the single sender-drop fault used to traverse the real production disconnect path.
- Production R/C authority checks, born/ref nonzero OID validation, disconnected-owner recovery, authoritative consumer reconciliation, SeqCst exact-token release, and accepted-terminal counter correction were not rolled back.

## Rerun history

- Before the E2E-first prune, the monolithic app fixture failed at a timing-dependent post-commit branch-generation assertion after it had torn down and rebuilt routes for generation-C/stale-route test-only sections. This was retained as a failed pre-freeze observation, not rewritten as a pass.
- After removing those unverified Cartesian sections while preserving production behavior, the reduced production-handler E2E passed twice. The full workspace regression then passed.

## Redlines

- `cargo tree -p vega_runtime` contains no `gpui` or `vega_ui`.
- No `Cargo.toml`, `Cargo.lock`, migration, DDL, or event-enum change was introduced by T34.
- Production portions of the new commit service, panel, and controller contain no `unwrap()` or `expect()`; test-only occurrences are confined below their test modules.
- Commit mutation remains exact in-memory stdin; no temporary message file, retry, rollback, restore, stash, clean, checkout, push, amend, real provider, or key access is used.
- UI uses theme tokens; no T34 hard-coded color value was added.

## Residuals and manual boundaries

- `ACCEPTED`: same-user changes between finite checks, including complete away-and-back ref/path/content ABA, remain the documented path-based Git TOCTOU residual. This evidence does not claim byte-atomic or race-free mutation.
- `LIMIT`: `MockProvider` proves local request grammar and production wiring only; it does not prove a real provider, network, billing, or model quality.
- `NOT RUN`: real API/key dogfood and Keychain mutation are human-only. The workspace test's one ignored test is the existing real macOS Keychain roundtrip.
- `NOT RUN`: manual visual review and hardware performance are T35/S8 evidence, not inferred from GPUI handler tests.
- `LIMIT`: no additional public API was created for evidence. Provider-constructor-to-loopback composition remains covered at the runtime retry-policy boundary, not promoted to a new T34 public seam.

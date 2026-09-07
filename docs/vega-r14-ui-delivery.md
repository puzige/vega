# R14-U delivery

Root integration update (2026-09-06): final application `0d7056a` passed 988/0/0 workspace gates and native acceptance. Root-owned checks listed below are historical executor handoff boundaries; final results and remaining limits are in [R14 acceptance](vega-r14-acceptance.md).

## Freeze

- verified_at_utc: 2026-09-06T07:42:10Z
- verified_at_local: 2026-09-06 15:42:10 +08:00
- branch: codex/r14-provider-ui
- git_head: `12c5750` plus final provider-form baseline patch
- tracked_diff_sha256 before this report: `523426bc06e2d62988d517ee69abc008a064b33d8fbf414b1c5dc2e0e1063dd6`
- task_contract: `docs/vega-r14-provider-management.md` R14-U
- os_arch: Darwin arm64; rustc 1.98.0; cargo 1.98.0; git 2.55.0

## Results

| Requirement | Evidence class | Exact command | Result | Duration / bounded footer |
|---|---|---|---|---|
| Mounted Settings pointer → actual service → owned config and loopback HTTP, enable/reorder/discovery/select/import/test | E2E-REAL | `cargo test -p vega_ui settings:: --lib` | PASS | 22 passed; 0 failed; test execution 0.19s |
| Existing model multiline layout, form keyboard actions, pricing/reasoning, usage statistics | E2E-REAL / supporting UNIT | Same command | PASS | Existing assertions retained; form test now opens the explicit Edit surface; asynchronous save test waits before identical assertions |
| Preference worker preserves newer provider/sidebar fields and rejects same-field conflict; rename collision retains exact form baseline and disabled/key reference | E2E-REAL | Same command | PASS | Production background worker and owned config; handlers invoked directly for these disk-conflict invariants |
| Strict UI crate lint, including tests | STATIC | `cargo clippy -p vega_ui --all-targets -- -D warnings` | PASS | Finished dev profile in 1.53s |
| Formatting | STATIC | `cargo fmt --all -- --check` (commit hook) | PASS | Pre-commit OK |

Latest raw logs are retained as `/tmp/vega-r14-ui-tests-7.log` (SHA256 `2548623661ff952aa9e4af4b58f2f8000133c0973589dc1e8bab75c394fd643d`) and `/tmp/vega-r14-ui-clippy-4.log` (SHA256 `42da84513e80fa6e2e26358f245935d7d3cbbb0b99bff84a64ec6859f7a088a1`).

First attempts were retained: `vega-r14-ui-check-1.log` failed on private-module import, corrected to the public re-export; `vega-r14-ui-tests-1.log` had 16 PASS / 1 FAIL because the old test focused an unmounted form before opening Edit; `vega-r14-ui-clippy-2.log` found the now-test-only legacy upsert helper and a cfg-dependent needless return. These were corrected without suppressing warnings or weakening assertions. Intermediate logs numbered 2–6 remain in `/tmp`.

## Implementation

- Provider selector and details use existing theme tokens, truncation, separate scrolling, enabled state and masked credential presence. A dedicated width token is the only added theme token.
- All new provider network, save, import, ordering, reload and preference disk operations run in background workers. Existing pinned workspace tokio and tokio-util are the only UI dependency additions.
- Network callbacks require operation ID, generation, exact provider snapshot and active cancellation token. Switching sections/providers, Escape, Back, SettingsOpen route closure and view drop cancel/invalidate network work. Discovery never writes without Add Selected.
- Short explicitly submitted disk transactions may finish after close; their callbacks do not switch the new target or clear newer drafts. Failed model/import/form mutations retain input. Service authority serializes field edits; preference patches preserve unrelated fields.
- Production form saves use the conversation service. Old injectable synchronous key/config writers are compiled only for existing fault tests.
- Provider renames use the exact baseline captured when Edit opens, preventing a changed name from silently editing another provider.

## Residuals

- NOT RUN by U: full workspace gates, packaged native app checks, final 960×600 and 1280×750 native light/dark screenshots, OS pointer drag and restart verification. Root owns those integration gates.
- LIMIT: U mounted production pointer loopback test uses 1280×750 light/dark; existing form/keyboard tests cover 960×600. Programmatic rendering is not a claim of native pixel acceptance.
- LIMIT: cancellation races, malformed/oversized/network error boundaries have production service evidence in R14-D; U does not label those as pointer E2E. Root native cancellation/late-result checks remain separate.
- ACCEPTED: no real provider or key, no Keychain, no paid request, no context/vision capability editor, no bench/soak. Existing upstream `block` future-incompatibility notice is unchanged.

## Native follow-up: obsolete credential failure after successful save

Root reported native build `1d17b90`: Test with no local key showed the expected credential failure; Edit Provider → enter an owned synthetic key → Save succeeded and the badge changed to 已存储, but the previous credential error remained at the bottom. This is the exact pre-fix native observation supplied by root, not a claim of post-fix native acceptance.

The narrow follow-up clears prior network messages and per-model results after successful provider save, patch (including model deletion/import), or reload. Provider selection already cleared both. Clearing results does not mark the updated configuration as tested. Failures retain their diagnostics and inputs.

- verified_at_utc: 2026-09-06T07:52:52Z; local: 2026-09-06 15:52:52 +08:00
- git_head: `faa6ca9` plus follow-up patch; tracked_diff_sha256 before this report append: `081d3a6d49fcf4ea6463868c294171116314db06379a0d7daf1a6da5d240046b`
- E2E-REAL: mounted Settings pointer Test → pointer Edit → keyboard synthetic key entry / Cmd+Enter → production service / owner-only credential store. The regression verifies persisted key and badge presence with no remaining old message/status, then repeats the failure and verifies patch/reload clear it. No network destination is contacted because missing credentials fail before transport.
- `cargo test -p vega_ui settings:: --lib`: PASS, 23 tests, 0 failed, 0.26s test execution. Raw `/tmp/vega-r14-ui-native-followup-tests.log`, SHA256 `8fd1cfbcc81ad12babe13c178fa14dcf888ade1d2119c94874dfc4c42fbd65d8`.
- `cargo clippy -p vega_ui --all-targets -- -D warnings`: PASS, 1.47s. Raw `/tmp/vega-r14-ui-native-followup-clippy.log`, SHA256 `68c08b6820ef77a30c8a47a36b565752d6c9884ba173da019a3b4bde61f753fd`.
- NOT RUN by U: post-fix native recheck; root owns the running unique application instance.

## Native follow-up: small-window detail text collapse

Root reported native build `1d17b90` at 960×600: after importing a second model and/or displaying test status, the Base URL label remained while its value disappeared even at scroll top. The rendered regression reproduced the exact defect with three models including a long ID: URL bounds were `474px × 0px`.

The detail viewport now owns height and scrolling, while its child is an intrinsically sized, nonshrinking vertical flow. URL, model rows, status and form content therefore increase scroll extent instead of competing for constrained flex height. Typography, theme tokens, spacing and horizontal truncation are unchanged.

- verified_at_utc: 2026-09-06T08:00:26Z; local: 2026-09-06 16:00:26 +08:00
- git_head: `5b99b22` plus sizing follow-up; tracked_diff_sha256 before report append: `9a97ef992a86fb71af13f8c369cb679db8b78a83fc17473099fe56da1e8095b9`
- RENDERED INTEGRATION: actual SettingsView at 960×600 in light/dark, three models including a long ID, seeded error/status projection. Asserts a full pixel-snapped text line for URL/status, URL inside the viewport at scroll top, and content height greater than the scroll viewport. The seeded status is solely a layout fixture, not network E2E.
- First regression failure: `/tmp/vega-r14-ui-small-detail-before.log`, SHA256 `ee50546ec2e2972f11f39cf29e21f2cdce819c2aa1b1b55a816d0afd0ef93bd6`. Intermediate `/tmp/vega-r14-ui-small-detail-after.log` recovered URL height to 20px but exposed that the new assertion expected a fractional unsnapped token height; the test was corrected to account for integer pixel snapping.
- `cargo test -p vega_ui settings:: --lib`: PASS, 24 tests, 0 failed, 0.28s. Raw `/tmp/vega-r14-ui-small-detail-final.log`, SHA256 `ff2377c4bfa2af0f60f3e97018fe5cfdf139aaa4cf11149b34e65d6ebbf37ebd`.
- `cargo clippy -p vega_ui --all-targets -- -D warnings`: PASS, 1.19s. Raw `/tmp/vega-r14-ui-small-detail-clippy-final.log`, SHA256 `9e72d3df3ca41ec7f038f35bbae737cc5c9540b0e0ff6a7b326f37ab4f27a22e`.
- NOT RUN by U: post-fix native screenshot/scroll recheck; root owns the unique running app.

The same unintegrated sizing follow-up also addresses root's native Stop finding: Slow Test → Stop immediately restored the controls and removed the row's pending state, but left 正在连接… indefinitely in the footer. All operation/generation invalidations now clear the old footer message. The supporting pointer Stop regression starts from a seeded in-flight projection and verifies token cancellation plus removal of pending message/status; it is not labeled real network E2E.

Final combined checks: `cargo test -p vega_ui settings:: --lib` PASS, 25 tests, 0 failed, 0.25s (`/tmp/vega-r14-ui-small-detail-cancel-final.log`, SHA256 `c6bacc666a5bb9b076c170655df420f0af832e839bdcca4147b1174b54870d2d`); `cargo clippy -p vega_ui --all-targets -- -D warnings` PASS, 1.24s (`/tmp/vega-r14-ui-small-detail-cancel-clippy.log`, SHA256 `c66bcf168dd0f138fd37babe102c19a2625ae12a1962601a2a0945c5e8b87e42`). Post-fix native recheck remains root-owned.

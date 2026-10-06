# Issue #287: A4-C2B Codex task routing

Issue: https://github.com/puzige/vega/issues/287

Parent contract: [A4 Codex v1 spec](vega-acp-codex-v1-spec.md)
Related cards: [#281 ACP runtime](https://github.com/puzige/vega/issues/281), [#284 task identity](https://github.com/puzige/vega/issues/284)

## User outcome

From a fresh New Task, a user can choose Vega Native (the configured model Provider route) or Codex ACP for the draft, submit a coding request in the selected project/worktree, see its real response and tool activity, answer an ACP permission using the Agent's exact option, stop the run, and inspect the actual workspace diff. New and historical Native tasks retain their current route.

## Acceptance matrix

| ID | Risk / requirement | Precondition and operation | Expected observable result | Test layer | Evidence |
|---|---|---|---|---|---|
| C2B-01 | Native default and draft switching | Open New Task, add text and attachment, switch Native → Codex → Native | Default is Native; text and attachment are unchanged after both switches | UI state test | PASS: issue287 draft-switch test preserves text, attachment, and Native model state |
| C2B-02 | Existing task backend immutability | Open a materialized Native task and request backend switch | Selector is absent/disabled; persisted task remains Native | UI/conversation test | PASS: `issue287_materialized_native_task_backend_is_immutable` proves a committed Native task cannot open the selector, emit a backend-selection request, or change backend, while a fresh Native draft can open the menu and request Codex. `cargo nextest run -p vega_ui issue287_materialized_native_task_backend_is_immutable` — 1 passed, 561 skipped. |
| C2B-03 | Profile validation and safe storage | Save/load a valid absolute executable with safe argv; try relative path and sensitive flags/values | Valid profile round-trips; invalid inputs fail; config and task snapshot contain no credential material | Store/settings tests | PASS: profile validation/round-trip and immutable task identity tests pass |
| C2B-04 | Session-before-prompt ordering | Scripted peer accepts initialize/new/set_mode; inspect Store when peer receives `session/prompt` | One materialized thread, confirmed external session binding is already committed, then one prompt | Conversation/app test | PASS: scripted empty set_mode ACK test proves durable binding precedes the single prompt |
| C2B-05 | Fail-closed route | Try missing profile, failed initialize, unadvertised/rejected mode, definitive new-session error, and unknown create outcome | No Native provider construction or prompt fallback; draft is retained; definitive/uncertain state is recorded correctly; uncertain intent is not retried | App/Store tests | PARTIAL: worker tests cover initialize rejection, unadvertised mode, definitive `session/new` rejection, and unknown create outcome with no prompt/binding/message/retry. The app-level missing-profile regression confirms the error, retained Codex draft text, no materialization, and zero Native worker/provider requests. App-level draft restoration after a post-materialization session setup failure remains unverified. |
| C2B-06 | Unsupported attachments | Submit a Codex draft with an attachment the text-only path cannot encode | Clear pre-send explanation; composer retains attachment and text; no materialization or prompt | UI/app test | PASS: app-level GPUI composer regression submits text plus PNG on a Codex draft, verifies the clear unsupported-attachment error and retained route/text/image, with no thread/message/Codex task/session materialization, Native worker start, or MockProvider request |
| C2B-07 | Text and tool activity projection | Script text deltas and tool lifecycle notifications; repeat an external tool ID on another session | Correct stream/history updates are visible and durable; local IDs do not collide or cross threads | Conversation projection tests | PARTIAL: scripted text/tool lifecycle projection is covered; repeated external IDs across sessions lack a separate regression |
| C2B-08 | ACP approval identity | Send permission request with multiple named options; answer, then replay response against old/foreign request | UI shows original names; exact selected `optionId` is returned once; stale/duplicate/foreign responses are rejected | Conversation/runtime/app tests | PASS: UI and queue preserve exact option identity; duplicate, late, foreign-session, and unknown-kind cases fail closed |
| C2B-09 | Cancel and event ownership | Stop during prompt and during a pending permission, then route to another task | ACP cancel is sent, responders close, terminal status is truthful, late events remain owned by the original task | App lifecycle test | PARTIAL: Stop during permission, late permission after tool start, and foreign-session ownership are scripted; full route-away worker lifecycle is not integration-tested |
| C2B-10 | Actual workspace diff | Create an actual change in a disposable Git workspace and open Review from the task | Review reads the repository's actual diff; it does not synthesize a diff from protocol events | Git workspace/UI test; real adapter path also checked manually | PASS: the official real-adapter smoke task created a disposable Rust project, and Changes / Review showed the actual `src/lib.rs` diff (+26 / -0); see [issue evidence](https://github.com/puzige/vega/issues/287#issuecomment-6010021189). |
| C2B-11 | Native regression | Run affected Native composer, provider preflight, worker, cancellation, and Review tests | Existing Native route and task behavior remain unchanged | Focused regressions | PARTIAL: focused touched-package tests pass; broad Native behavior matrix was not run |
| C2B-12 | Real adapter acceptance | After merge/release, configure pinned official `codex-acp` v2.0.0 in an isolated local install and use a disposable Git project | A short real coding request edits a file; Vega shows response/activity/permission as applicable and the true Git diff; record installed build and adapter versions | Manual native app | PASS: official v0.1.61 smoke acceptance used the isolated pinned adapter and disposable `codex/acp-smoke` Rust project; Vega showed `workspace-write · on-request`, ACP execute/edit activity, the response, and the real `src/lib.rs` diff. See [issue evidence](https://github.com/puzige/vega/issues/287#issuecomment-6010021189). |
| C2B-13 | Hydrated ACP tool activity | Complete a Codex task, then reopen the same Vega conversation so durable tool audits are projected into the timeline | Successful ACP tool rows remain successful, with no corruption label or failed-call summary; output remains within the existing redaction boundary; malformed rows still fail closed | History projection regression; manual native app after release | PASS: regression reproduced on v0.1.61, fixed in PR #289 and released in v0.1.62. Reopening the same task in the official app showed all five persisted rows as `Codex completed tool activity`, with no corruption label or failed-call summary; malformed and oversized rows remain covered by the focused regression test. See the post-release evidence below. |

## Implementation plan

1. Add a validated, non-secret Codex ACP profile to app configuration and a minimal Settings → Agents editor for display name, absolute executable and argv.
2. Add backend selection to the draft/New Task state, preserving all composer state and preventing edits to persisted task identity.
3. Freeze the profile and selected canonical workspace into the existing A4-C2A task snapshot; reject unsupported attachments before materializing or sending.
4. Add a Codex worker route before Native provider/tools construction. Persist creation intent, launch and initialize the owned `vega_acp::Connection`, create the session, set and confirm `workspace-write`, persist session binding, then send exactly one prompt.
5. Project assistant and tool updates into conversation events with task-scoped Vega IDs. Add an approval response path that carries the exact ACP option IDs and ties responder lifetime to thread/run/connection generation.
6. Route Stop through ACP cancellation and the existing app run owner. Verify the completed task's Review derives changes from real workspace state.
7. Add/adjust the focused tests in the matrix before implementing each path; record exact commands and results below. Do not run workspace-wide tests locally; rely on required PR cloud checks for that gate.
8. Hydrate already-persisted ACP tool audits when opening the same conversation without calling ACP again. Preserve strict rejection of malformed data and prove successful rows remain successful after projection and timeline construction.

## Compatibility and failure rules

- Existing threads without an ACP snapshot remain Native.
- No ACP error or missing profile may fall through to Native.
- A confirmed session binding is a strict prerequisite for the first prompt.
- Unknown session creation outcomes are durable and non-retryable within this card.
- Rendering the same Vega conversation from already-persisted tool audits is in scope (C2B-13); this reconstructs display projections only and never invokes ACP or replays an external action.
- ACP session resume/load, automatic prompt replay, and reconstruction from ACP session history remain out of scope.
- Raw prompt, workspace file contents, process environment and credentials do not enter diagnostics or profile storage.
- The profile executable is launched only after an explicit Codex submit and only with its configured absolute path, argv, and the frozen selected cwd.
- Tests use in-process scripted ACP peers and existing seams; no real external process, network, or model call is part of the local automated test gate.

## Verification log

Focused local verification (all exit code 0):

- `cargo fmt --all -- --check` — formatting clean after applying rustfmt.
- `cargo test -p vega issue287_ --no-fail-fast` — 10 passed, 230 filtered.
- `cargo test -p vega issue287_late_permission_after_tool_started_fails_closed --no-fail-fast` — 1 passed, 239 filtered.
- `cargo test -p vega_ui issue287_ --no-fail-fast` — 2 passed, 559 filtered.
- `cargo test -p vega_conversation issue287_ --no-fail-fast` — 2 passed, 516 filtered; auxiliary integration binaries had 0 matching tests.
- `cargo test -p vega_conversation codex_task_identity --no-fail-fast` — 2 passed, 516 filtered.
- `cargo test -p vega_conversation codex_draft_materialization_is_atomic_and_reuses_its_id --no-fail-fast` — 1 passed, 517 filtered.
- `cargo test -p vega_conversation codex_session_intent_is_durable_and_confirmed_binding_alone_is_prompt_eligible --no-fail-fast` — 1 passed, 517 filtered.
- `cargo test -p vega_store codex_acp_profile_round_trips_and_rejects_unsafe_launch_values --no-fail-fast` — 1 passed, 153 filtered.
- `cargo test -p vega_conversation issue287_codex_acp_tool_projection_is_typed_and_value_free --no-fail-fast` — 1 passed, 517 filtered.
- `cargo clippy -p vega_conversation --all-targets -- -D warnings` — clean, exit code 0.
- `cargo test -p vega r49_utility_bar_mounts_above_the_card_only_on_the_new_task_page --no-fail-fast` — 1 passed, 239 filtered.
- `cargo test -p vega_ui r49_utility_bar_keeps_the_frozen_inset_and_chip_ladder --no-fail-fast` — 1 passed, 560 filtered.
- `cargo test -p vega_ui r64_project_menu_bounds_match_the_baseline --no-fail-fast` — 1 passed, 560 filtered.
- `cargo test -p vega_ui issue86_mcp_settings_tab_and_shift_tab_move_focus_between_actions --no-fail-fast` — 1 passed, 560 filtered.
- `cargo test -p vega_ui issue74_skills_settings_import_is_keyboard_reachable --no-fail-fast` — 1 passed, 560 filtered.
- `cargo test -p vega issue287_ --no-fail-fast` — 10 passed, 230 filtered.
- `cargo test -p vega_ui issue287_ --no-fail-fast` — 2 passed, 559 filtered.
- `cargo test -p vega_conversation issue287_ --no-fail-fast` — 2 passed, 516 filtered; auxiliary integration binaries had 0 matching tests.
- `cargo test -p vega_store codex_acp_profile_round_trips_and_rejects_unsafe_launch_values --no-fail-fast` — 1 passed, 153 filtered.
- `cargo clippy --workspace --all-targets -- -D warnings` — clean, exit code 0 after grouping Codex worker and prompt-finish dependencies into context structs.
- `git diff --check` — clean.

The parent-reported cloud run `cargo nextest run --workspace` exited 100 with 2080 passed, 5 failed, and 5 skipped. Three geometry failures came from inserting the backend chip before existing utility controls; two Settings focus failures came from placing Agents after Skills in keyboard order. The backend selector now follows the existing project/branch chips, and Agents precedes Skills in navigation so the established Skills-to-page focus path remains intact; all five named regressions pass in the focused local runs above. No workspace-wide test suite was run locally. Real adapter acceptance, merge, release, and native recheck evidence are recorded below. The cloud Clippy findings about a boolean comparison and two overly large function signatures were fixed; workspace Clippy now passes.

Resolved verification retries: the first `cargo fmt --all -- --check` exited 1 on rustfmt line wrapping; `cargo fmt --all` was applied and the final format check passed. The new late-permission test command exited 101 twice before its final passing run: the first assertion expected no wire response after cancellation, and the second expected an open request to return the same closed-request error after the transport had already sent its cancelled outcome. The test now verifies that cancellation outcome and that no second permission response can be sent.

### C2B-13 persisted ACP activity regression — 2026-10-06

- Vega self-fix route: the first Codex ACP task was stopped immediately after the user's direction to avoid ACP during self-bootstrapping; it made no source changes and does not count as a failed fix attempt. Vega Native then used project `vega-287-acp-history-replay`, branch `feat/287-acp-history-replay`, and passed `pwd` verification against the isolated worktree. No ACP request was used for implementation.
- RED from the Vega Native task: `cargo nextest run -p vega_conversation issue287_codex_acp_history` — exit 100; 0 passed, 1 failed, with the successful Codex ACP history projection assertion failing.
- GREEN independently rerun after the fix: `cargo nextest run -p vega_conversation issue287_codex_acp_history` — exit 0; Nextest run `433ef756-9fd7-4bec-9c94-196577758465`, 1 passed, 570 skipped.
- `cargo fmt --all -- --check` — exit 0. `git diff --check` — exit 0. No workspace-wide test was run.
- Source fix is restricted to the history projection: successful durable `codex_acp` rows recover `truncated=false`; all other tools/statuses retain their prior metadata. The test covers valid hydration, malformed identity, oversized output remaining corrupt, and non-ACP reused behavior. The merged/released native desktop recheck is recorded below.

### C2B-13 official v0.1.62 native recheck — 2026-10-06

- Installed the official v0.1.62 update through Vega's built-in updater and confirmed the version in Settings → General after restart. Release target: `b19a504d02232d7fe1551ba825f8dcff7c715743` ([release](https://github.com/puzige/vega/releases/tag/v0.1.62)). The app was `/Applications/Vega.app`.
- Reopened the same completed history task on `codex/acp-smoke`; no prompt was sent and no ACP call was started during this self-bootstrap recheck.
- Expanded the persisted activity group and each of its five rows (four `execute`, one `edit`). Every row rendered `Codex completed tool activity`; no `工具结果损坏` label or failed-call summary appeared. The existing assistant response remained visible.
- C2B-13 desktop acceptance: PASS. This recheck validates history display only; it did not resume the ACP session or replay any action.

### C2B-02 and route-choice official v0.1.62 spot check — 2026-10-06

- On the fresh New Task screen, the route control showed `Vega Native` and `Codex ACP`; Native remained selected. No ACP task was submitted during this self-bootstrap.
- Opened the existing completed Native task `Markdown 格式验收`. Its composer showed the `gpt-6-luna` model selector, with no backend/route selector or ACP option. This is consistent with persisted task route immutability in the shipped UI.
- The focused GPUI regression `issue287_materialized_native_task_backend_is_immutable` passed with 1 test and 561 skipped. It verifies that a committed Native task cannot open the selector or emit/change its route, while a fresh Native draft can open the menu and request Codex. C2B-02 acceptance: PASS. No route was changed and no prompt was sent during the desktop check.

### C2B-05 fail-closed route coverage — 2026-10-06

- After three Vega Native attempts covered the worker setup branches, a focused app-level test was added through the takeover path. No ACP route or external adapter was used for implementation.
- Attempt record: the first Vega run ended after 7m13 without a code change; the second initialize-rejection test panicked because its scripted peer unwrapped an expected EOF; the third run fixed that test helper and the initialize, unadvertised-mode, and new-session failure tests all passed.
- `cargo nextest run -p vega issue287_codex_preflight_failure_never_starts_native_worker_and_retains_draft` — exit 0; 1 passed, 243 skipped.
- Final PR worktree recheck: `cargo nextest run -p vega issue287_` — exit 0; Nextest run `d0a28e1a-aa38-4501-8df2-78938ba9fc19`, 15 passed, 230 skipped.
- `cargo fmt --all -- --check` and `git diff --check` — exit 0.
- The app test drives the real New Task route, chooses Codex, and submits with no ACP profile. It verifies the setup error, retained Codex draft/text, unchanged durable thread count, no draft messages, no Native worker start, and no MockProvider request.
- C2B-05 remains PARTIAL until the app-level restore path after a definitive post-materialization setup failure is regression-tested; the worker-level unknown outcome is durably uncertain and non-retryable.

### C2B-06 unsupported attachment submission — 2026-10-06

- `cargo nextest run -p vega issue287_codex_attachment_submit_rejects_before_preflight_and_retains_draft` — exit 0; Nextest run `debc72eb-bf72-4c61-9c8a-1ab57ff2f314`, 1 passed, 244 skipped.
- The GPUI app regression creates a draft, selects Codex, pastes a PNG, and submits text plus the image. It verifies the clear unsupported-attachment error, retained Codex route/text/image, no new thread/message/Codex task/session, and zero Native worker starts or MockProvider requests. Rejection occurs before ACP preflight; implementation used Vega Native without real ACP, model, external process, or provider calls.
- `cargo fmt --all -- --check` and `git diff --check` — exit 0. The first format check exited 1 on test line wrapping; formatting was corrected, after which the focused test and format check passed. The focused test had no failed attempt.

## Manual acceptance checklist

- [x] Record Vega build/version and official app identity: v0.1.61 was used for the original real-adapter smoke task; v0.1.62 at `/Applications/Vega.app` was used for the history recheck.
- [x] Use only pinned official `codex-acp` v2.0.0 installed under an isolated task-local directory; no global or unpinned adapter was installed.
- [x] Use a disposable Git repository with no secrets or user files and a short request that makes one small file edit.
- [x] Verify New Task → Codex, selected cwd, actual response/activity, and actual Review diff. Permission and Stop branches were not triggered in the completed smoke run; their focused automated coverage is recorded in the matrix.
- [x] Verify a Codex task uses the selected ACP route and a Vega Native task uses the configured model Provider route; the ACP smoke task and Vega Native self-fix task both completed through their displayed routes.
- [ ] Finish the remaining PARTIAL automated coverage entries in the matrix before closing #287 or marking it Done.

# Issue #287: A4-C2B Codex task routing

Issue: https://github.com/puzige/vega/issues/287

Parent contract: [A4 Codex v1 spec](vega-acp-codex-v1-spec.md)
Related cards: [#281 ACP runtime](https://github.com/puzige/vega/issues/281), [#284 task identity](https://github.com/puzige/vega/issues/284)

## User outcome

From a fresh New Task, a user can choose Codex for the draft, submit a coding request in the selected project/worktree, see its real response and tool activity, answer an ACP permission using the Agent's exact option, stop the run, and inspect the actual workspace diff. New and historical Native tasks retain their current route.

## Acceptance matrix

| ID | Risk / requirement | Precondition and operation | Expected observable result | Test layer | Evidence |
|---|---|---|---|---|---|
| C2B-01 | Native default and draft switching | Open New Task, add text and attachment, switch Native → Codex → Native | Default is Native; text and attachment are unchanged after both switches | UI state test | PASS: issue287 draft-switch test preserves text, attachment, and Native model state |
| C2B-02 | Existing task backend immutability | Open a materialized Native task and request backend switch | Selector is absent/disabled; persisted task remains Native | UI/conversation test | PARTIAL: route only offers backend selection for a draft; no dedicated materialized-task UI regression |
| C2B-03 | Profile validation and safe storage | Save/load a valid absolute executable with safe argv; try relative path and sensitive flags/values | Valid profile round-trips; invalid inputs fail; config and task snapshot contain no credential material | Store/settings tests | PASS: profile validation/round-trip and immutable task identity tests pass |
| C2B-04 | Session-before-prompt ordering | Scripted peer accepts initialize/new/set_mode; inspect Store when peer receives `session/prompt` | One materialized thread, confirmed external session binding is already committed, then one prompt | Conversation/app test | PASS: scripted empty set_mode ACK test proves durable binding precedes the single prompt |
| C2B-05 | Fail-closed route | Try missing profile, failed initialize, unadvertised/rejected mode, definitive new-session error, and unknown create outcome | No Native provider construction or prompt fallback; draft is retained; definitive/uncertain state is recorded correctly; uncertain intent is not retried | App/Store tests | PARTIAL: unavailable executable is rejected before materialization; rejected and transport-failed set_mode outcomes are tested; other setup error branches are not all scripted |
| C2B-06 | Unsupported attachments | Submit a Codex draft with an attachment the text-only path cannot encode | Clear pre-send explanation; composer retains attachment and text; no materialization or prompt | UI/app test | PARTIAL: code rejects unsupported blocks before materialization and draft-switch preservation is tested; no submit-with-attachment app integration regression |
| C2B-07 | Text and tool activity projection | Script text deltas and tool lifecycle notifications; repeat an external tool ID on another session | Correct stream/history updates are visible and durable; local IDs do not collide or cross threads | Conversation projection tests | PARTIAL: scripted text/tool lifecycle projection is covered; repeated external IDs across sessions lack a separate regression |
| C2B-08 | ACP approval identity | Send permission request with multiple named options; answer, then replay response against old/foreign request | UI shows original names; exact selected `optionId` is returned once; stale/duplicate/foreign responses are rejected | Conversation/runtime/app tests | PASS: UI and queue preserve exact option identity; duplicate, late, foreign-session, and unknown-kind cases fail closed |
| C2B-09 | Cancel and event ownership | Stop during prompt and during a pending permission, then route to another task | ACP cancel is sent, responders close, terminal status is truthful, late events remain owned by the original task | App lifecycle test | PARTIAL: Stop during permission, late permission after tool start, and foreign-session ownership are scripted; full route-away worker lifecycle is not integration-tested |
| C2B-10 | Actual workspace diff | Create an actual change in a disposable Git workspace and open Review from the task | Review reads the repository's actual diff; it does not synthesize a diff from protocol events | Git workspace/UI test; real adapter path also checked manually | PARTIAL: Review uses the selected project's existing Git diff path; no disposable Git workspace acceptance run |
| C2B-11 | Native regression | Run affected Native composer, provider preflight, worker, cancellation, and Review tests | Existing Native route and task behavior remain unchanged | Focused regressions | PARTIAL: focused touched-package tests pass; broad Native behavior matrix was not run |
| C2B-12 | Real adapter acceptance | After merge/release, configure pinned official `codex-acp` v2.0.0 in an isolated local install and use a disposable Git project | A short real coding request edits a file; Vega shows response/activity/permission as applicable and the true Git diff; record installed build and adapter versions | Manual native app | NOT RUN: requires post-integration manual acceptance with the pinned adapter |

## Implementation plan

1. Add a validated, non-secret Codex ACP profile to app configuration and a minimal Settings → Agents editor for display name, absolute executable and argv.
2. Add backend selection to the draft/New Task state, preserving all composer state and preventing edits to persisted task identity.
3. Freeze the profile and selected canonical workspace into the existing A4-C2A task snapshot; reject unsupported attachments before materializing or sending.
4. Add a Codex worker route before Native provider/tools construction. Persist creation intent, launch and initialize the owned `vega_acp::Connection`, create the session, set and confirm `workspace-write`, persist session binding, then send exactly one prompt.
5. Project assistant and tool updates into conversation events with task-scoped Vega IDs. Add an approval response path that carries the exact ACP option IDs and ties responder lifetime to thread/run/connection generation.
6. Route Stop through ACP cancellation and the existing app run owner. Verify the completed task's Review derives changes from real workspace state.
7. Add/adjust the focused tests in the matrix before implementing each path; record exact commands and results below. Do not run workspace-wide tests locally; rely on required PR cloud checks for that gate.

## Compatibility and failure rules

- Existing threads without an ACP snapshot remain Native.
- No ACP error or missing profile may fall through to Native.
- A confirmed session binding is a strict prerequisite for the first prompt.
- Unknown session creation outcomes are durable and non-retryable within this card.
- No automatic prompt replay, session resume/load, or transcript reconstruction is in scope.
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
- `git diff --check` — clean.

The parent-reported cloud run `cargo nextest run --workspace` exited 100 with 2080 passed, 5 failed, and 5 skipped. Three geometry failures came from inserting the backend chip before existing utility controls; two Settings focus failures came from placing Agents after Skills in keyboard order. The backend selector now follows the existing project/branch chips, and Agents precedes Skills in navigation so the established Skills-to-page focus path remains intact; all five named regressions pass in the focused local runs above. No workspace-wide suite was run locally, and real adapter acceptance or merge was not performed. The cloud Clippy finding about a boolean comparison was fixed by using direct negation and the focused conversation Clippy run now passes. Record the next cloud gate, merge SHA, and real adapter evidence after integration.

Resolved verification retries: the first `cargo fmt --all -- --check` exited 1 on rustfmt line wrapping; `cargo fmt --all` was applied and the final format check passed. The new late-permission test command exited 101 twice before its final passing run: the first assertion expected no wire response after cancellation, and the second expected an open request to return the same closed-request error after the transport had already sent its cancelled outcome. The test now verifies that cancellation outcome and that no second permission response can be sent.

## Manual acceptance checklist

- [ ] Record Vega build/version and official app identity.
- [ ] Use only pinned official `codex-acp` v2.0.0 installed under an isolated task-local directory; do not install a global or unpinned latest adapter.
- [ ] Use a disposable Git repository with no secrets or user files and a short request that makes one small file edit.
- [ ] Verify New Task → Codex, selected cwd, actual response/activity, permission behavior, Stop behavior where applicable, and actual Review diff.
- [ ] Verify a Codex task does not invoke a Native provider and a Native task continues to use the Native route.
- [ ] Record PASS / PARTIAL / FAIL / NOT RUN for each item; do not close #287 or mark it Done before acceptance is complete.

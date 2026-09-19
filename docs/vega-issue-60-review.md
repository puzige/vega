# Issue 60 review ledger

Date: 2026-09-19, Asia/Shanghai.
Status: implementation in progress; no acceptance or integration yet.
Contract: [R1–R5 and P1–P9/N1–N2](vega-issue-60-unpriced-chat.md).

## Red evidence before production changes

Main read the raw log for
`scripts/cargo-lock.sh test -p vega issue60_ -- --test-threads=1`.
Exit 101: 0 passed / 2 failed / 156 filtered out.

- `issue60_configured_unpriced_model_is_selectable`: failed on configured
  unpriced model missing from `model_options_for_pricing()`.
- `issue60_unpriced_first_submit_reaches_provider`: failed because real submit
  changed `SettingsOpen` to true, rather than continuing the conversation.

Raw evidence identifier: `vega-issue60-focused.7T8O3J/p1-p2-before.log`.
SHA-256: `e718a7f9c56cf5a4a61ed89ec6d5afbd6d94eaf56ce18c12ec7072e4ef75381e`.
Main reviewed test diff: real DraftFixture, actual selection subscription and
cmd-enter submission; only provider/network boundary mocked. Strengthening P2
success evidence to include assistant output, usage provenance and unknown cost
is required after the red run; provider invocation alone is not sufficient.

## Native prerequisites (not final acceptance)

Read-only Settings inspection of the currently installed prior build found an
enabled configured `hy4` model and no exact price entry for it. This is a useful
candidate for N1 without modifying user configuration or deleting a price.
The pricing-page screenshot is retained under local evidence identifier
`issue-60-2026-09-19/00-pricing-before.png`; no private screenshot was uploaded.
Model/server availability still needs a real post-fix UI response, not an
assumption from configuration. No model or price setting was changed.

## Required final review

Intermediate evidence: first complete app suite passed 158/158, 0 failed,
49.70 seconds (`vega-issue60-focused.7T8O3J/app-first.log`). P2 now verifies
the exact persisted assistant body and `done` status, 21 actual usage tokens,
NULL pricing provenance, `21 tok · —` meter text and reopened unknown aggregate
and summary cost. The first enhanced assertion mistakenly expected `completed`
instead of the actual protocol value `done`; its failed log is retained as
`p1-p2-after.log`. This correction is a test expectation typo, not an application
behavior change. Additional matrix cases and full final gates remain pending.

- Optional snapshot policy covers Ready, Saving.previous and all no-authority
  states without using unsaved pricing drafts.
- Configured model membership and provider/credential/permission/route gates
  remain distinct from optional accounting.
- First and persisted submits, approved plan, later priced turn, unknown/mixed
  usage and automatic-title calls follow the frozen contract.
- Old assertion updates are limited to explicitly superseded price blockers.
- Full gates, installed native E2E, persisted evidence, integration/cleanup and
  Issue/Project writeback remain pending.

## Additional matrix and independent review

Independent read-only review found no blocking production issue across optional
snapshot selection, configured model resolution, draft materialization,
ApprovedPlan, title accounting and unknown/mixed aggregate semantics. This is
code evidence, not a replacement for the remaining tests and native acceptance.

`matrix-rerun.log`: 5 passed, 0 failed, 156 filtered, 1.03 seconds. Includes
approved-plan execution, unavailable pricing states with automatic title and
committed/previous snapshot selection. `matrix-first.log` retains the earlier
missing test import compilation failure.

`app-final.log`: 159 passed, 2 failed, 56.70 seconds. Not accepted:
- New P7 integer-cost expectation was 1500; actual was 15. Executor must verify
  the fixture and pricing units before correcting any assertion.
- Existing automatic-title real HTTP test encountered `WouldBlock` in its local
  server read. Cause is not yet established; isolated verification required.

Original executor stopped on an account usage limit. Code and test artifacts
remain intact. A dedicated replacement executor owns the bounded follow-up;
main retains review, full gates and native installation ownership.

Native prerequisite only: prior installed build's built-in fixed-text provider
test reported `hy4` connection success. Screenshot
`issue-60-2026-09-19/01-provider-connectivity-before.png` was saved and opened;
SHA-256 `9f9edba90e699f381621847ab25813f947a8014dfc7a083ef73f1393e1401de0`.
No configuration was changed; this does not prove post-fix chat acceptance.

## Executor follow-up verified

P7 expectation corrected only after checking S7's unit definition:
15 tokens × USD 1/million × 1,000,000 stored units/USD = 15 stored units.
No production pricing formula changed. The original title HTTP test passed
unchanged in isolation and in the subsequent complete app suite.

Main inspected `app-recheck.log`: 161 passed, 0 failed, 50.16 seconds.
Format and strict app clippy passed; executor returned Cargo ownership.
Source frozen; full workspace test suite started by main. Native acceptance,
remaining workspace gates and integration are still pending. Raw intermediate
logs copied to persistent local evidence outside the worktree.

## Workspace acceptance progress

Frozen-source workspace run completed with exit 0: 1326 passed, 0 failed,
9 ignored. Exact command: `scripts/cargo-lock.sh test --workspace --
--test-threads=1`; raw local evidence `workspace-tests.log`.
Separate ignored tests and remaining format/clippy/build/tree/package gates
are in progress, not yet accepted. Project status is In review, Issue remains
open. Installed app is still the previous verified build.

## Final gate and native acceptance

All workspace gates now passed: 1326 tests, 0 failures; separate ignored suite
9 passed, 0 failures; fmt, strict workspace/all-target clippy, all-target build,
runtime dependency tree and package exit 0. No UI dependency in runtime tree.
The previous `block` dependency future-compatibility notice remains non-fatal.

Installed executable SHA-256:
`14281e1622c42c4e6e536a9c0a505237c0907faaf7b893b747e7b3442350e0a7`.
Main used native UI to create an owned task, select configured unpriced hy4,
send a harmless fixed-text request and receive the exact requested answer.
No forced Pricing route or manual configuration/database repair occurred.
Usage displayed 818 hy4 tokens and explicitly one unpriced call. Quit/restart
and reopen retained model, answer and unpriced accounting. N1/N2 passed.
Private screenshots 02–07 were saved outside the worktree, opened, verified
and hashed in local evidence manifest `issue-60-2026-09-19/manifest.md`.

P1–P9 coverage/classification remains as documented in executor delivery;
native evidence does not replace or overstate its mock/injected-state scope.
No blocking findings remain. Local integration, cleanup and card writeback
are pending, so this is not yet a completed delivery.

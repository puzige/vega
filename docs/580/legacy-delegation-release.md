# Historical terminal delegation release recovery

Issue: [#580](https://github.com/puzige/vega/issues/580). Acceptance v1, 2026-10-24.
Original baseline: `80eaba474df5a23c3a7849e11b39a0864b7e0ad6` (v0.1.217).
Takeover baseline: `cd04e0f0ac17467aedeafe0a4265fa41018b7339`.
Branch: `feat/580-release-takeover`.

## Takeover checkpoint, 2026-10-24

The user explicitly confirmed the old execution stopped and authorized takeover. Coordination marker: https://github.com/puzige/vega/issues/580#issuecomment-6387688791 . The original worktree and all its uncommitted artifacts remain untouched. Its production patch applied cleanly to the updated baseline; this is preserved work, not a new verified implementation. Acceptance A1–A8 remains unchanged.

Original production patch SHA-256: `6e08735d394885b6c96f1c03a7ada051ee2704cddcd3a468687654bea77d893e`.
Original spec SHA-256: `813f1b82e7b0d813aa2811a8c3c3f0ce18f25b5ef59a4389701377ea2359fa6a`.
Original development regression SHA-256: `9a0a390734ad6dcb361c02cbeb86d92ca51628501260b401d38856401dafe31f`.
Original fixture SHA-256: `93da4ebe79951d03ac3cf047ca680d93b60c80f0c4975bd895b9a7acbcffa28c`.

The recovery/load/snapshot call chain was re-inspected on the takeover baseline. No original cloud run or independent implementation review is recorded. Next: a single dedicated writer audits/completes the preserved implementation and regressions against unchanged acceptance; then fix an immutable content version for independent review and existing Actions. No local application execution, tests, build, live database mutation, installation or restart is authorized by this checkpoint.
Status: implementation authorized; cloud verification pending. This is a compatibility correction to [stage recovery](../556/stage-handoff-recovery.md), not a change to concurrency, ownership or review authority.

## Evidence and scope

Read-only investigation found a parent conversation with fifteen latest child executions: five completed and three failed without recovery rows, and seven interrupted with `recovered_abandoned`. Saved list/cancel receipts projected the eight terminal records as running with unknown release. Admission consequently exceeded the four-execution limit. No user identifiers, transcript contents or local paths are reproduced here.

Migration 0033 adds recovery metadata without backfilling older executions. Missing release rows become `unknown`. Both `stage_recovery::load` and `recover_thread_with_owner` handle running/cancelling and interrupted+unknown, but skip completed/failed+unknown. Load counts every state other than released/recovered_abandoned as occupied. `AgentRecord::snapshot` masks occupied terminals as running, while cancel uses the raw terminal and is a no-op. Every parent run reloads the same incorrect occupancy.

Only static code and saved read-only evidence have been inspected; there has been no local application/test execution. Current baseline still contains the diagnosed code. This task does not modify the user's live database, cancel tasks, install/restart the app, use CUA, raise capacity, change ordinary live execution release, or amend unrelated #548 work.

## Acceptance before implementation

| ID | Setup and action | Required observable behavior | Verification |
| --- | --- | --- | --- |
| A1 | Historical completed/failed child lacks a recovery row, ownership is available; reload the parent. | Acquire and validate existing exclusive ownership, re-read inside the existing coordinated transaction, persist recovered release. Original execution state/result/truncation/usage and completed/failed messages remain unchanged. No synthetic handoff, review readiness or successful validation. | Cloud development regression through production store recovery and supervisor load/receipts. |
| A2 | Load eight legacy terminal children as in the reported failure, then request new children. | Legacy executions no longer occupy capacity; actual supervisor reserve admits four new executions and rejects the fifth. Same-parent history remains inspectable. | Cloud regression using production load/reserve, not a duplicate counting algorithm. |
| A3 | Another native writer holds the same child ownership lock, including when the persisted execution appears terminal. | No recovery row/release/checkpoint is fabricated; the entry remains occupied, not review-ready, not a released wait result, and cannot followup/resume. Ownership-unavailable must not be mistaken for permission to recover. | Cloud regression with real production ownership on an isolated database; no external process/network. |
| A4 | Historical terminal is still release-unknown because ownership is unavailable. Query/cancel it. | Keep the durable terminal state and result visible with explicit unknown release and `ready_for_review=false`, rather than invent running/cancelling. Cancel does not falsely acknowledge release or free capacity. Existing live held/draining projection is unchanged. | Cloud receipt/cancel/wait boundary regression. |
| A5 | Terminal release is already held, quarantined, released or recovered_abandoned. | Compatibility recovery applies only to missing metadata: held/quarantined terminal records are not silently cleared; released/recovered records are unchanged. Quarantine still blocks admission. | Cloud negative controls and repeated-load comparison. |
| A6 | Reload recovered completed/failed terminals repeatedly, including a new parent run. | Idempotent release/identity/message linkage; no duplicate recovery checkpoint, changed result/usage, or altered original parent/execution identity. Other parent histories excluded. | Cloud store/load regression. |
| A7 | Historical interrupted+unknown, abandoned running/cancelling, or ordinary current execution. | Existing ownership-fenced interruption recovery, unknown-effect capture and future drain/release remain intact. Missing declaration never becomes ready. | Small targeted cloud regression and static review of unchanged release/resume paths. |
| A8 | Recovery metadata write fails before commit. | No partial release or linkage survives; original execution is retained, never admitted as released by a failed operation. | Cloud transactional failure injection into isolated store, not mocked recovery decisions. |

All rows are pending implementation/cloud validation initially. Tests are developmental only and must be retained in accessible Git history/PR evidence, then removed with their exclusive support before merge. Final empty-suite CI is separate evidence, not behavioral verification.

## Minimal implementation plan

1. `crates/vega_store/src/recovery.rs`: extend legacy terminal handling under `recover_thread`'s existing write coordination, nonblocking `DelegatedOwnership::acquire_for_thread` and transaction. Confirm proof before metadata writes. Distinguish completed/failed legacy terminal backfill from abandoned running/interrupted normalization: register the missing execution metadata and mark `recovered_abandoned`, preserving terminal result/message/usage. Do not invent a stage declaration, checkpoint/ready handoff for a normally ended historical execution. Re-read latest execution/release within the transaction; errors remain fail-closed. Do not alter migration 0033 or mass-update all unknown states.
2. `crates/vega_conversation/src/agent/subagents/stage_recovery.rs`: include completed/failed+unknown in the ownership-fenced recovery attempt; refresh authoritative data after successful recovery. Count only established released/recovered states as free, as before. No capacity increase or fallback that clears occupied on failure.
3. `crates/vega_conversation/src/agent/subagents.rs`: keep actual legacy terminal+unknown state visible without broadening release authority. Preserve current held/draining projection, `released_terminal`, reserve/followup/resume occupancy guards and live cancellation semantics. Do not change the public schema merely for this correction.
4. Add focused new development tests, preferably within the existing conversation subagents module so private supervisor APIs can be exercised without production public test seams. Isolated temporary file databases are necessary for the real ownership API (`database_path` rejects memory-only stores). Use `Store::open`, existing thread/message/delegation APIs, `stage_recovery::load`, `Supervisor::reserve`, `Registry::released_terminal` and the real cancel path. A minimal provider/host substitute may prevent external effects; it must not replace storage, recovery, locking, admission or snapshot decisions. Inspect the real provider/host APIs before creating test doubles. Do not restore old test collections or add new dependencies/CI workflows for convenience; existing std/ulid facilities can supply owned temporary paths.

One implementation writer owns these production files and directly scoped development test modules; parent owns spec/evidence/Git/PR and does not edit implementation concurrently. Non-implementer review starts only after writer READY and cessation of writes. No new production comments except necessary unsafe SAFETY lines; no non-test unwrap/expect.

## Verification and delivery

Local: only quick static/diff/format checks. No Vega execution, builds, cargo check, Clippy, tests or packaging, even if cached. Existing Actions PR check runs fmt, full Clippy, preserved Python suites and Nextest 0.9.146. Query cloud checks at five-minute intervals, except explicit completion/failure notification or user request.

Workflow: implement + focused regressions -> immutable development commit/draft PR -> independent review and actual cloud regression results -> fix responsible failures without weakening assertions -> record accessible full test SHA/source/patch and actual run/job counts -> remove only development Rust tests/exclusive wiring -> static cleanup audit and final exact-head full required check -> squash merge -> In review, Issue open. No claim of user acceptance or application installation.

User manual acceptance after installing a released build: continue an affected old parent conversation, verify latest historical terminal statuses and release state, and delegate one new read-only child successfully. Real active children must still block unsafe reuse; existing four-concurrent limit remains. Do not expose transcript/credentials in issue evidence. Rollback is a code revert through normal PR/CI; recovered metadata is additive and must not be deleted to simulate pre-upgrade state.

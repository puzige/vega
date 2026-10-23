# Stage handoff and recovery: validation ledger

Status: independent implementation review rejected the first runtime candidate for two ownership-lifecycle gaps; its cloud run also stopped on a macro import error before Rust discovery/execution. The same implementer has handed off the combined repair and stopped writing. Incremental full-head review and cloud verification are pending. This ledger records each candidate stage; final results are linked from [PR #572](https://github.com/puzige/vega/pull/572).

## Fixed contract and baseline

- Contract: [stage handoff and recovery v1.2](stage-handoff-recovery.md); the published tests/spec head initially used v1.1. v1.2 records internal-permission and steering integration corrections without weakening R1–R10.
- Original production baseline: `4c0d225daf47e66e559752ee78ba067520bea080`. Latest master `f3eab9827dcd1ddb459088410d6f1eedcbce8864` was merged without conflicts as `03c1296aefb346e6dc76f04e0d3c930b00463c04` before continuing the coupled recovery implementation; the first timer-removal patch and store drafts were retained.
- Before publishing the combined repair, master advanced to `9f9fa092a3ca391a65f08419c9206f32979119e6` (#573) and was integrated without conflict as `37b0291fd35ca0fe50f8cf2da9da907e593bd1ab`. This removed the 10 upstream development-only #570 cases and their wiring, leaving its production read-transaction fix intact. The task's repair manifest did not change.
- Branch: `feat/556-stage-checkpoint-recovery`.
- User instruction: remove the default 20-minute cancellation, implement durable artifacts/handoff and explicit interrupted recovery, then reuse and verify the existing #522 comparison. Retain independent review and CI; earlier termination is not faster delivery.
- The original implementation agent and original non-implementer reviewer are reused. No additional planning, testing or research agent was spawned.

## Architecture review before production changes

The independent reviewer found three concrete boundaries before test freeze/RED: all delegated recovery-normalization entry points must use the same OS ownership lock; ReadState is not a complete authorized call-bound Read receipt; and ready handoff needs a typed runtime-origin terminal outcome rather than fabricated model final metadata. Acceptance v1.1 incorporates all three. This is preparatory specification review, not implementation approval.

## Evidence states

| Stage | Exact content and result |
| --- | --- |
| Local static checks | Initial tests and the runtime candidate pass rustfmt/diff checks, including a main-agent `cargo fmt --all -- --check`. No local Vega build, run or tests. |
| Independent test review | Three fixture/observation corrections were independently rechecked and approved at `928cf828e80e24593c79c36f42869ac555cc09f6`; the 17-case source SHA256 is `4de8cf17a4593799f4242ae69deda6390250a705e1067a2ae930720c036d47d1`. |
| Existing Actions baseline result | [39883065565](https://github.com/puzige/vega/actions/runs/39883065565): Clippy/fmt and 60 Python checks passed; Rust discovered/executed 27, with 11 passed / 16 failed / 0 skipped. Direct timer behavior failed; most other new cases stopped at missing checkpoint/schema/resume prerequisites. |
| Runtime implementation | Same implementer handed off the complete runtime/UI candidate and stopped writing. The first retained clock after handoff is 2026-10-23T02:11:30Z. Terminal persistence, host release and interrupted recovery still require cloud verification and independent implementation review. |
| Candidate regression verification | First runtime candidate failed compilation before discovery/execution; no behavioral result is claimed. Original 17 cases are retained byte-for-byte, with 3 permission/Read cases and 1 native steering case added for integration findings. |
| Independent implementation review | Rejected `ee05bba185392d7606c6178814a342d802c6a0f4`: future-drop ownership and cancelled host-admission windows require repair. Other reviewed paths are not a blanket approval of the full candidate. |
| Final exact-head required check | Not run. |
| Merge and installed application | Neither claimed. |

## First cloud baseline and current policy

The draft [PR #572](https://github.com/puzige/vega/pull/572) first published the tests/spec head `928cf828e80e24593c79c36f42869ac555cc09f6`. Actions tested integration merge `a191c6098a7f175274b733fecd90ab82f705b185`, merging that head into `d4a65a381d95b6bb8e4ff8d2858a11a1042ffa3a`. Its 27 Rust cases include the 17 new recovery cases and the 10 storage-contention cases from #570; this was not a pure checkout of the original `4c0d225d` baseline.

Run creation → completion was 2026-10-23T01:16:32Z → 01:19:20Z, 168 seconds of wall time. Clippy/fmt job `120184747733` completed at 01:18:05Z; Nextest job `120184747938` at 01:19:15Z. The jobs overlapped and their durations are not added to the wall time. The test `elapsed_active_time_never_cancels_native_child_or_repeats_provider_work` failed with “elapsed active time must not cancel a native child.” Most deeper recovery assertions did not execute because their new interface or prerequisite behavior was absent. No unrelated compilation or runner failure was used as behavioral evidence.

That baseline run followed the policy checked out when it was started. The then-integrated #569 policy removed mandatory TDD/test freeze and stated that useful tests could remain. The 17 recovery cases and 10 upstream storage cases were retained at that stage, without starting a new RED cycle. #573 subsequently corrected that policy: TDD is still optional, but Rust tests and exclusive support must not enter master. Development test source/results must remain accessible, then final cleanup and full required CI must confirm 0 discovered / 0 executed, explicitly not a behavioral test pass. The upstream 10 cases were removed by #573 itself; the task's development cases stay until their failures are resolved and actual verification is recorded. Necessary test/observation repairs still preserve acceptance and record their cause. Independent review and user manual acceptance remain separate requirements.

The existing [#522 comparison audit](../522/existing-study-audit.md) was independently verified read-only from already completed evidence with zero new model calls. It is separate from this runtime's correctness and speed evidence. Historical #561's 28-test result and final empty-suite gate remain in [their original ledger](test-evidence.md); they do not validate this revised behavior.

## Pre-RED corrections and rework

The initial tests-only handoff contained 17 native tests, SHA256 `d81812b0c51bda321c75a60a18ac33c24f23c437833ef26d13252797b210f272`. Production still matched the baseline apart from conditional test wiring. Independent review identified these observation faults before any cloud run:

1. The shared fixture put database checkpoints inside the test project, so the existing safety layer would reject the Write positive control before the intended assertion. Separate workspace and database/checkpoint roots; keep the safety rule and assertion.
2. The mixed-ready test cancelled immediately after the checkpoint event, potentially before the later Write could dispatch. Observe the next provider request, actual permission request or release event before cancelling. Replace the unrelated one-second release deadline with event competition and an outer safety timeout.
3. The handoff/followup test did not inspect actual final-response metadata. Add assertions that runtime handoff has no model final and the subsequent real model End has legitimate final metadata.

These are test-harness/observation repairs, not product fixes or altered acceptance. Preserve all expectations. Most recovery tests initially depend on the missing checkpoint/resume/schema behavior; their first baseline failure must be labelled interface or prerequisite-behavior RED, not proof that every later recovery assertion independently failed.

## Implementation integration finding

Static integration review found that real UI permission installation waits for a matching model ToolCard. An internal artifact revalidation Read has no such proposal; a TestHost-only permission queue cannot prove that the real prompt becomes visible. The contract now explicitly permits a private runtime-validation origin and dedicated internal queue entry to present the existing PermissionCard, while preserving ordinary ToolCard identity checks and all decision/cancellation/teardown guards. This is a required wiring correction for the accepted current-permission revalidation, not a relaxed permission boundary or evidence that the UI already passed. Its focused regression and independent implementation review remain pending.

Static integration also found that accepted steering changes the persistence actor's assistant-message ID within a still-live delegated execution. Binding every Read/handoff to the initial assistant message would reject valid work after steering. v1.2 defines a private durable execution-to-assistant-segment association, established by admission and inherited transactionally through accepted steering. Real call/message provenance remains mandatory; a later manual message cannot acquire ownership just because it is in the same thread. A focused steering → Read → handoff regression supplements the original 17 cases. These are implementation findings and additional risk-based checks, not new experiments or a repeated RED cycle; cloud outcomes are pending.

The original 17-case file is an exact byte prefix of the candidate; its original SHA256 remains `4de8cf17a4593799f4242ae69deda6390250a705e1067a2ae930720c036d47d1`. Appending only the steering-test module wiring changes the complete file SHA256 to `3072b7e3434222ad11c6b58a47a3687e80f808cbf4e2cabc47149f4f5a8a31e3`. The three permission additions exercise the real queue/presenter eligibility and Read decision boundary; they are not a claim of GPUI user acceptance. Together the task now has 21 cases, plus the 10 existing #570 cases, before any cloud discovery/execution count is obtained.

Before publishing the production candidate, main-agent static integration caught an undeclared `sha2` import in `vega_tools`, which already has a private raw-byte SHA256 facility. The original implementer corrected that dependency-level wiring by reusing the existing implementation, without adding dependencies or changing hash semantics. This is implementation rework before cloud execution, not an additional experimental or CI attempt.

The stopped production/test candidate covers 33 code paths. Its manifest is the UTF-8 concatenation of `SHA256`, two spaces, repository-relative path and newline for each path in lexical path order; manifest SHA256 is `1bf2c933ca811ace21755f4d2494e98418d5c9b54dd5aefa1dc6094ed683f12b`. The main agent independently recomputed it after the dependency repair and before committing. Documentation is outside that code manifest; independent review and Actions bind to the full published commit, with their final result recorded in PR #572.

## First implementation review and cloud attempt

Candidate `ee05bba185392d7606c6178814a342d802c6a0f4` was reviewed while its existing Actions ran, with no code changes during review. [Actions 39888493764](https://github.com/puzige/vega/actions/runs/39888493764) ran 2026-10-23T02:14:35Z → 02:17:12Z, 157 seconds of wall time. The actual integration checkout was `0d21e1754aa5c08c1c9354272036428bb52d9f62`, merging that head into `f3eab9827dcd1ddb459088410d6f1eedcbce8864`. Clippy job `120202268303` and Nextest job `120202268656` both stopped on the same three undeclared `json!` macro uses in `checkpoint_definition`; this is an implementation compilation error, not a runner failure or behavioral RED. Format and all 60 Python checks passed. Rust test discovery/execution was not reached; counts are unavailable, not an empty-suite success. The status query began at 02:20:16Z under the five-minute cadence while independent review continued; that observation interval overlaps review and is not added as a separate task phase.

The reused independent reviewer verified all 33 manifest paths and rejected the full candidate for two additional behavior gaps:

1. Explicit error branches retained the lock, but direct destruction of the lifecycle/provider/release future had no armed guard. After the actor's temporary owner clone disappeared, history or ordinary preparation could acquire the lock without confirmed host release. Add an ownership-lifecycle guard and verify provider-pending and release-pending drop against history, a new Supervisor and ordinary preparation, with no normalization or replacement dispatch.
2. Resume set its admission flag only after an Ok acknowledgement. Native host cancellation could win after the UI had accepted ownership but before the caller observed the acknowledgement, skipping cleanup and releasing the lock. Preserve accepted/uncertain ownership through cancellation, await cleanup or quarantine, and send a terminal failure/interruption snapshot rather than Running-as-Finished. Verify the accepted-before-ack cancellation window.

These findings preserve R4/R6/R9; v1.2 now explicitly describes the guard and admission-ack boundaries. The original implementer receives one combined repair covering both behavior findings, the macro import and necessary regression coverage. No unchanged CI rerun, new research trial, additional agent or test deletion is used to obtain a pass.

## Combined lifecycle/admission repair candidate

The original implementer handed off the repair and stopped writing before the retained clock at 2026-10-23T02:39:50Z. It adds an armed lifecycle guard, explicit Accepted/Rejected/Unknown admission ownership, native acknowledgement draining and terminal-only host/UI release, and fixes the three macro calls. Three focused cases supplement the original 21 task cases: one native lifecycle test exercises provider-pending and release-pending future destruction; two real NativeSubagentHost transport tests cover accepted acknowledgement/cancellation and uncertain outcomes. These host tests do not claim GPUI manual acceptance. Source expectations were not weakened.

The 10 repair paths' manifest SHA256 is `7a43950aaa1f1067781de984a266b4b5f952faa1ed9355cb3801602f11a99a54`; the full 39 task-code paths relative to the current master have manifest SHA256 `14b4a5ffef067622ec937a9886ae06a93de448e0c1244f5affc1c32ea82feca9`, using the same sorted-path manifest format. Main-agent recomputation matched before and after integrating #573. The original 17-case byte prefix remains unchanged; the complete file including supplemental module wiring is now SHA256 `12c08c6328d0bbe52119baeab1a7f423307328049d3ffb0becdde92de093894a`. Static `cargo fmt --all -- --check` and `git diff --check` passed; no local compilation or tests ran.

After #573's upstream cleanup, this candidate has 24 task development cases and no longer includes the 10 upstream cases. Cloud discovery/execution counts have not yet been obtained. Incremental independent review, development verification, source/patch archival, task-test cleanup and final exact-head empty-suite required CI are pending; the final empty suite will not replace the development results.

## Time accounting

This corrective continuation began before the first retained clock observation at 2026-10-23T00:36:32Z; an exact original turn-start timestamp has not been obtained. Do not reset the work clock to that observation or invent a total. Record cloud run creation/completion times and review/repair intervals from actual receipts. Keep parallel work separate from wall time and retain unsuccessful attempts.

No paid native delivery trial has been started for this runtime change. Passing deterministic tests will establish the stated mechanism only. A faster error, budget stop or incomplete checkpoint is not a reviewable artifact or final delivery milestone.

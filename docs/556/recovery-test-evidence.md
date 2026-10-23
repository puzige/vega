# Stage handoff and recovery: validation ledger

Status: the runtime candidate has been handed off and stopped for integration. The first cloud run executed 27 Rust cases and exposed the old timer cancellation. Independent implementation review and the candidate's cloud execution remain pending; this ledger's candidate-stage status is historical, with final results linked from [PR #572](https://github.com/puzige/vega/pull/572).

## Fixed contract and baseline

- Contract: [stage handoff and recovery v1.2](stage-handoff-recovery.md); the published tests/spec head initially used v1.1. v1.2 records internal-permission and steering integration corrections without weakening R1–R10.
- Original production baseline: `4c0d225daf47e66e559752ee78ba067520bea080`. Latest master `f3eab9827dcd1ddb459088410d6f1eedcbce8864` was merged without conflicts as `03c1296aefb346e6dc76f04e0d3c930b00463c04` before continuing the coupled recovery implementation; the first timer-removal patch and store drafts were retained.
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
| Candidate regression verification | Not run. Original 17 cases are retained byte-for-byte, with 3 permission/Read cases and 1 native steering case added for integration findings. |
| Independent implementation review | Not run. |
| Final exact-head required check | Not run. |
| Merge and installed application | Neither claimed. |

## First cloud baseline and current policy

The draft [PR #572](https://github.com/puzige/vega/pull/572) first published the tests/spec head `928cf828e80e24593c79c36f42869ac555cc09f6`. Actions tested integration merge `a191c6098a7f175274b733fecd90ab82f705b185`, merging that head into `d4a65a381d95b6bb8e4ff8d2858a11a1042ffa3a`. Its 27 Rust cases include the 17 new recovery cases and the 10 storage-contention cases from #570; this was not a pure checkout of the original `4c0d225d` baseline.

Run creation → completion was 2026-10-23T01:16:32Z → 01:19:20Z, 168 seconds of wall time. Clippy/fmt job `120184747733` completed at 01:18:05Z; Nextest job `120184747938` at 01:19:15Z. The jobs overlapped and their durations are not added to the wall time. The test `elapsed_active_time_never_cancels_native_child_or_repeats_provider_work` failed with “elapsed active time must not cancel a native child.” Most deeper recovery assertions did not execute because their new interface or prerequisite behavior was absent. No unrelated compilation or runner failure was used as behavioral evidence.

That baseline run followed the policy checked out when it was started. The newly integrated #569 policy removes mandatory TDD, test freeze, GREEN-then-delete and empty-suite delivery requirements. The 17 useful recovery cases and the 10 existing storage cases remain; there is no new RED or deletion cycle for this policy change. Necessary test/observation repairs must still preserve acceptance and record their cause. Independent implementation review, the latest-head full required check and user manual acceptance remain separate requirements.

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

## Time accounting

This corrective continuation began before the first retained clock observation at 2026-10-23T00:36:32Z; an exact original turn-start timestamp has not been obtained. Do not reset the work clock to that observation or invent a total. Record cloud run creation/completion times and review/repair intervals from actual receipts. Keep parallel work separate from wall time and retain unsuccessful attempts.

No paid native delivery trial has been started for this runtime change. Passing deterministic tests will establish the stated mechanism only. A faster error, budget stop or incomplete checkpoint is not a reviewable artifact or final delivery milestone.

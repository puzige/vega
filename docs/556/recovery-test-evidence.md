# Stage handoff and recovery: validation ledger

Status: specification and independent architecture review complete; 17 native acceptance tests handed off and are receiving pre-RED observation corrections. No new production implementation or cloud result is claimed at this point.

## Fixed contract and baseline

- Contract: [stage handoff and recovery v1.1](stage-handoff-recovery.md).
- Production baseline: `4c0d225daf47e66e559752ee78ba067520bea080`.
- Branch: `feat/556-stage-checkpoint-recovery`.
- User instruction: remove the default 20-minute cancellation, implement durable artifacts/handoff and explicit interrupted recovery, then reuse and verify the existing #522 comparison. Retain independent review and CI; earlier termination is not faster delivery.
- The original implementation agent and original non-implementer reviewer are reused. No additional planning, testing or research agent was spawned.

## Architecture review before production changes

The independent reviewer found three concrete boundaries before test freeze/RED: all delegated recovery-normalization entry points must use the same OS ownership lock; ReadState is not a complete authorized call-bound Read receipt; and ready handoff needs a typed runtime-origin terminal outcome rather than fabricated model final metadata. Acceptance v1.1 incorporates all three. This is preparatory specification review, not implementation approval.

## Evidence states

| Stage | Exact content and result |
| --- | --- |
| Local static checks | Initial 17-test patch rustfmt/diff checks passed. No local Vega build, run or tests. |
| Independent test review | Initial stopped version reviewed; three fixture/observation corrections required before Actions. Incremental recheck pending. |
| Existing Actions baseline result | Not run; no RED claimed. |
| Runtime implementation | Not started. |
| Same acceptance verification | Not run. |
| Independent implementation review | Not run. |
| Final exact-head required check | Not run. |
| Merge and installed application | Neither claimed. |

The existing [#522 comparison audit](../522/existing-study-audit.md) was independently verified read-only from already completed evidence with zero new model calls. It is separate from this runtime's correctness and speed evidence. Historical #561's 28-test result and final empty-suite gate remain in [their original ledger](test-evidence.md); they do not validate this revised behavior.

## Pre-RED corrections and rework

The initial tests-only handoff contained 17 native tests, SHA256 `d81812b0c51bda321c75a60a18ac33c24f23c437833ef26d13252797b210f272`. Production still matched the baseline apart from conditional test wiring. Independent review identified these observation faults before any cloud run:

1. The shared fixture put database checkpoints inside the test project, so the existing safety layer would reject the Write positive control before the intended assertion. Separate workspace and database/checkpoint roots; keep the safety rule and assertion.
2. The mixed-ready test cancelled immediately after the checkpoint event, potentially before the later Write could dispatch. Observe the next provider request, actual permission request or release event before cancelling. Replace the unrelated one-second release deadline with event competition and an outer safety timeout.
3. The handoff/followup test did not inspect actual final-response metadata. Add assertions that runtime handoff has no model final and the subsequent real model End has legitimate final metadata.

These are test-harness/observation repairs, not product fixes or altered acceptance. Preserve all expectations. Most recovery tests initially depend on the missing checkpoint/resume/schema behavior; their first baseline failure must be labelled interface or prerequisite-behavior RED, not proof that every later recovery assertion independently failed.

## Time accounting

This corrective continuation began before the first retained clock observation at 2026-10-23T00:36:32Z; an exact original turn-start timestamp has not been obtained. Do not reset the work clock to that observation or invent a total. Record cloud run creation/completion times and review/repair intervals from actual receipts. Keep parallel work separate from wall time and retain unsuccessful attempts.

No paid native delivery trial has been started for this runtime change. Passing deterministic tests will establish the stated mechanism only. A faster error, budget stop or incomplete checkpoint is not a reviewable artifact or final delivery milestone.

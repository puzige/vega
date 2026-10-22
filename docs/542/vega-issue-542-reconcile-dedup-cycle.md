# #542 reconciliation/deduplication boundary cycle

## Reason and exact starting version

This is an additive TEST-FIRST boundary cycle required by execution-guide §7.2, not a retrospective test for a completed deduplication repair. The original frozen v2.1 contract and 22 tests remain required; no expectations are weakened or removed.

- Original implementation reviewed: `7acb4bc225eb83184a3f7436b2afca3b7ddd2a97`.
- Current prerequisite-repaired baseline: `f52a1e7f0b599dfc6f80611a43be7223df7ca929`.
- Current master baseline: `6615748f91273bfb8ee424011b3b5a09b3777d2b`.
- The prerequisite repair adds only the required focus-out event callback parameter and test-only `InputEvent` trait import, plus evidence. It does not change the defective deduplication implementation.
- Independent implementation review identified this boundary before any deduplication repair. The parent confirmed the service/controller/group call chain.
- PR #564 remains draft. Original RED evidence and unexecuted behavior limitations remain in the [cloud evidence](vega-issue-542-cloud-evidence.md).

## Defect and authority boundary

Real artifact reconciliation can update an existing sealed card's display label after a rename. Controller metadata delivery updates that same UI entity. `ArtifactGroup` currently caches its initial label key and only observes errors, so subsequent capture matching uses stale presentation identity. Capture `a.txt`, reconcile its projection to `b.txt`, then capture another edit of `b.txt` in the same turn can leave two rows displaying `b.txt`.

Labels are escaped full relative-path presentation strings, not basename or filesystem authority. Neither the regression nor repair may derive an open path or capability from a label. The latest real sealed card entity supplies preview/open requests. Capture validation, provenance, Git checks, cancellation, route/history fences, capacities, and six-target allowlist remain unchanged.

## Additive observable acceptance

1. A captured entity may receive a current projection label through its existing public `apply_metadata` boundary, while retaining its sealed card ID and receiving current file capability metadata.
2. When another capture for that current full label joins the same turn, the finished conversation group contains exactly one file row, not duplicate visible names or stale overflow accounting.
3. The surviving row uses the latest capture's optional size and typed preview card/file capability. Older size and card capability must not be dispatched from that row.
4. Existing distinct-file ordering, turn isolation, active buffering, terminal/Summary placement, errors, menus, keyboard/disabled states, and all original 22 regressions remain mandatory without expectation changes.

This refines the existing same-file/latest-capability requirement for the discovered metadata-change boundary. It does not authorize new product UI, alternate dedup semantics, test coverage expansion for reporting, or any safety relaxation.

## Test and observation route before repair

Add one focused GPUI regression to the existing UI test module, reusing its no-I/O fixtures, real `ConversationStream`, actual painted group text/row count, and actual typed preview event subscription. Use distinct synthetic sealed test card IDs for the original and latest captures. Capture an initial filename, apply a renamed projection through the existing UI entity's public metadata API, deliver the latest capture with that new full label, finish the turn, and inspect the actual painted group plus Preview request. No private group fields, cached vector algorithm, source-symbol assertion, fake rendered text, or mocked deduplication decision.

The production deduplication code must remain unchanged until this regression is independently reviewed and existing draft-PR Actions produces a valid target-behavior RED. Unrelated API/harness/compiler/dependency/runner failures are not valid RED and must be separately diagnosed/reviewed while preserving assertions. Original 22 test bodies/data/assertions remain unchanged; the already-approved import-only harness correction is recorded separately.

## Repair and verification gates

After valid boundary RED, freeze the additive test and make the minimum presentation-key synchronization/coalescing correction, preserving latest sealed capability and deterministic distinct-file order. Use actual current projections or synchronized keys rather than turning label strings into authority. Keep original and additive tests until the combined suite is GREEN in existing Actions. Independently review the exact stopped repair version and affected invariants.

Then preserve the complete raw source/patch/full SHAs and both cycles' RED/GREEN evidence in the PR, remove tests and exclusive support in an independent commit, and require latest-head complete empty-suite gate (0 Rust discovered/0 executed, not behavior tests passed) before formal PR/merge. Local checks remain static/format/diff only; no local builds/tests/Vega/CUA.

## Current state

The dedicated writer authored `artifact_group_merges_capture_after_reconciled_rename` and stopped writes. Code binary-diff SHA-256 against `f52a1e7f0b599dfc6f80611a43be7223df7ca929`: `729407ff84576961bec29e2cd426a253c27f1650e9a1b7437091fdb5531f04e0`. Removing only the additive test restores the entire original file byte-for-byte; all original 22 tests and production code are unchanged. The parent reproduced the diff hash, fmt check and whitespace check.

The nonwriter reviewer approved this test for existing draft-PR Actions RED. It changes the original entity label through public metadata, later captures that new full label with a distinct sealed entity, and requires one painted row plus an exact latest Preview event. The initial size is 100 B, latest size 200 B; the renamed entity retains its original file capability and the subsequent capture supplies generation 2. No private-group algorithm or mocked decision is used. Original specification content reviewed before this status append had SHA-256 `6d77b55cb6cf83c68fd00254be2dcfc2a64e456a426c30abd3a2fc4e852c0515`.

Static inventory is now 23 definitions (13 UI, 6 controller, 4 projection), not runtime counts. Production deduplication repair remains unstarted and unauthorized until valid target-behavior RED. No new RED/GREEN or runtime counts are claimed. Parent owns this specification and Git integration; all test writes are stopped.

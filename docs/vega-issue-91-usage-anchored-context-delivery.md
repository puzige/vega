# Issue #91 — delivery record

Spec: [usage-anchored context accounting](vega-issue-91-usage-anchored-context.md). Issue: https://github.com/puzige/vega/issues/91.

## Freeze

- Baseline master `8c1d874`; isolated branch `feat/issue91-usage-anchored-context`.
- Initial state: U91-1–U91-8 NOT RUN; current results below. Implementation delegated after specification freeze; main owns review, integration and native/provider acceptance.
- Prior invalid_summary/over_limit metadata and the user's screenshot preserved privately in persistent evidence `issue-91-2026-09-20.T3ACti`; readonly backup taken before implementation. No user DB/config edit.
- Scope: A2/A3 usage baseline and safe diagnostic accounting; summary output cap and thresholds retained. #90 tool parameter/error-display fix and #66 keyboard work are separate.

## Results

U91-1 red reproduced in the production runtime loop before implementation. A primary request returns input=500 with cache_read=400, then an actual owned-file read appends a tool result. The old whole-history heuristic calls the compaction hook once even though the valid input baseline plus the tail fits. Assertion expected 0 compactions, actual 1; 0 passed / 1 failed, exit 101 (`usage-anchor-red-final.log`). This scripted provider proves the decision bug, not real network behavior. The same production regression passed after implementation: 1 passed / 0 failed (`usage-anchor-green1.log`). This is the first targeted green only; boundary, workspace, real-provider and native acceptance remain pending.

## Implementation review and targeted checks

- Primary input anchors retain a tagged SHA-256 identity of the exact covered request prefix, model, tools and reasoning identity; no second long-history copy is kept. Prediction is normalized provider input plus the newly appended wire content estimate. Cache/output counts are not added again.
- Incomplete, malformed, cancelled, failed, missing/zero/inconsistent-usage responses cannot establish an anchor. Matching-prefix arithmetic overflow fails closed. Runs without a configured context budget retain their legacy request behavior.
- Compaction replacement retires the anchor; independent revisions prevent a later fallback from being rejected as an older UI event. Controller/thread/model/message guards and asynchronous projection fencing protect live accounting.
- Summary input preflight now returns `SummaryInputOverLimit` with its input budget; post-summary projection still returns `ResultOverLimit`. Both preserve the existing durable `over_limit` code. Local UI copy no longer claims provider-confirmed capacity exhaustion.

Targeted evidence (all exit 0 unless red is explicitly noted):

| Coverage | Result | Private log |
| --- | --- | --- |
| Runtime accounting, identity, overflow, fallback, large tail, malformed terminal | 8 passed / 0 failed | `accounting-runtime-green-final.log` |
| Live UI ownership, stale reload and fallback revisions | 1 passed / 0 failed | `accounting-ui-green-final.log` |
| Conversation event conversion | 1 passed / 0 failed | `usage-anchor-conversation-green2.log` |
| App model ownership retirement | 1 passed / 0 failed | `accounting-app-owner-green2.log` |
| Summary stage distinction | red 0 passed / 1 failed (exit 101), then green 1 passed / 0 failed | `summary-stage-red.log`, `summary-stage-green.log` |
| Strict Clippy for runtime/conversation/UI/app, all targets | passed | `accounting-affected-clippy2.log` |

No new dependency or migration. Summary cap 8192, trigger 80%, target 60%, output reserve and tool authority remain as specified. First request/reopen/manual compaction still use labelled estimates because old database usage cannot prove reconstructed request identity. The card does not eliminate every genuine summary failure. Full workspace gates, owned real-provider validation, package/native acceptance and integration remain pending.

## U91-7 owned real-provider acceptance

The temporary harness called the production conversation entry point with the existing configured provider, an owned temporary database/project and a synthetic historical prefix. The model used the real read tool on an owned file and returned its final sentinel. It made two primary requests and zero summary requests. No non-read tool was called; the file and every field of the original two fixture message rows remained unchanged. No checkpoint was installed. The harness observed requests/events without rewriting them and was archived outside the worktree, then removed from source.

| Boundary | Raw estimate | Provider input | Effective decision |
| --- | ---: | ---: | --- |
| First primary | 41,170 | 15,554 | Estimated, below trigger |
| Second primary | 49,908 | 28,467 | UsageAnchored: 15,554 + 8,738 = 24,292 |

Budget was frozen before network responses: input 53,802; trigger 43,042; target 32,281; output reserve 8,192. The second raw estimate would trigger compaction, while both anchored prediction and subsequent actual provider input remain below the trigger. Prediction undercounted actual input by 4,175 tokens; this is measured approximation, not exact tokenization. Real run: 1 passed / 0 failed, exit 0, 30.54 seconds. Compile and offline preflight passed before the single real run. Evidence: `owned-real-diagnostic-result.json`, `owned-harness-real-first.log`, `owned-real-accounting-10606.log`; final harness and hashes are preserved privately.

This validates avoidance of premature compaction through the real production provider/tool path. True threshold crossing is covered by deterministic production tests; no real-provider positive-compaction claim is made for this fixture. Native packaged acceptance remains pending.

## Full-gate attempts

- Final `scripts/cargo-lock.sh fmt --all -- --check`: exit 0 (`final-fmt.log`).
- Final `scripts/cargo-lock.sh clippy --workspace --all-targets -- -D warnings`: exit 0 (`final-clippy.log`).
- First full workspace test attempt stopped in the app crate: 181 passed / 1 failed, exit 101. Existing `tests::r69::issue74_repeated_first_send_pins_once` timed out waiting for its terminal state at `r69.rs:665`; original log `final-workspace-tests.log` retained.
- Exact isolated rerun passed 1/1 in 0.40 seconds, exit 0 (`issue74-gate-isolation.log`), without source changes. Complete workspace rerun passed: 1,652 passed / 0 failed / 9 ignored across 35 result summaries, exit 0 (`final-workspace-tests-retry.log`), without source changes. The original timeout remains disclosed as an intermittent gate failure.

## Packaged candidate and native continuation

Candidate code commit `7fa10a0`. `scripts/cargo-lock.sh xtask package` passed, exit 0 (`final-package.log`). Installed executable SHA-256 matches the package: `2092a2d8d55004bd5581ddaeaf82cc2cc54528b4eb706346af28487a1883c8bb`; `codesign --verify --deep --strict` passed. Before installation, a read-only check found no active messages; the previous App and a fresh read-only database snapshot were saved outside the worktree. The installed process exited normally before replacement.

The candidate opened the latest reported failed thread successfully. Its existing failure history remains visible with the revised local-budget wording. `native-before-continuation.png` is saved, inspected and hashed in the manifest; it is not a successful continuation screenshot. The original thread's four messages and 108 tool records are captured for post-run field comparison. Owner has been asked for one harmless manual submission because GPUI synthetic Composer input is unreliable. U91-8, restart verification, integration and closure remain pending.

## 2026-09-21 failed owner acceptance and corrective amendment

Owner reports another failure in the original thread after further work. Read-only snapshot `sept21-failure.db`, original owner screenshot `sept21-reported-failure.png`, and content-free `sept21-failure-metadata.json` are preserved privately. The current installed executable is `76ea301e53df0cd2e577da146e48623e25e60cf88b2aae2bee843312e35dd642`, matching the master checkout's package and lacking #91's local-budget copy. It replaced the prior #91 candidate. Therefore this attempt cannot be counted as acceptance of `7fa10a0`. No attribution of who replaced it is inferred.

The latest durable attempt is `invalid_summary`, source revision 350, estimated input 244,158, input budget 300,000, target 180,000, no checkpoint. Six summary outputs: 3,689 / 3,733 / 5,813 / 5,699 / 7,294 / 8,192 tokens. The final stop reason was not persisted, so truncation is a hypothesis, not a recorded fact. Code review identifies an uncovered structural risk: each stage resubmits and rewrites the entire prior summary under the same cap. Run-local usage anchoring cannot protect the first request after reopen from this path. U91-8 remains unaccepted.

The main agent froze U5 before delegating the correction and expanded real acceptance to first-request, multi-stage compaction and reopened continuation. No private database/config edits and no cap increase are authorized. The branch was rebased onto `bf43965` to retain the independently delivered model-picker and bash diagnostics changes; an export-list conflict was resolved by retaining both sets of exports. Rebased #91 commit is `c626294`. Delivery changes were backed up before rebase and restored. New gates, build identity and evidence are required.

### U5 red/green progress

The owned long-source regression failed on the old rolling implementation with `InvalidSummary`, six recorded stage usages, exit 101 (`sept21-independent-red.log`). Its fault-injection provider was strengthened to detect the actual prior returned summary body, independent of the wrapper wording. The strengthened test was run against the old `compaction.rs` again and failed at the sixth stage (`sept21-independent-red-strengthened.log`); the saved new implementation was restored exactly. This scripted provider models accumulation-dependent output exhaustion and is not evidence of the historical remote stop reason.

The initial new independent-stage implementation passed the production compaction regression (`sept21-independent-green1.log`), including reconstruction of the complete large original output from stage requests, ordered segment facts, per-stage bounds/usage and one final checkpoint. Additional boundary regression, real provider, full gates and renewed native acceptance remain pending.

Diagnostic limitation: the app currently has no tracing subscriber initialization. New content-free tracing stage fields are observable when a subscriber is installed; this card does not claim persistent native diagnostic logs. A typed `SummaryOutputTruncated` error carries visible/thinking byte counts and optional output usage to callers, while preserving the durable `invalid_summary` mapping. Real-provider acceptance observes the unchanged stream directly for stop reason and byte counts.

### U5 targeted gate handoff

Final affected conversation-library run: 432 passed / 0 failed / 3 ignored, exit 0 (`sept21-conversation-lib-green2.log`). Issue91 cases: 4 passed (`sept21-issue91-green2.log`); runtime/conversation strict all-target Clippy passed (`sept21-affected-clippy.log`); format/diff checks passed. An earlier library attempt failed one existing prompt-contract assertion for the literal phrase `exact paths`; the prompt was corrected to retain that explicit requirement, and the original failed log (`sept21-conversation-lib-green.log`) is preserved.

Coverage includes old checkpoint inclusion exactly once, unchanged raw records and predecessor on aggregate target rejection, the aggregate byte bound, complete ordered source-stage coverage, and genuine later Length/cancellation retaining prior usage with no commit. New source diagnostics and `SummaryOutputTruncated` retain content-free counts, not text. Main review found no remaining production-code issue at handoff. Temporary owned multi-stage acceptance is next; no final green/native claim yet.

### U91-11 first real attempt — retained as FAILED

Compilation and offline preflight passed. The first real run made four independent summary calls (all End, outputs 1,786 / 2,429 / 1,409 / 1,596 tokens) and two primary calls (both End). Both primary answers preserved the four required historical facts; one checkpoint, all original message/tool fields, owned files, and exact-once source coverage checks passed.

The test nevertheless exited 101 after 46.87 seconds: an overbroad observer assertion rejected historical tool IDs even when mentioned as ordinary summary text. U5 explicitly preserves relevant exact identifiers; only structured tool replay is forbidden. At the point of failure, the first primary's exact checkpoint wrapper/no-Tool-role checks had passed, but the second primary's wire assertions had not run. This attempt is not accepted/green. Evidence `sept21-owned-multistage-result-first.json` preserves the failed log and executed harness hashes.

Main review authorized correcting only the test boundary: distinguish normal text mentions from Tool roles, `tool_call_id` and `tool_calls`; verify both primary requests. No production code, fixture, budget or provider data changes are allowed to make the retry pass. A second independently logged real run is required.

### U91-11 second real attempt — PASSED

The corrected observer ran the same fixture/budget against unchanged production/provider code: 1 passed, exit 0, 44.26 seconds (`sept21-owned-real-second.log`). Four independent summary calls all returned End, input usage 11,525 / 11,484 / 11,405 / 11,368 and output usage 1,139 / 2,200 / 1,474 / 1,461. Both primary calls returned End and correctly recalled the early project/read-only constraints, parser failure and later TSV correction.

Actual primary input usage was 4,192 then 4,323. Both wire requests contained the exact production checkpoint message once, zero Tool-role messages, zero structured `tool_call_id` values and zero structured tool calls. Each contained 29 textual historical ID mentions, all within the checkpoint body and none elsewhere; this confirms why the previous prose-ID assertion was invalid. All 48 source markers appeared in exactly one summary request; all fields of the four original messages and 48 tool records, 48 owned files and the single checkpoint remained unchanged, including after fresh-Store reopen.

Evidence/hashes: `sept21-owned-multistage-result-second.json`, final harness `sept21-owned-multistage-harness-second.rs`, and explicit assertion-correction patch. Both failed first attempt and passed second attempt remain archived. Temporary source/module were removed. Full workspace gates, packaging and native original-session acceptance must still pass on this final revision.

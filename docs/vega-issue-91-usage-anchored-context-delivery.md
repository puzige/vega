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

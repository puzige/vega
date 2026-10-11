# Issue #302 — bounded Shell output in model context

## Problem and evidence

Issue: https://github.com/puzige/vega/issues/302. The owner requested investigation and repair on 2026-10-11. The repository baseline is the current master fetched before creating this task branch.

Read-only incident evidence matches the screenshot: a successful Shell call returned 2,622,078 bytes in 69 lines, including 40 line-truncation markers. The next primary preflight predicted 1,006,002 input tokens against a 300,000-token input budget and 180,000-token compaction target. The preceding provider call reported 131,289 input tokens. Compaction failed locally within 58 ms, before a summary provider request. Earlier compactions had succeeded.

The Shell collector intentionally retains up to 8 MiB for display/audit, including up to 64 KiB per line. Runtime currently replays that entire retained output to the model. Compaction preserves the newest user turn and its tool results, so it cannot remove this oversized live suffix. The error is a local context overflow, not evidence of a failed Shell/JADX process or provider outage.

## Contract

1. Separate Shell display/audit output from its model projection. A `bash` result's provider-facing text is at most 65,536 UTF-8 bytes, including an explicit middle-omission marker. Retain deterministic head and tail portions without splitting UTF-8 characters. The marker tells the model output was shortened and to narrow/filter or page the command when more detail is needed. Small output is byte-for-byte unchanged.
2. Preserve the original collected result in runtime tool events, completed-call records, SQLite and tool-card display. Do not alter the collector's existing resource limits, execution status, exit code, cancellation, permission or credential-redaction behavior. Projection happens after credential redaction. Manual/hook history reconstruction must check known/pattern-sensitive credentials in the raw Shell result before shortening it, so a secret crossing a cut boundary cannot evade the existing guard. Check only results eligible for the reconstructed history, not rows already excluded by the checkpoint. A complete existing credential-redaction marker is safe and must remain idempotent; recognizing it must not skip genuine credentials elsewhere. This check is in memory and must not rewrite raw history. No user database/configuration mutation, dependency or migration.
3. Use the same production projection for fresh Shell results, completed-call reuse and history reconstruction. Reopen, retry, manual/automatic compaction and source-fence validation must all see the same bounded messages. Keep tool call/result identities and ordering intact. Other tools, structured mutation results, MCP and images retain their existing contracts in this fix.
4. Compaction continues to retain the newest user content and the already-projected live suffix exactly. This explicitly amends the exact raw Shell replay assumption in Claude compaction parity C1/C3 and issue #91 U3; it does not authorize arbitrary truncation of user messages, summary text, other tools or history rows. Summaries consume the bounded projection, with omission made explicit.
5. Keep the configured trigger/target/input limits and fail closed on remaining genuine over-budget requests. The per-result ceiling prevents this incident's single large-output spike; it is not a promise that unlimited tool rounds or intrinsically oversized user input will always fit.
6. Preserve typed local context-budget failures through run diagnostics and user-facing error classification where the current path instead emits unknown runtime failure. Cover OverLimit, SummaryInputOverLimit and ResultOverLimit. Do not claim an actual provider capacity error or suggest checking the execution environment for these local failures; unrelated failures retain existing classification.

The 65,536-byte ceiling contributes at most 21,846 estimated text tokens under the current byte estimator, plus framing: ceil(65,536 / 4) = 16,384; ceil(16,384 × 4 / 3) = 21,846. It replaces approximately 874,027 estimated text tokens for the incident output while retaining the original display/audit record.

## Acceptance matrix (before implementation)

| ID | Risk / operation | Expected observation | Level | Initial status |
| --- | --- | --- | --- | --- |
| B302-1 | Single Shell result with many very long JSON lines, about 2.6 MB | Existing code fails red; fixed production loop sends bounded result to next primary call and completes under the 300k budget; tool event retains collected result | In-process provider/tool fixtures | NOT RUN |
| B302-2 | At/below/above 65,536 bytes, long single line, multibyte text | Exact small payload; bounded valid UTF-8 head/marker/tail for large payload; idempotent projection | Unit | NOT RUN |
| B302-3 | Reuse completed Shell call; persisted oversized output reopened/reconstructed | No re-execution; same projected text as live path; original stored output and complete call pairing unchanged | Runtime / conversation storage | NOT RUN |
| B302-4 | Compaction source validation and checkpoint-aware projection after bounded Shell output | Projection identity agrees; no false SourceChanged; current user and tool pairing unchanged | Conversation storage | NOT RUN |
| B302-5 | Genuine hard budget failure in each affected typed path | No oversized primary send; persisted run failure and user copy identify local context budget, with no raw content | Runtime / conversation | NOT RUN |
| B302-6 | Small Shell output and non-Shell structured/tool results | Existing content/status/permission/cancellation behavior unchanged | Focused regressions | NOT RUN |
| B302-8 | Historical Shell credential crossing a projection cut boundary during manual compaction; already-redacted assignment and checkpoint-excluded row | Existing credential rejection occurs before shortening; no provider request, checkpoint or raw-row edit on rejection; safe placeholders and excluded rows do not falsely block | Conversation storage / mock credential reader | NOT RUN |
| B302-7 | Owner resumes original affected conversation on merged application | Continues without replaying the giant Shell result in full; narrower queries remain available | Owner manual integration | NOT RUN |

## Implementation and delivery plan

Main agent owns this spec, review and integration. Dedicated implementation agent owns the shared model-projection helper, runtime callsites, conversation reconstruction, raw Shell credential guard, the narrowly required complete-marker idempotence correction, failure classification and directly related tests. Freeze this spec before implementation; add the incident regression and retain its red output before production changes.

Use the isolated issue branch/worktree and default Cargo target per the repository delivery skill. Run only focused `cargo nextest run -p vega_runtime ...` and `-p vega_conversation ...` tests for this card and affected existing paths. No real network/process E2E and no local workspace-wide suite. Retain bounded original test output and hashes in `docs/vega-issue-302-context-budget-delivery.md`.

Open a PR referencing the issue without auto-close keywords; wait for required cloud checks and merge when green. Then set the card In review and wait for owner manual testing. Do not close the issue, clean its worktree or install/interrupt the running application before the relevant authorization and manual acceptance. Rollback is reverting the change; original audit rows require no restoration.

# Issue #117 delivery

Contract: [compaction stream specification](vega-issue-117-compaction-stream.md).

## Implementation

Compaction is a semantic conversation list item. The first accepted lifecycle
event closes the active Markdown segment and appends one row; subsequent accepted
events update that operation in place. Existing list splice/remeasure handling
preserves detached scrolling. The fixed Composer status band and six-transition
eviction are removed. Status rows contain no summary content and do not enter
the provider transcript. Existing generation/model/thread fences remain.

Reopen uses the existing latest durable status projection. Since legacy records
have no transcript anchor, the restored row appears after loaded history with an
explicit “上次” label. This does not reconstruct all prior operations or claim an
exact original position. An active/new operation rejects late restoration; the
controller also retains its owner/load-sequence fence. No schema, provider,
budget, compaction algorithm or dependency change.

## Verification

Local evidence is retained outside the worktree in the private `issue-117`
evidence directory. Intermediate compilation failures are preserved along with
the original red test; they are not counted as passing runs.

| Requirement | Evidence | Result |
|---|---|---|
| C1 regression | `cargo test -p vega_ui issue117_context_status_is_a_conversation_item` on old production code | RED, exit 101: old fixed status band still exists |
| C1–C6 stream | `cargo test -p vega_ui conversation_stream::context_control::tests` | PASS, 7 tests, exit 0 (`green-context-ui-5.log`) |
| C2/C4/C5 controller | `cargo test -p vega context_compaction` | PASS, 15 tests, exit 0 (`controller-tests.log`) |
| Formatting | `cargo fmt --all -- --check` | PASS, empty output |
| Strict lint | `cargo clippy -p vega_ui -p vega --all-targets -- -D warnings` | PASS, exit 0 (`clippy.log`) |
| Broader stream regression | `cargo test -p vega_ui conversation_stream` | PASS, 260 tests, exit 0 (`stream-regression.log`); includes the 7 context tests above |
| Candidate package | `cargo xtask package`; strict deep code signature verification | PASS, exit 0 (`package.log`) |
| Native recovery and scrolling | Open existing conversation in installed candidate; scroll up and return to tail | PASS: one historical status row, row scrolls away with transcript, no fixed band |
| Live native compaction, C7 full appearance matrix | Owner manual acceptance | NOT RUN by agent; do not infer from recovery screenshot |
| Cloud PR check, integration, cleanup | Pending | NOT RUN |

Tests assert actual before-text / compaction / after-text / tool order, one row
per generation, terminal non-regression, more than six operations without
eviction, detached follow state, historical restore/idempotency and late/foreign
restoration rejection. The reopened production controller test checks the
restored row and absence of the fixed band. These automated results do not
substitute for native pixel or real-provider verification.

## Residuals

The owner elected to preserve the existing conversation and perform the live
compaction acceptance manually. No new prompt was sent to that conversation.
Live native acceptance, full appearance matrix, cloud check, integration and task cleanup remain outstanding.
Draft PR: <https://github.com/puzige/vega/pull/134>.
The Issue stays open until required acceptance and delivery are complete.
Rollback is a revert of this card's implementation commit; data formats are unchanged.

## Installed candidate and native evidence

Production source commit: `255a063`; subsequent changes only update this report.
Candidate and installed executable SHA-256 both equal
`9ae68d7dc780e4f4120c7404985723c01a102743a7462de23636ec51994a9321`.
The canonical Documents application was updated after checking that no message
was streaming and no tool was running. The previous application is backed up
as a zip with its executable hash outside the worktree.

Private evidence: `candidate-restored.png`, `candidate-scrolled.png`,
`baseline.png`, command logs and `manifest.json`. Screenshots were inspected:
the restored row aligns to the conversation column; scrolling moves it out of
the viewport and returning to the tail restores it. Existing conversation
content was neither edited nor submitted to a provider. These screenshots are
kept local because they include existing conversation content.

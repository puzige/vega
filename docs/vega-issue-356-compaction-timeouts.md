# Issue #356 — Progress-aware context compaction deadlines

Issue: https://github.com/puzige/vega/issues/356

## Authorization and incident

On 2026-10-14 the owner accepted the proposed compaction timeout/error-classification repair and set the idle deadline to **300 seconds** and the operation deadline to **600 seconds**. This specification supersedes the 60-second acquisition-plus-stream request deadline and 180-second operation deadline in `vega-claude-compaction-parity.md` C1, and the conflicting older #76/#88/#91 timeout requirements. The remaining compaction, permission, accounting, and history-authority contracts remain in force.

Read-only incident evidence from v0.1.102 showed three summary attempts failing at approximately 60,000 ms after emitting approximately 7.5–10.6 KB of visible summary text. Diagnostics recorded `summary_timeout`, while the compaction status/UI classified the same failures as invalid summaries. After continued primary calls the original input exceeded its configured budget. A later user turn successfully compacted in approximately 54 seconds. The evidence proves that a fixed total request deadline interrupted incomplete summaries; it does not prove when the last text delta arrived or that every slow request would eventually complete.

## Contract

1. Start one immutable **600-second operation deadline** at the existing compaction-operation boundary. Source loading, planning, summary acquisition/stream collection, any existing input-overflow trimming retries, projection validation, and checkpoint installation share its remaining time. A retry must never restart the operation clock. Do not add timeout retries or change the existing consecutive automatic-failure circuit.
2. Give each summary request a **300-second inactivity deadline**, initially running from provider acquisition. Reset it only after a nonempty, valid `TextDelta`, `ThinkingDelta`, or `SummaryDelta` received before completion. Empty deltas, opaque reasoning replay, metadata, and heartbeat-like events cannot keep a stalled summary alive. Provider acquisition and the first meaningful output share the initial idle interval; obtaining response headers alone must not restart it. Idle waiting is always bounded by the remaining operation time.
3. Keep cancellation responsive through acquisition and collection. Either deadline returns the typed summary-timeout error and cancels the child provider token without cancelling the owning parent token. Timeout, cancellation, missing completion, invalid/truncated/empty summary, changed source/predecessor, and invalid projection never install a partial checkpoint. Preserve exactly-once observed usage and unknown accounting when final usage is absent.
4. Carry timeout separately from invalid-summary failure through runtime, conversation events, durable status, and restored UI projection. The only new visible text is the failed-compaction label **“上下文压缩超时，请重试”**. Existing malformed/empty/truncated summary classification retains its current label. Reopening a new timeout status preserves the timeout label. Do not infer that legacy `invalid_summary` rows were timeouts.
5. If persistence needs schema support, use the next additive migration, preserving existing status identifiers, historical rows, indexes, and old failure values. Do not rebuild/drop existing tables or rewrite raw messages, tool calls, checkpoints, user configuration, or the live user database. This task specifically permits minimal additive timeout metadata, superseding the prior no-schema clause only for this new failure classification. No new dependency or broader UI/configuration change.

## Acceptance matrix established before implementation

| ID | Case | Expected observable result | Evidence/status |
|---|---|---|---|
| T356-01 | Complete summary with meaningful deltas beyond the old 60-second total duration | One successful checkpoint and normal continuation with a complete valid summary | Paused-time provider regression; pending CI |
| T356-02 | Output continues for over 300 seconds with each gap below 300 seconds | Idle timer renews; success before the shared 600-second operation deadline | Paused-time provider regression; pending CI |
| T356-03 | Acquisition, no first output, or stream stops making meaningful progress for 300 seconds | Typed timeout, child cancelled, no parent cancellation or partial checkpoint; empty deltas do not extend waiting | Paused-time fault injection; pending CI |
| T356-04 | Meaningful output continues until 600 seconds, or an overflow retry uses the remaining time | Shared operation timeout ends collection; a retry cannot obtain a fresh 600 seconds | Paused-time production-hook regression; pending CI |
| T356-05 | User cancels during acquisition or streaming | Prompt cancellation, original transcript/checkpoint preserved, observed usage retained once | Existing and targeted in-process regressions; pending CI |
| T356-06 | Persist timeout, reopen; migrate a pre-change database with legacy failure rows | Timeout remains distinct; old rows and their identifiers/failure semantics survive unchanged | Store/conversation restoration regression; pending CI |
| T356-07 | UI receives timeout versus malformed-summary status | Exact timeout label versus existing invalid-summary label, no additional steady-state explanation | Focused UI projection regression; pending CI |

## Implementation and verification plan

The main agent owns this spec, review, integration, and GitHub delivery. One dedicated implementation agent owns the affected runtime/conversation/store/UI source and regressions in the task worktree. Implement tests before behavior changes, then the two deadlines and end-to-end timeout classification. Do not introduce production public APIs solely for tests, add code comments, weaken existing assertions, or change unrelated behavior.

Local verification is limited to fast diff, formatting, and static checks. No local compilation/build/packaging or `cargo check`, Clippy, test, or nextest. Executable feature regressions and the complete gate run in the existing cloud PR Check: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo nextest run --workspace`. No local red/green result is claimed; preserve CI failure evidence if any.

After the required cloud check succeeds, squash-merge and set the Project to In review. Keep the Issue, branch/worktree evidence, and manual acceptance pending until the owner tests a released application. Do not restart or install over an active application as part of this code fix.

## Manual acceptance and rollback

On the released application, continue a long conversation and verify that a progressing summary may finish beyond the previous 60-second cutoff, that actual stalls produce the timeout label, and that reopening retains the distinction. The 300/600-second boundary behavior is primarily proven by deterministic CI tests; manual acceptance does not need to deliberately wait ten minutes. Rollback is a normal follow-up PR reverting behavior while leaving additive migration compatibility intact.

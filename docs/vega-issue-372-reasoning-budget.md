# Reasoning budget lifecycle and truthful failures

## Authorization and evidence

2026-10-14 user explicitly requested issue #372: successful context compaction must allow the same task to continue; reasoning-budget failures must show their actual cause instead of runtime-environment advice. This supersedes historical lifetime-generated-byte accounting in the reasoning contracts only. Preserve the existing numerical stream, turn, retained-window, replay, and UI bounds.

Observed: context summarization succeeded, a later primary-model attempt recorded `reasoning_limit` with HTTP 200, the root diagnostic recorded `unknown_safe_failure`, and the UI displayed the Runtime fallback. The run had 105 primary-model calls. Historical diagnostic metadata cannot identify the violated byte scope; do not invent that scope when restoring the error.

Related contracts: `vega-issue-71-reasoning-summary.md`, `vega-continuous-compaction.md`, `vega-model-switch-summary.md`, `vega-compaction-status-layout.md`, `vega-issue-171-run-diagnostics-spec.md`. Only the authorized budget lifecycle and reasoning-failure classification are changed.

## Behavior

1. Reasoning capacity follows retained live state, rather than all bytes ever generated during the task. After a successful, validated, accepted compaction replaces the live projection, release capacity for covered/discarded thinking and opaque reasoning replay, and charge any retained suffix. Repeated compactions in one run must permit further model/tool rounds while carrying task goals, progress, latest user correction, and tool pairing forward. Do not reset blindly while keeping the same data resident or replayed.
2. Preserve the existing 64 KiB streamed delta, 256 KiB logical-call thinking/summary, 1 MiB retained reasoning-window guard, and existing opaque replay byte/item bounds. Streamed summary and thinking share their relevant budget. No unlimited event history or replay storage as a consequence of resetting accounting. Use existing bounded event/UI behavior or prune acknowledged transient reasoning payloads safely; immutable user/assistant/tool audit, chronology and durable diagnostics must remain intact.
3. Failed, invalid, over-budget, source-conflicted or cancelled compaction does not grant fresh capacity or discard history. Only the accepted live-projection transition is eligible. Provider retry rollback must free discarded attempt content without allowing retained content to evade limits; do not replay partial tools or retry a budget rejection indefinitely. Preserve cancellation, safety, source fences, summary timeouts, usage and frozen model/provider settings. No silent truncation of protocol-required active reasoning/tool chains.
4. Classify `VegaError::ReasoningBudgetExceeded` explicitly throughout runtime, conversation diagnostics, and UI. Root and primary diagnostic records must agree on `reasoning_limit`. No raw provider text, reasoning, credentials or internal paths in user-facing messages or metadata.
5. Live failures and reopened history show one owned actionable error card identifying that model thinking content exceeded a limit. Suggested recovery must fit the actual scope if known; lowering thinking intensity or retrying/continuing from retained context are legitimate actions. Never show environment/credit/network advice for a proven reasoning-limit failure. Do not show a budget failure as successful compaction or create another always-visible explanation/status surface.
6. Restore historical failed rows from same-run durable diagnostic evidence. Legacy root `unknown_safe_failure` plus an owned terminal primary `reasoning_limit` can recover the specific cause. Pending-message delivery can change assistant row identity while the run id stays original; preserve exact ownership across that path. Do not use merely adjacent/latest failures, cross-run inference, free-form string matching, or borrow a reason from a succeeded/retried primary attempt. Keep unknown legacy reasons honest. No direct edits to user DB/config/installed app. Avoid a migration unless existing durable data cannot safely express the necessary ownership.

## Acceptance matrix established before implementation

| ID | Scenario / actual operation | Expected production result | Layer / evidence |
|---|---|---|---|
| RB1 | Mock model emits bounded thinking and tools across rounds; successful production compaction covers earlier thinking; continuation exceeds the old lifetime aggregate | Same task reaches the final answer; continuation request carries goal/progress and correct tool results | Runtime + conversation regression; cloud CI pending |
| RB2 | Same run requires a second accepted compaction | Further rounds succeed; acknowledged thinking/replay remains bounded and no repeated tool execution | Runtime regression; cloud CI pending |
| RB3 | Compaction retains a suffix with reasoning or opaque replay | Retained data is still charged and replayed correctly; over-retention still fails | Runtime regression; cloud CI pending |
| RB4 | Failure/invalid result/cancellation at compaction boundary | No new capacity, no loss of raw audit or fake success/extra model call | Runtime/conversation regression; cloud CI pending |
| RB5 | Exact byte edges, shared thinking/summary, provider retry discards partial attempt, opaque replay | Existing protections hold; accounting tracks accepted/retained state | Focused production regression; cloud CI pending |
| RF1 | Stream/turn/window reasoning violation | Typed cause reaches root and primary diagnostics and one live actionable card | Runtime-to-conversation/UI regression; cloud CI pending |
| RF2 | Persist failure, close store/controller, reopen | Same specific failure on correct failed assistant row, one visible card | Store-to-conversation/UI regression; cloud CI pending |
| RF3 | Legacy unknown root with same-run terminal reasoning failure; row identity changes after steer | Restore correct cause on owning row using durable evidence | Conversation/UI regression; cloud CI pending |
| RF4 | Adjacent independent runs, succeeded primary or unknown historical failure | No cross-run attribution; no hiding unrelated context/provider/tool errors | Conversation/UI regression; cloud CI pending |
| M1 | User updates cloud-built Vega and resumes a long thinking/tool task through compaction | Task continues; actual limit failures display cause and recovery, including reopen | User hand-test pending |

The original failed task and source paths establish the defect evidence. Write regression assertions before implementation where feasible; no local red/green execution is permitted and no unexecuted test is claimed as passing.

## Plan and ownership

One dedicated implementation subagent owns scoped production code and tests in `vega_runtime` reasoning/loop/events, `vega_conversation` diagnostic/restoration/failure types, and the minimal `vega_store`/`vega_ui` projection support required for truthful restored error cards. Main agent owns this specification, review, integration, PR and CI. Do not change other task worktrees, provider configuration, install/runtime state, tool permission semantics, broad UI layout, model selection, or the Markdown-links feature. No new dependencies or code comments.

Local checks: formatting and `git diff --check` only. No compilation, build, package, cargo check, Clippy or local tests. Cloud `pr-check` runs fmt/Clippy and `cargo nextest run --workspace`, including this matrix. Main agent reviews source and assertions, fixes real failures through the implementer, then squash merges only the checked head. Keep issue open/In review and preserve worktree until user hand-test. Rollback: revert this PR through the same checked PR flow; no user data migration should be necessary.

## Validation

Implementation and regression assertions are complete. Local `cargo fmt --all -- --check` and `git diff --check` passed; no local compilation or test execution occurred. Cloud PR Check remains pending at submission; its run is the authoritative execution evidence. Real model/native UI acceptance remains pending until user hand-test.

The implementation reconciles retained thinking/replay at accepted compaction and discarded retry-attempt boundaries, and releases acknowledged reasoning payloads in both runtime and conversation event buffers. Projection rejection uses an exact operation-key diagnostic to recover its typed cause without a schema migration. Historical assistant ownership requires typed delivered-steer evidence; absent or ambiguous legacy evidence stays unknown. Specification deviations: none.

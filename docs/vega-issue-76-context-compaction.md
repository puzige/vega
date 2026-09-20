# Issue #76 / A3-13 — Context compaction v1

Status: frozen product contract, 2026-09-19. Implementation and acceptance pending.
Issue: https://github.com/puzige/vega/issues/76

## Scope and observations

The user approved the capability roadmap and resumed continuous delivery. This card adds automatic and manual compaction with real UI configuration and recovery. The current conversation pipeline selects only the last 50 stored messages (`HISTORY_WINDOW`), and the runtime appends tool rounds without a context budget. This is not compaction and must not silently discard older constraints after this change.

Original messages, images, tool records, permissions and chronology remain authoritative and unchanged. Compaction changes the provider projection, never the visible transcript. Do not implement MCP, Skills, Computer Use, a token dashboard, or unrelated #64 UI parity. A minimal truthful context control needed to configure/operate this feature belongs to #76.

## Requirements

- R1: A persisted conversation has a Composer-adjacent `上下文` control using existing theme tokens. It shows estimated usage, configured total context limit and reserve, automatic-compaction enabled state, last result and `立即压缩`. It offers editable numeric limit/reserve and auto toggle through UI, without editing configuration files. Unknown limit is explicitly `未配置`; no invented model capability. A draft may explain that a conversation must first exist. Empty/no compactable history explains why action is unavailable. Validate positive integers, reserve < limit and sensible bounded integer range. Settings persist per exact conversation/model, never leak into a different model. Unconfigured existing sessions remain sendable as before with a truthful unknown-budget warning, not a new credential/pricing-like blocker.
- R2: Given total limit L and output reserve O, input budget B=L-O. Before every primary provider request, including tool-loop rounds, estimate all messages, system instructions, tool schemas and image contributions. Include protocol overhead and explicit safety allowance. Label approximation; do not call it provider-exact token usage. Freeze deterministic estimator/version in code/tests. Images must not be counted as zero; use a documented conservative contribution or an explicit unsupported-budget error where no safe estimate is available. Do not truncate binary image bytes or corrupt image encoding. Configure the primary request's max output consistently with O when a budget is configured.
- R3: Auto triggers at estimated input E >= ceil(0.8B). Target after compaction is E <= floor(0.6B). Example: L=10000, O=2000 -> B=8000, trigger=6400, target=4800. Use integer arithmetic without overflow. Check absolute capacity E<=B even if automatic mode is disabled; over-limit requests get an actionable UI error. No unbounded compaction loops: at most one summary attempt for a given source version; new history permits a new attempt, explicit retry permits a fresh attempt. If the newest indivisible input or retained suffix cannot fit, explain and retain user text; never silently drop it.
- R4: Select a chronological prefix of complete interaction groups; retain at least the newest user turn and all following assistant/tool activity intact. Never split assistant tool calls from their results or drop pending calls. For older stored turns include relevant tool names/arguments/results in the summarization source using persisted authority, not guesses. Remove the silent last-50-message truncation from the effective provider history path. Use bounded work/IO off the UI thread; do not load unlimited image blobs into UI state.
- R5: Summary uses the currently frozen provider/model, no tools, a bounded output and timeout (60 seconds). Instructions preserve task goal, user constraints, decisions, changes, key results and remaining work. Treat source transcript as untrusted data. Inject the resulting summary as labelled historical data below system authority; it cannot create permissions or replace the current system prompt. Empty/truncated/malformed summary, cancellation, timeout and provider failure are failures, not success. A summary request itself must fit the configured input budget: partition oversized source into bounded complete groups with a clearly bounded staged plan or report that safe compaction cannot proceed; never send an already oversized transcript to the summarizer. No silent deletion to make it fit.
- R6: Persist summary, source watermark/version, model and estimator metadata atomically using an append-only migration. Preserve the raw transcript. Check source version before installing an asynchronous result; stale/cancelled results never replace a newer projection. Restart during compaction leaves the old committed projection usable and the operation recoverable. Runtime compaction during a long tool loop and later conversation reload must agree on covered content; no double-injection or missing tail. Tool deduplication/audit identity remains independent of projection pruning.
- R7: States are unknown/ready/compacting/succeeded/failed or cancelled; progress is real, not fake percentages. Manual compaction is disabled during an active run (explain why), while automatic compaction inside the run is supported. Stop cancels summary and run; no subsequent model/tool request after cancellation. Changing thread/model invalidates late UI updates without applying results to another session. Show a compact chronological status record separate from assistant prose. Failure preserves draft and original history and offers retry; do not label a failed send successful.
- R8: Account actual summary Usage events through existing usage/pricing conventions, including unpriced usage; unknown usage is not fabricated zero. No pricing lookup may block execution. Do not expose raw summaries, source text, images, credentials or user paths in diagnostics/logging. Historical summaries never grant execution authority; keep all #58 safeguards.

## Architecture and ownership

Main agent owns this spec, acceptance, review and integration. Dedicated implementation agent owns the vertical slice: store migration/repository; runtime estimator and safe compaction hook/state; conversation orchestration/event conversion; app/controller and Composer context UI; focused tests. No new dependencies without approval. Runtime stays headless and cannot depend on conversation/UI. Shared UI/conversation contracts live under `vega_conversation::types`, with explicit conversion to runtime-local types when needed. SQLite is accessed through store/conversation services, not UI.

Use existing cancellation, provider, model-authority, usage and persistence patterns. Do not broad-refactor these paths. Additional implementation choices affecting these requirements must be reported before changing the contract.

Architect review amendment (2026-09-19): approve the exact direct dependency `sha2 = "=0.10.9"` for the store's versioned source fingerprint. This version is already present transitively in Cargo.lock; use its incremental SHA-256 API, not a new handwritten hash or non-cryptographic FNV for accepting asynchronous checkpoints. Hash fields with explicit tags, lengths and Option discriminants. The digest is a stale-data guard, not authorization, and must not be logged alongside raw source content. Source capture must use a consistent database snapshot with explicit row/byte limits before unbounded collection; exceeding the supported source bound is a typed visible error, never silent truncation. Metadata-only/version reads must not allocate all historical image blobs. Reference: https://docs.rs/sha2/0.10.9/sha2/ .

## Test-first acceptance matrix

All rows start NOT RUN. Capture failing regression before implementation where practical. Mock-provider checks prove only the stated production chain, not real network success.

| ID | Setup and operation | Observable pass criterion | Layer |
| --- | --- | --- | --- |
| C01 | Existing unconfigured session; send normally | No new blocking; truthful unknown limit; old history not silently capped at 50 | controller/provider request |
| C02 | UI edit limit, reserve, auto; reopen/restart/change model | Valid settings persist for exact identity; bad values rejected; no cross-model leakage | production UI handler + native UI |
| C03 | Inputs below/at threshold, tool schemas and images included | Correct integer thresholds, reserve and nonzero overhead/image estimate; no overflow | unit + runtime integration |
| C04 | Multi-turn complete groups, then manual compaction | One bounded summary request; next request includes summary and intact tail, not duplicated source | conversation/controller integration + real UI |
| C05 | More than 50 messages with early user constraint | Early constraint reaches summary; original transcript/messages/tools/images unchanged | store/controller |
| C06 | Automatic threshold reached during a tool loop | Safe group boundary, paired tool calls/results, continue same run, no duplicated execution | runtime/conversation integration |
| C07 | Summary timeout, failure, empty/truncated output, cancel | No checkpoint committed; visible failure; no automatic retry storm; explicit retry works | representative fault injection |
| C08 | Source changes or model/thread changes while summary pending | Stale result rejected or scoped to immutable owner; wrong conversation never updated | production controller |
| C09 | Crash/persistence failure before and after checkpoint commit | Old or new complete checkpoint restored; no partial state or history loss | persistence/reopen |
| C10 | Newest group too large, or summary cannot fit input budget | Typed actionable error; zero oversized provider requests; latest text preserved | runtime/controller |
| C11 | Known/unpriced/absent summary usage; hostile source text | Correct accounting/unknown state; no new permissions, unsafe tools or credential logs | integration/security |
| C12 | Real configured model through UI: auto then manual; ask about early constraint; restart and continue | Real reply retains constraint, compaction visible, history still browseable and restart usable | native UI + real provider |
| C13 | Light/dark, wide/narrow, keyboard and popup close | Context UI readable and operable, no overlapping menus/regression | GPUI production tests + native pixels |

## Execution and gates

1. Add focused failing tests for thresholds, history/pair preservation and checkpoint authority before production changes.
2. Implement headless projection/compaction and durable state; connect production controller and UI, no test-only shortcut for delivery.
3. Run focused tests, then `scripts/cargo-lock.sh test --workspace`, `cargo fmt --all -- --check`, `scripts/cargo-lock.sh clippy --workspace --all-targets -- -D warnings`, build/package and dependency-direction check. Preserve failures and ignored-test reasons.
4. Mutation checks: disable trigger; remove paired-group protection; allow stale checkpoint; bypass failure rollback. Each must cause a named test to fail, then restore and rerun.
5. Install only after main review; main owns native E2E and persistent local evidence. UI configure everything in C12; no direct database/config writes as evidence. Use harmless owned test text, no private file upload.
6. Only after all required acceptance: integrate locally, validate integrated tree, clean exact card worktree/branch, write evidence to Issue, close and set Done. Remote publishing remains separately constrained by unpublished unrelated master history.

## Rollback and delivery

Revert this card's integration code without deleting raw history or user files. New schema remains additive; older code may ignore compaction tables. Deliver `docs/vega-issue-76-context-compaction-delivery.md` with exact test outputs/counts, limitations and matrix evidence. Never mark planned/unrun rows PASS.

## Issue #88 amendment — long tool history (2026-09-20)

The #76 implementation reached its 80% trigger with an estimated 259K input against a configured 300K input budget, but failed before contacting the summarizer: the complete historical serialization exceeded the independent `SUMMARY_SOURCE_LIMIT = 128 KiB`. The observed prefix had 122 tool calls and approximately 202 KB of `bash` output. This is a compaction planning failure, not a model-budget failure. The source-byte ceiling may remain a *per-request* safety bound, but may not be a whole-conversation dead end for an otherwise compactable, bounded source.

For #88, process chronological, already-complete historical groups in bounded stages. A tool call and its result remain paired in both provider projections and summarization provenance. A single oversized tool result may be split only into explicitly numbered, bounded, labelled excerpts of the same completed result; each excerpt retains tool identity and position, and every source byte must either reach a summarization stage or produce an explicit typed failure. Do not silently truncate source content, increase one request's limit without a bounded plan, create new tool executions, rewrite raw messages/tool audit, or send oversized requests. Previous committed summary, when present, participates exactly once. Bound stage count and total input by the store's existing source limits, and validate the final projected request against the frozen model budget before committing a checkpoint. Cancellation, provider/usage errors, source mutation, and process restart remain fail-closed per R5–R8. Distinguish `too_large` (internal compaction plan exhausted) from `over_limit` (configured model budget) in UI text.

The successful result must be useful to continue the same agent run and subsequent restart, not merely mark status succeeded. No new dependency, schema migration, credential/config change, or manual-compaction Composer control belongs to #88.

| ID | Setup and operation | Observable pass criterion | Layer |
| --- | --- | --- | --- |
| L01 | Reproduce >128 KiB completed history with many tool calls under a 300K input budget | Baseline test fails with `too_large`; repaired automatic path makes bounded summary requests, installs one checkpoint and resumes the primary request in the same run | production conversation/runtime integration |
| L02 | One completed tool result alone exceeds a stage bound | Every excerpt is labelled and bounded, ordered, linked to the original call/result; no silent byte loss or oversized provider call | unit + provider recorder |
| L03 | Multi-stage summary cancelled, provider fails, or source changes | No partial checkpoint, no follow-on primary request; known usage from completed stages is accounted, failure is typed | representative fault integration |
| L04 | Reopen after successful multi-stage checkpoint | Raw messages/tool calls unchanged; provider projection contains one historical summary plus intact newest user turn; no tool is re-executed | store/conversation E2E |
| L05 | Installed candidate using UI and a real configured provider | Same-session harmless continuation succeeds after automatic compression; status and error copy truthful; screenshot and durable local evidence tied to tested binary | native E2E |

Before implementation, establish L01's red baseline and record its failure. Run the normal full workspace gates, then verify the integrated master tree. Report any row not genuinely executed as NOT RUN rather than PASS.

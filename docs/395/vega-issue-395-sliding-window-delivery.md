# Issue #395 — recent thinking window

## Contract and implementation

The implementation follows [the sliding-window contract](vega-issue-395-thinking-sliding-window.md) and retains the responsiveness behavior delivered in PR #396. This is a UI-only, memory-only follow-up. Provider reasoning limits, task execution, answers, tools and persistence are unchanged.

The previous implementation kept a prefix, rejected subsequent events once its budgets were exhausted, and stopped admitting thinking cards after 256 cards. The new path retains a continuous recent suffix. A card shares 256 KiB between raw thinking and summary; summary takes precedence and can displace a full raw buffer. Per-view FIFO accounting releases older bodies when the 1 MiB or 256 retained-body budget is exceeded. Lightweight titles stay in the original timeline. Eviction drops source/copy text, prepared data, horizontal handles, selection handles/subscriptions and preview construction buffers. Empty historical thinking cannot be expanded.

Small events append without scanning/copying the accumulated body. Front eviction is amortized in 64 KiB batches, preferring nearby paragraph boundaries and preserving UTF-8. Oversized events keep their actual newest suffix instead of concatenating around discarded middle text. A bounded fence tracker supplies rendering context when an opener has left the window; copy uses the exact retained source, without that synthetic opener. Both disclosure presentations use “Earlier thinking omitted”.

The background worker still prepares Markdown and visual fragments. Prepared block/fragment arrays and mapping arrays are shared; front eviction projects a retained range and vertical offset rather than cloning the visual tree. Native prepaint only materializes the viewport and overscan. The existing selection API requires a bounded normalized-text suffix copy when the window advances; this happens at batch eviction, not per small delta. Horizontal source ranges are computed once in background preparation, and UI publication uses their ordered index to migrate retained code/table scroll handles.

A narrow, UI-free `vega_markdown` helper uses the existing parser's source ranges. Literal bytes use absolute source positions; escapes/entities use the raw atom and rendered offset. Layout consumes the parser's literal sequence exactly, without searching for matching repeated text. Synthetic decorations have no source identity. Unmappable selections are cleared rather than reassigned. Retained vertical anchors and selections map through source positions, evicted anchors clamp to the beginning, and tail intent survives an empty pending body.

Worker completion checks source identity, visibility and style generation. If the source window moved, completion publishes only accurately mapped retained overlap and keeps catching up. A snapshot entirely superseded by newer oversized events is rejected and cannot resurrect discarded text. A neutral pending body and current title remain until a newer snapshot is ready.

## Regression matrix

These tests were added before or alongside the implementation. The old failure mechanisms were established from the prefix-admission and unconditional selection-invalidation paths; local execution was intentionally not performed.

| Requirement | Production-path regression | Status |
| --- | --- | --- |
| Folded UTF-8 overflow, advancing title, truthful source/copy, no preparation; an already-truncated visible window emits display change before worker completion even when its fenced title stays unchanged | `issue395_folded_window_keeps_latest_utf8_source_and_title` | First-run PASS; added display assertion pending |
| Oversized event and raw-to-summary budget displacement | `issue395_large_event_keeps_contiguous_tail_and_summary_can_replace_full_raw` | First-run PASS |
| Whitespace summary cannot leave an evicted raw copy | `issue395_whitespace_summary_cannot_leave_evicted_raw_in_copy` | First-run PASS |
| View/body-count pressure preserves answer/tool order while old thinking yields capacity | `issue395_view_window_evicts_old_bodies_without_dropping_timeline` | First-run PASS |
| Repeated Markdown, entities, retained vertical anchor/selection, evicted anchor, tail follow and bounded fragments | `issue395_window_mapping_keeps_repeated_markdown_anchor_and_selection` | First-run PASS |
| Completion during ongoing input publishes overlap; no-overlap and card eviction reject stale work; actual scroll remains at the tail after pending resolves; prepared/selected caches release | `issue395_worker_publishes_retained_overlap_during_streaming_and_rejects_evicted_work` | First-run PASS; added tail assertion pending |
| Oversized fenced line and cut inside a closing delimiter retain correct code/prose semantics and original copy | `issue395_window_preserves_fence_context_and_original_copy` | First-run PASS |
| Retained code preserves horizontal offset after preceding content leaves the window | `issue395_retained_code_keeps_horizontal_scroll_when_window_moves` | First-run PASS |
| Parser source identity distinguishes repeated text and remains stable as literal text grows | `source_positions_distinguish_repeated_markdown_and_survive_append` | First-run PASS |

Existing #395 tests remain unchanged and cover folded notifications, hidden/offscreen preparation, viewport bounds, selection/resize/source replacement, vertical recycling of code, tables and summary disclosure transitions.

Two older #61 tests encoded superseded policy and were updated explicitly: the near-full UTF-8 view now admits the new card and evicts an old body; the cumulative-bound test accepts an event larger than 64 KiB, keeps at most 256 bodies while preserving lightweight timeline entries, and retains the newest card. Existing source order, total byte bounds and UTF-8 assertions remain. Four existing event/card helpers gained test-module visibility for reuse; no production test-only public API was introduced.

The first cloud run exposed one remaining prefix-policy assumption in the cumulative-bound test: the window was required to contain exactly 256 KiB. Its initial three-byte UTF-8 text forces each of the first two 64 KiB evictions forward by two bytes to a character boundary. Later equally sized appends/evictions preserve that four-byte gap. The regression now distinguishes all eight 64 KiB deltas with tail markers and checks the exact continuous input suffix, full newest delta, UTF-8 boundary, the 192–256 KiB retained range for this fixture, and byte accounting after each event. The existing final exact 1 MiB cumulative-budget assertions remain unchanged; this correction is pending cloud validation.

## Local validation freeze

- Date: 2026-10-16; branch: `feat/thinking-sliding-window`.
- Implementation baseline: the master revision released as v0.1.125, containing PR #396. Before submission the main agent fetched and rebased onto the newer master containing PRs #398 and #399; `git range-diff` confirmed the task patch was unchanged by integration.
- Scope: the referenced spec, conversation-stream retention/rendering/selection, the source-position helper, and focused tests.
- `cargo fmt --all`: completed successfully with no output.
- `git diff --check`: completed successfully with no output.
- After rebasing, `cargo fmt --all -- --check` and `git diff --check` both exited `0`; the quick checks took 3.149 seconds. The added-Rust-comment scan found zero new comment lines, and no dependency or workflow files changed.
- Local compilation, Clippy, Rust tests, nextest and packaging: **NOT RUN**, following the user-directed Vega local-validation limit.
- First cloud run `38772438084` on PR #400 at `867da095` failed Clippy because `ThinkingBlock::source_text` became unused in the non-test library. Its remaining callers are tests, so the existing helper is now gated by `#[cfg(test)]`; no lint allowance was added.
- That run's nextest result was 2,729 run: 2,728 passed, 1 failed, 5 skipped. All nine new regressions passed. The only failure was the cumulative-bound test's exact 256 KiB expectation described above. The display/tail assertion additions and both CI fixes await the follow-up run; the complete PR Check has not passed yet.
- Cloud commands: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo nextest run --workspace`.

The main agent records the exact submitted head, CI run and raw failure/pass output during integration. No app installation, restart or manual acceptance was performed by this implementation agent.

## Accepted boundaries and manual verification

- When sustained oversized events replace an entire snapshot before its worker completes, discarded text is never published. The title keeps advancing and the body catches up once input permits a usable snapshot. This is the explicit no-overlap boundary in the spec, not a second retention archive.
- If earlier blocks disappear while a code/table block remains intact, its horizontal offset is retained. If the beginning of one giant unwrapped code line itself is discarded, the numeric horizontal offset is retained and clamped to the new width; this version does not promise that the same source byte stays at the same horizontal screen position.
- Unknown parser/display alignments are not matched by text search. Their selection mapping fails closed, and a fresh preparation reconstructs the retained source.
- Real application responsiveness, mouse scrolling, copy and user acceptance remain manual checks after CI and merge. Issue #395 stays open, and the worktree is retained until that result.

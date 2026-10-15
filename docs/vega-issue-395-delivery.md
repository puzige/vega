# Issue #395 delivery record

## Scope and implementation

Contract: [Long thinking UI responsiveness](vega-issue-395-thinking-ui-performance.md). Delivery: [PR #396](https://github.com/puzige/vega/pull/396). No spec deviation, new dependency, runtime/provider budget change or data migration.

Collapsed thinking keeps its bounded source and incremental title state without preparing Markdown or copying the cumulative source on each delta. Thinking changes now distinguish visible state from geometry changes. Redundant `set_thinking` notifications are suppressed; clock updates use the display path, and changed stage geometry invalidates the owning list entry. An unchanged active assistant skips Markdown snapshot/model synchronization and remeasurement during redraw.

The existing native outer List remains in place. Expanded thinking prepares Markdown and shaped visual fragments on a background worker, then lays out and paints only viewport fragments plus one line of overscan. Visual-line fragments bound long paragraphs; horizontal fragments bound long unwrapped code without adding source newlines. Tables, inline code and nested blocks retain their Markdown structure. The existing maximum disclosure height, short natural height, inner scroll and boundary handling remain.

Each card has at most one preparation task. Further deltas coalesce into the next snapshot. Append-only work may publish a coherent prepared prefix so streaming cannot starve publication; source replacement, visibility, width and theme epochs fence obsolete results. Hidden ancestors and recycled outer-list entries disable preparation. Reading anchors and per-block horizontal scroll handles survive viewport recycling.

Selection uses global UTF-8 byte offsets into shared prepared text, preserving offscreen logical paragraph selection and source copy. Width/theme preparation preserves selection identity; source changes invalidate it. Complete rendered text is retained as `Arc<str>` in the copy interface, so prepaint shares that buffer rather than copying it.

Stage title animation has a retained leaf entity. This changes the notification owner and avoids root observer work such as image-preview synchronization; GPUI can still dirty and render ancestors. This change does not claim to eliminate conversation renders on animation frames.

## Regression coverage

All new cases live in `crates/vega_ui/src/conversation_stream/tests/issue395_thinking_performance.rs` and exercise production event/render/selection paths using the in-process GPUI harness.

| Test | Contract exercised |
| --- | --- |
| `issue395_collapsed_deltas_keep_source_without_preparing_or_invalidating` | Exact retained UTF-8 source; no body preparation, generic group notification or typed display invalidation for unchanged folded deltas |
| `issue395_giant_paragraph_and_code_render_only_viewport_fragments` | Asynchronous opening; large paragraph and single code line have bounded visible fragments, including the tail |
| `issue395_hidden_ancestor_rejects_preparation_and_reopens_latest_source` | Run collapse rejects pending work and reopening prepares the latest retained source |
| `issue395_virtual_thinking_selection_keeps_logical_paragraph_and_rejects_new_source` | Double/triple click, full logical paragraph including offscreen text, clipboard content, resize/scroll preservation and replacement invalidation |
| `issue395_code_horizontal_offset_survives_vertical_recycling` | Horizontal code reading position survives vertical removal and re-entry |
| `issue395_offscreen_thinking_defers_new_source_until_visible` | Native outer-list recycling suppresses hidden preparation; re-entry displays newest source |
| `issue395_centered_and_right_table_fragments_keep_inline_order` | Mixed plain/inline-code table fragments retain row alignment without overlap |
| `issue395_summary_transition_removes_open_body_and_invalidates_stage_height` | First summary removes the open raw body and emits owning-stage geometry invalidation |

Existing thinking shrink/empty tests retain their assertions. Their setup now uses an explicit source-replacement helper instead of mutating the private text buffer and appending an empty delta; production deltas no longer rediscover arbitrary direct field replacement through a cumulative comparison. The existing `issue296_expansion_p07_markdown_selection_and_original_copy` assertions remain unchanged.

## Freeze and local checks

- Recorded at UTC `2026-10-15 10:09:23`, local `2026-10-15 18:09:23 +08:00`.
- Branch: `feat/thinking-ui-performance`; this record covers the follow-up implementation checkpoint before integration.
- Code diff SHA-256, from `git diff origin/master -- crates/vega_ui/src/conversation_stream | shasum -a 256`: `a432eb56e6a869a40bb54bc1015210dc4e3322eaab56ae286f6a8562c71727b3`.
- Local environment: `Darwin arm64`, `rustfmt 1.9.0`, `git version 2.55.0`.
- Local Rust compilation, Clippy, tests and packaging: **NOT RUN**, per the user’s local resource restriction.

Exact formatting check:

```sh
rustfmt --edition 2024 --check crates/vega_ui/src/conversation_stream/core.rs crates/vega_ui/src/conversation_stream/content.rs crates/vega_ui/src/conversation_stream/render.rs crates/vega_ui/src/conversation_stream/model.rs crates/vega_ui/src/conversation_stream/selection.rs crates/vega_ui/src/conversation_stream/thinking.rs crates/vega_ui/src/conversation_stream/thinking_body.rs crates/vega_ui/src/conversation_stream/tests/issue395_thinking_performance.rs
git diff --check
```

Both commands exited `0` with empty stdout/stderr. Combined check duration: `0.217 s`.

## Cloud checks

The first checkpoint ran in [PR Check run 38716142418](https://github.com/puzige/vega/actions/runs/38716142418). Clippy job succeeded, including `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings`. The workspace command was `cargo nextest run --workspace`.

First nextest bounded footer, completed at `2026-10-15 10:02:52 UTC`:

```text
Summary [ 161.814s] 2701 tests run: 2700 passed, 1 failed, 5 skipped
FAIL [ 0.081s] vega_ui conversation_stream::tests::issue296_expansion::issue296_expansion_p07_markdown_selection_and_original_copy
```

The three initial Issue #395 cases passed in that run. The failure exposed an existing copy-interface regression: projected selection had not registered complete rendered text, so `visible_text()` returned `None`. The follow-up checkpoint fixes this by sharing the prepared `Arc<str>` through that interface. It does not alter the failing assertion or restore full-text copies in prepaint. Additional selection, viewport recycling, table and typed-invalidation cases are included in the follow-up checkpoint.

The follow-up checkpoint and additional tests are **PENDING CLOUD CI**. The first run’s success on other tests is not evidence that these later changes pass. The main agent records the final cloud result after integration.

## Limits and manual validation

- Visible preparation takes one source snapshot and parses/shapes that snapshot on the worker. There is no timer throttle or cross-worker parser cache; one in-flight worker bounds concurrency and coalesces deltas. Folded and offscreen bodies do not schedule this work.
- No measured FPS, end-to-end latency, installed-app performance result or real-provider validation is claimed. The original Taffy sample is consistent with layout work but does not prove the complete original runtime cause.
- After green cloud checks and merge, user manual validation should cover the reported long-thinking conversation folded/open, scroll middle/tail, resize, copy selection, return from another task and scroll handoff at disclosure boundaries. No app install/restart was performed by this implementation task.
- Issue status and worktree cleanup remain pending user manual acceptance. Rollback is a revert of the scoped PR.

# Issue #72 — Message anchor navigation delivery

Branch: `feat/72-message-anchor-navigation`
Baseline: `origin/master` at `90d7eba` plus frozen spec commit `a2b78ff`.
Scope: implement `docs/vega-issue-72-message-anchor-navigation.md` without rebuilding #148 history navigation or virtualization.

## Acceptance matrix

| Contract | Evidence | Status |
|---|---|---|
| Hide the rail for short content and show it for overflowing content | `rail_is_hidden_for_short_content_and_shown_for_long_overflow` | Passed |
| Show stable, deduplicated durable-message anchors in conversation order | Projection test plus strict ordered-fraction assertions | Passed |
| Use visible row measurements and content-sensitive estimates, with bounded list measurements and a fixed-size cache | Unequal entry-height/order assertions; scroll-window cache cap; #148 callback bound regression | Passed |
| Keep previews short, normalized, parsed-text-only, and redact credentials; tool previews expose category text only | Projection and sanitizer tests | Passed |
| Mouse click and keyboard navigation emit `MessageLocationRequested` with real message IDs; wheel input over the rail scrolls the list | GPUI interaction test | Passed |
| Preserve the durable scroll anchor and recompute row/rail geometry after a workspace width change | `width_remeasure_preserves_anchor_identity_and_updates_rail_geometry` | Passed |
| Preserve existing message identity, order, geometry, and pixel anchor when a neighboring older page is prepended | `prepending_neighbor_history_page_keeps_existing_anchor_identity_and_order` | Passed |
| Show recoverable location status in populated and empty threads, while preserving #148 routing/pagination | Empty-thread status assertion; `issue148_long_session_scroll` target | Passed |
| Preserve the just-merged #147 Markdown selection/copy rendering path | `issue147_markdown_selection` target on latest master | Passed |
| `cargo fmt --all -- --check` and `git diff --check` | Both commands completed with exit code 0 and no output | Passed |
| Native Computer Use acceptance on an integrated desktop build | Owned by the main agent after integration | Pending |

## Implementation and verification log

### Implementation notes

- Added a narrow left-side rail with one deduplicated anchor per durable message ID. Clicks and keyboard activation use the existing `request_message_location` path; no second history-navigation mechanism was added.
- The current marker follows the list's logical scroll offset. Row positions use measured `bounds_for_item` heights when valid, plus per-entry estimates for offscreen messages. Each capture inspects at most 256 rows; the retained identity-keyed height cache is capped at 512 entries and does not mount extra virtual rows.
- GPUI can briefly report a zero-width row during list remeasurement. Those stale bounds are ignored so they cannot create false overflow or corrupt anchor positions. The list retains explicit full-height/full-width sizing inside the new rail layout.
- Previews use parsed user/assistant text, generic labels for tool/plan/summary rows, a 384-character input scan cap, whitespace normalization, credential-pattern redaction, and a 96-character output cap. The rail is keyboard focusable and exposes an accessible name; wheel events are forwarded to the existing list.
- Added a compact visible status row for the existing Searching, Deferred, NotFound, and Failed navigation states. `Located` adds no persistent UI.

### Verification log

Before implementation, `cargo nextest run -p vega_ui issue72_` ran the four new tests: the preview/projection tests passed while the two visibility/interaction tests failed because the rail did not yet exist. After implementation and fixes:

```text
$ cargo nextest run -p vega_ui issue72_
Starting 8 tests across 1 binary (487 tests skipped)
PASS preview_normalization_redacts_common_credentials_and_is_bounded
PASS anchors_use_unique_durable_message_ids_and_safe_text_projections
PASS rail_is_hidden_for_short_content_and_shown_for_long_overflow
PASS mouse_and_keyboard_anchor_navigation_emit_real_message_ids (wheel forwarding included)
PASS width_remeasure_preserves_anchor_identity_and_updates_rail_geometry
PASS prepending_neighbor_history_page_keeps_existing_anchor_identity_and_order
PASS measured_entry_height_cache_stays_bounded_while_scrolling
PASS empty_thread_still_displays_recoverable_location_status
Summary: 8 tests run: 8 passed, 486 skipped
```

The additional compatibility check initially caught that the wrapped virtual list needed its own explicit `.h_full().w_full()` sizing; after restoring those constraints, the complete existing #148 target passed:

```text
$ cargo nextest run -p vega_ui issue148_long_session_scroll
Starting 9 tests across 1 binary (486 tests skipped)
Summary: 9 tests run: 9 passed, 486 skipped
```

The adjacent 10k mixed-entry regression originally compared a pixel offset with exact float equality. On this branch, the rail's wrapped layout makes the same preserved offset round from `76.5px` to `76.50001px` (about `0.0000076px` drift); three runs reproduced it. The same target passed on the no-#72 `origin/master` baseline (`90d7eba`), confirming the new layout path exposed the overly strict assertion. Since the contract and assertion message already allow `<1px` drift, the assertion now enforces `<0.001px`, retaining a much tighter bound than the contract while tolerating the observed float representation.

```text
$ cargo nextest run -p vega_ui ten_k_mixed_items_trunk_e2e --retries 2
TRY 1 FAIL: left 76.50001px, right 76.5px
TRY 2 FAIL: left 76.50001px, right 76.5px
TRY 3 FAIL: left 76.50001px, right 76.5px

$ cargo nextest run -p vega_ui ten_k_mixed_items_trunk_e2e  # origin/master 90d7eba, no #72 rail
Summary: 1 test run: 1 passed, 486 skipped
```

After replacing the exact-equality assertion with the tighter-than-contract tolerance, the targeted e2e passed on the feature branch:

```text
$ cargo nextest run -p vega_ui ten_k_mixed_items_trunk_e2e
Starting 1 test across 1 binary (494 tests skipped)
PASS ten_k_mixed_items_trunk_e2e
Summary: 1 test run: 1 passed, 494 skipped
```

The latest-master rebase also encountered #147's new selection-aware row renderer; the #72 layout keeps that renderer intact. Its focused target passed:

```text
$ cargo nextest run -p vega_ui issue147_markdown_selection
Starting 3 tests across 1 binary (489 tests skipped)
Summary: 3 tests run: 3 passed, 489 skipped
```

During the short-session render test, a remeasure frame exposed zero-width stale row geometry. Ignoring those invalid measurements fixed the false rail display; the final #72 and #148 runs above both passed.

```text
$ cargo fmt --all -- --check
(exit 0; no output)

$ git diff --check
(exit 0; no output)
```

Final follow-up verification after rebasing onto `90d7eba`:

```text
$ cargo nextest run -p vega_ui issue72_
Starting 8 tests across 1 binary (487 tests skipped)
Summary: 8 tests run: 8 passed, 487 skipped

$ cargo nextest run -p vega_ui issue148_long_session_scroll
Starting 9 tests across 1 binary (486 tests skipped)
Summary: 9 tests run: 9 passed, 486 skipped

$ cargo nextest run -p vega_ui ten_k_mixed_items_trunk_e2e
Starting 1 test across 1 binary (494 tests skipped)
Summary: 1 test run: 1 passed, 494 skipped

$ cargo fmt --all -- --check
(exit 0; no output)

$ git diff --check
(exit 0; no output)
```

No full-workspace tests were run. Native Computer Use remains for the main agent after integration.

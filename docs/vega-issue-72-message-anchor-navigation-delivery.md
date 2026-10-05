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
#### Hover-preview dismissal follow-up (2026-10-04, Asia/Shanghai)

- On installed S21 (v0.1.22), native hover showed the preview and keyboard navigation updated it. A clean native pointer-exit check remains inconclusive because the Computer Use interface has no move-only pointer action; clicking or dragging is not a substitute.
- The local fix changes rail exit handling to clear the rail-hover preview when the pointer leaves both the rail and preview. The preview's own hover state still keeps the preview open when the pointer enters it.
- The new regression was red on the previous implementation (Nextest run `535c1aad-e39b-495f-94d3-2830dc7a6f88`) and passes on this fix (run `9f7b211c-9879-4e77-a815-1cd9abdd8561`, 1 passed / 517 skipped). The complete #72 target passed 12/12 (run `8537f117-62b4-419a-9aee-b10489f007f4`, 506 skipped); this includes entering the preview from the rail and retaining it.
- The first run after extending the assertions stopped at compilation: the test compared `Some(target)` and later reused the `String`. Cloning the expected ID fixed the ownership error; no runtime test ran on that attempt.
- The first 960px preview-lane version failed because it expected anchor 38 after hiding and reopening the lane, although remeasurement made the same fixed pointer coordinate select anchor 39 (Nextest run `0c0fc6c7-79b9-4022-9327-730a4da81e2f`, 11 passed / 1 failed). The regression now captures the actual selected ID after reentry and verifies the preview retains that selection when entered. This was a brittle test-coordinate assumption, not a product failure.
- The expanded regression covers rail-to-outside dismissal, rail-to-preview retention, and preview-to-outside dismissal in the 960px lane layout; the existing interaction test covers the 1200px overlay layout. The keyboard test asserts that the visible preview moves when the selected anchor changes. The lane-only check passed (Nextest run `473f39f1-6260-48d0-a2ce-2d8e03d4a802`, 1 passed / 518 skipped); final target: `cargo nextest run -p vega_ui issue72_`, run `cd50142e-f452-4e22-8f97-9f5e80a7de21`, 12 passed / 507 skipped. `cargo fmt --all -- --check` and `git diff --check` exited 0.
- Raw targeted result:

```text
$ cargo nextest run -p vega_ui issue72_
Finished `test` profile [unoptimized + debuginfo] target(s) in 0.41s
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
────────
Nextest run ID cd50142e-f452-4e22-8f97-9f5e80a7de21 with nextest profile: default
Starting 12 tests across 1 binary (507 tests skipped)
PASS [   0.015s] ( 1/12) vega_ui conversation_stream::tests::issue72_message_anchor_navigation::preview_normalization_redacts_common_credentials_and_is_bounded
PASS [   0.028s] ( 2/12) vega_ui conversation_stream::tests::issue72_message_anchor_navigation::anchors_use_unique_durable_message_ids_and_safe_text_projections
PASS [   0.029s] ( 3/12) vega_ui conversation_stream::tests::issue72_message_anchor_navigation::empty_thread_still_displays_recoverable_location_status
PASS [   0.031s] ( 4/12) vega_ui conversation_stream::tests::issue72_message_anchor_navigation::run_activity_entries_contribute_geometry_without_duplicating_message_anchors
PASS [   0.060s] ( 5/12) vega_ui conversation_stream::tests::issue72_message_anchor_navigation::rail_is_hidden_for_short_content_and_shown_for_long_overflow
PASS [   0.093s] ( 6/12) vega_ui conversation_stream::tests::issue72_message_anchor_navigation::keyboard_anchor_preview_uses_a_reserved_lane_in_a_narrow_pane
PASS [   0.104s] ( 7/12) vega_ui conversation_stream::tests::issue72_message_anchor_navigation::hover_anchor_preview_dismisses_when_pointer_leaves_rail_and_preview
PASS [   0.109s] ( 8/12) vega_ui conversation_stream::tests::issue72_message_anchor_navigation::keyboard_anchor_selection_displays_a_bounded_sanitized_preview
PASS [   0.114s] ( 9/12) vega_ui conversation_stream::tests::issue72_message_anchor_navigation::prepending_neighbor_history_page_keeps_existing_anchor_identity_and_order
PASS [   0.131s] (10/12) vega_ui conversation_stream::tests::issue72_message_anchor_navigation::mouse_and_keyboard_anchor_navigation_emit_real_message_ids
PASS [   0.151s] (11/12) vega_ui conversation_stream::tests::issue72_message_anchor_navigation::width_remeasure_preserves_anchor_identity_and_updates_rail_geometry
PASS [   0.257s] (12/12) vega_ui conversation_stream::tests::issue72_message_anchor_navigation::measured_entry_height_cache_stays_bounded_while_scrolling
────────
Summary [   0.257s] 12 tests run: 12 passed, 507 skipped

$ cargo fmt --all -- --check
(exit 0; no output)

$ git diff --check
(exit 0; no output)
```
- `cargo fmt --all -- --check` and `git diff --check` both exited 0. No full-workspace tests were run. Installed-app acceptance of the fix remains pending a new candidate install and native pointer movement check.


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

## Mounted production-root navigation fix (2026-10-05)

- Branch: `codex/72-anchor-click-nav-20261005`; starting baseline was the requested current `origin/master`.
- Root cause: the mounted `VegaWindow` receives `MessageLocationRequested` and reports the loaded message as located, but `ListState::scroll_to_reveal_item` does not stop active tail-following. The next layout therefore returns to the tail and leaves the requested message offscreen.
- Fix: `ConversationStream::reveal_loaded_message` pauses tail-following before revealing the message. The regression mounts the production `VegaWindow`, confirms its loaded stream is following the tail, emits the request through the stream event, and checks the root request generation, `Located` status, real target message ID at the visible scroll anchor, and stopped tail-following after layout.
- Contract deviations: none. Deferred and unloaded-message route behavior remains covered by the root `message_location` subset. No provider request, settings change, app install, new dependency, or code comment was added.
- Verification time: 2026-10-05 03:38 CST / 2026-10-04 19:38 UTC.

### Red-first regression and setup diagnostics

The first compile attempt of the new test failed because its readiness closure passed `TestAppContext` where GPUI expected `App`; the closure was corrected before the runtime regression. A subsequent fixture assertion expected exactly 200 hydrated entries but observed 201, so the setup now checks that at least the 200-row requested page is loaded. The root starts in tail-follow mode, so the regression uses that mounted default instead of assuming a return-to-bottom button is visible.

Before the implementation fix, the production-root regression failed after the event route had run and returned `Located`:

```text
$ cargo nextest run -p vega loaded_message_location_routes_through_mounted_root_and_reveals_target
Nextest run ID a7a1bd36-14d4-4828-80c3-f8844474e8a3
Starting 1 test across 2 binaries (212 tests skipped)
FAIL vega::bin/vega tests::history::loaded_message_location_routes_through_mounted_root_and_reveals_target
assertion `left == right` failed
  left: Some("user-142")
 right: Some("user-100")
Summary: 1 test run: 0 passed, 1 failed, 212 skipped
```

An initial `cargo fmt --all -- --check` also found one formatting adjustment in the added test; it was corrected manually and the check rerun.

### Final focused verification

```text
$ cargo nextest run -p vega message_location
Nextest run ID 723159f3-2792-4791-a124-c0647638c20e
Starting 5 tests across 2 binaries (208 tests skipped)
PASS unknown_message_location_result_projects_not_found
PASS message_location_generation_fences_loaded_reselection_and_a_b_a
PASS message_location_worker_returns_the_bounded_containing_page
PASS active_message_location_defers_without_mutating_live_entries_then_resumes
PASS loaded_message_location_routes_through_mounted_root_and_reveals_target
Summary: 5 tests run: 5 passed, 208 skipped

$ cargo nextest run -p vega_ui issue72_
Nextest run ID 164898f3-da42-4840-9e83-4ef157aa6004
Starting 12 tests across 1 binary (517 tests skipped)
PASS preview_normalization_redacts_common_credentials_and_is_bounded
PASS anchors_use_unique_durable_message_ids_and_safe_text_projections
PASS empty_thread_still_displays_recoverable_location_status
PASS run_activity_entries_contribute_geometry_without_duplicating_message_anchors
PASS rail_is_hidden_for_short_content_and_shown_for_long_overflow
PASS keyboard_anchor_preview_uses_a_reserved_lane_in_a_narrow_pane
PASS keyboard_anchor_selection_displays_a_bounded_sanitized_preview
PASS hover_anchor_preview_dismisses_when_pointer_leaves_rail_and_preview
PASS prepending_neighbor_history_page_keeps_existing_anchor_identity_and_order
PASS mouse_and_keyboard_anchor_navigation_emit_real_message_ids
PASS width_remeasure_preserves_anchor_identity_and_updates_rail_geometry
PASS measured_entry_height_cache_stays_bounded_while_scrolling
Summary: 12 tests run: 12 passed, 517 skipped

$ cargo nextest run -p vega_ui issue148_long_session_scroll
Nextest run ID 7c8f04a8-eed2-4ed6-9f36-fa63e2739267
Starting 9 tests across 1 binary (520 tests skipped)
PASS production_render_samples_remain_bounded
PASS durable_entry_identity_survives_prepend_and_rebuild
PASS target_window_replacement_drops_old_cards_and_preserves_thread_state
PASS newer_page_appends_and_preserves_the_existing_anchor
PASS scroll_anchor_snapshot_restores_message_identity_and_offset_after_rebuild
PASS newer_page_request_waits_until_the_list_reaches_the_bottom
PASS loaded_message_reveal_keeps_the_entry_model_and_scrolls_to_target
PASS prepend_and_column_remeasure_restore_stable_pixel_anchor
PASS mixed_fixtures_keep_entry_counts_and_render_callbacks_bounded
Summary: 9 tests run: 9 passed, 520 skipped

$ cargo fmt --all -- --check
(exit 0; no output)

$ git diff --check
(exit 0; no output)
```

No workspace-wide tests were run. Native Computer Use on an integrated build remains with the main agent.

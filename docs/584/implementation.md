# Code viewport implementation

Contract: [code-horizontal-scroll.md](code-horizontal-scroll.md), v1.3. Baseline: `73977c89eba718da4b2573227c44beb8bcff6911`. Implementation handoff: 2026-10-24; changes are uncommitted for the root agent to integrate. Spec deviations: none.

## Implemented scope

- `conversation_stream/code_viewport.rs` composes one clipped, tracked content area and a final sibling `Scrollbar::horizontal`. `overflow_hidden` follows the pinned toolkit's own horizontal viewport helper: the native area supplies measurement/clamping but does not remap or consume wheel input. One private handler on the stable viewport owns horizontal wheel routing; keyed per-window/per-block gesture state combines pinned `OngoingScroll::filter` with horizontal ownership for filtered-zero tails. Child-before-parent Bubble ordering lets it consume horizontal input before the ancestor List or Thinking vertical body. Its X offset clamps immediately, and horizontal input remains consumed at either edge or without overflow. Ordinary vertical input passes through.
- Ordinary Markdown obtains its ScrollHandle from keyed per-window element state. The existing message/block/line identity remains the key. The header and copy action remain outside the scrolling body; text is unchanged and unwrapped.
- Thinking retains its existing `horizontal_scrolls` map and fixed prepared block dimensions. Its code/table horizontal body uses the same viewport without adding layout height. Wheel handling stays on the unshifted hidden-overflow viewport.
- Scrollbars use `Hover` when the system requests auto-hide and `Always` otherwise. The pinned scrollbar supplies real handle drag and track-click behavior. Scrollbar mouse handling does not start or expand text selection; existing selection clearing behavior is unchanged.
- `ScrollFade::horizontal_viewport` explicitly enables horizontal fades on the tracked hidden-overflow viewport. Its new flag defaults to false, preserving other fade call sites. Fades wrap content only, and the scrollbar paints afterwards.

## Development regression coverage

Eleven development GPUI tests exercise production `selectable_markdown_content` or Thinking `PreparedBody`/`BodyElement` under a real parent List. A development-only observation hook records the actual rendered ScrollHandles; it does not replace routing or scrolling decisions. Tests use real input dispatch, not copied algorithms or source-string checks.

| Test | Contract coverage | Execution |
| --- | --- | --- |
| `markdown_code_routes_wheel_and_keeps_viewport_capture` | Pixel/line Y, diagonal and gesture wobble, zero-X horizontal tail, X with Shift, translated-content viewport edge, both clamp edges and their zero-X tails, parent Y | Pending Actions |
| `thinking_code_routes_wheel_and_keeps_viewport_capture` | Same routing through actual Thinking body prepaint | Pending Actions |
| `markdown_horizontal_gesture_releases_for_vertical_input` | Ended/Cancelled tails stay contained, following vertical Moved/Started passes, line Y clears precise gesture ownership and following pixel Y Moved passes without Started, unphased Moved after the recognizer timeout passes | Pending Actions |
| `thinking_horizontal_gesture_releases_for_vertical_input` | Same gesture lifecycle through actual Thinking body | Pending Actions |
| `markdown_scrollbar_drags_without_selecting_and_text_still_copies` | Thumb drag, track click, parent Y, no new selection from chrome, subsequent text selection/clipboard | Pending Actions |
| `thinking_scrollbar_drags_without_selecting_and_text_still_copies` | Same through Thinking body | Pending Actions |
| `markdown_code_offsets_remain_independent_after_append` | Two actual code blocks, independent X offsets and preserved viewport bounds after incremental append/rerender | Pending Actions |
| `thinking_code_offsets_remain_independent_after_append` | Same with prepared Thinking body and retained map | Pending Actions |
| `fitting_markdown_code_contains_horizontal_input_and_passes_vertical` | No overflow: horizontal containment including zero-X tail, finite stationary handle, ordinary Y passes | Pending Actions |
| `fitting_thinking_code_contains_horizontal_input_and_passes_vertical` | Same through Thinking body | Pending Actions |
| `same_markdown_identity_in_separate_windows_has_independent_offsets` | Same message identity in distinct windows keeps separate handle state | Pending Actions |

The drag fixture waits 12 ms between down and move because the pinned scrollbar uses wall-clock `Instant` to enforce its existing 120 fps update limit. Each gesture lifecycle regression waits once for 40 ms to establish expiry of pinned `OngoingScroll`'s 28 ms fallback before dispatching a new vertical Moved without Started. Its `scheduler::Instant` is a `web_time::Instant` reexport using wall clock on native targets; advancing the executor's separate TestClock would not establish this precondition. Contained-tail sequences dispatch consecutive events inside one window update and assert the actual code offset and parent List Y after each event, before rendering a new frame. These are cloud-test timing conditions only; production timing is unchanged. TestPlatform reports always-visible scrollbars, so the system Hover preference and physical-device Shift behavior still require manual acceptance. The Thinking fixture uses actual preparation/rendering and stable map state; it is not evidence for the full asynchronous Thinking controller lifecycle.

Development-only files/support to remove after cloud evidence is fixed: `code_viewport_tests.rs`, `thinking_code_viewport_tests.rs`, their `cfg(test)` wiring/observation hook, the vega_ui dev `test-support` feature, and its exclusive lock closure. No old test collection was restored.

## Actual local verification

Only static/format/metadata checks were run. Vega was not launched, restarted, installed, compiled, or tested locally.

| Check | Result |
| --- | --- |
| `rustfmt --edition 2024 --config skip_children=true` on the seven changed Rust files | Exit 0 |
| `rustfmt --edition 2024 --check --config skip_children=true` on the seven changed Rust files | Exit 0 |
| `git diff --check` | Exit 0 |
| `cargo metadata --offline --format-version 1` | Exit 0; metadata/feature resolution only, no compilation or downloads |
| `cargo metadata --offline --locked --format-version 1 --no-deps` | Exit 0 |
| Standard-library TOML comparison of baseline/current lock package identities and dependency edges | Existing 914 package versions, sources and checksums unchanged; no existing dependency edge removed |
| New code scan for comments and non-test `unwrap()`/`expect()` | No matches in the new component/test files; edited production hunks add none |

Offline resolution added ten packages required by the already-pinned GPUI test-support feature: `bit-set 0.8.0`, `bit-vec 0.8.0`, `convert_case 0.11.0`, `proptest 1.11.0`, `proptest-macro 0.5.0`, `quick-error 1.2.3`, `rand_xorshift 0.4.0`, `rusty-fork 0.3.1`, `unarray 0.1.4`, `wait-timeout 0.2.1`. The only added edges on pre-existing packages are `gpui-pre` to existing `backtrace 0.3.76` and new `proptest 1.11.0`; other lock-line changes disambiguate package versions.

## Handoff and residuals

Actions run `39999442658` at head `73b5ce5b` failed to compile before Rust test discovery/execution: Thinking's outer vertical body still used `tracked_scroll_edge_fade` and `ScrollFade`, whose import was incorrectly removed while replacing its horizontal renderer. The required `ScrollEdgeFadeExt`/`ScrollFade` import was restored; no behavior or assertion changed. Static search confirmed the remaining vertical call site, and rustfmt/check plus `git diff --check` exited 0. Cloud revalidation is required.

Independent review follow-up on `89e75717`: the development fixture now places `gpui_kit::base::TextSelectionLayer` before its List body, matching the real Vega window root. `gpui_kit::init` alone does not install that window event layer. The 320 × 400 viewport, production code, all wheel/drag assertions, and the positive text-selection/clipboard control remain unchanged. Only test wiring was corrected. Follow-up rustfmt/check and `git diff --check` completed with exit 0; automated execution remains pending Actions.

READY: implementation writer stopped. Root owns Git/index, commit, rebase, PR, independent review, cloud checks, development-test evidence/cleanup, and final CI. No CI result has been obtained by this implementation agent. CI must validate Rust types and behavior; local static success is not behavioral validation. The current running Vega has not changed. No user database, credentials, other worktree, #548 branch, or scheduler state was modified.

## v1.3 continuation

Continuation starts from `3039b9cc6a1f3b816f6cd3a25c3bb7035e5c9a21`; the root fetched/updated the branch and wrote v1.3 before production changes. The stopped prior writer's existing implementation and nine development regressions were retained. Scope is limited to `code_viewport.rs`, `code_viewport_tests.rs`, `thinking_code_viewport_tests.rs` and this record.

The pinned mask filtered Started(-40,-10) followed by Moved(0,-1) to zero movement, then failed to consume the original Y because its guard only stopped propagation when X changed. The replacement uses one viewport `on_scroll_wheel` with GPUI's existing hitbox/occlusion check. It stores the recognizer plus horizontal ownership in keyed state, preserves ownership on a filtered-zero delta, consumes the prior horizontal gesture's Ended/Cancelled tail and clears ownership, and lets new nonzero filtered Y update ownership after Started or the existing recognizer timeout. Line deltas reset the preceding precise recognizer/ownership and use per-event dominance. Only an actual X offset change requests refresh. The mask was removed, so no second wheel-input owner remains. Scrollbar order, header, Thinking geometry and fades are unchanged.

The root supplied [Actions run 40000485051](https://github.com/puzige/vega/actions/runs/40000485051) at the starting head: Clippy succeeded; Rust discovered/executed nine tests, with seven passed and the Markdown/Thinking drag tests failed at the existing X < -100 assertion. Independent read-only analysis identified a fixture hit point on the thumb's excluded bottom boundary: pinned `Anchor::BottomLeft` geometry ends the thumb at viewport bottom minus 4, and bounds containment excludes that edge. The fixture now starts at bottom minus 8, inside the actual thumb. The drag-distance, parent Y, no-selection, track-click and positive text-selection/clipboard assertions remain unchanged. No scrollbar production behavior, retry or extra settling step was added.

Actual continuation checks: `rustfmt --edition 2024 --config skip_children=true` and its `--check` form on the three scoped Rust files exited 0; `git diff --check` exited 0. A static source inventory found six Markdown/fixture-side and five Thinking GPUI test declarations, exactly one production wheel-handler registration, no mask reference, no added code comment markers and no production `unwrap()`/`expect()`. This is static inventory, not test discovery or execution. No dependency, manifest or lockfile change was needed. No local Vega execution, compilation, Clippy or test run occurred. All eleven current regressions and the corrected drag fixture remain pending Actions; historical results do not validate this uncommitted continuation.

READY follow-up: the line-input branch now explicitly resets both precise recognizer and horizontal ownership before its per-event direction decision. The existing lifecycle regression dispatches precise horizontal Started, line Y, and pixel Y Moved without Started in one window update, asserting unchanged code X and a real parent List Y change for each vertical event before rendering. This prevents a stale precise lock from swallowing the post-line pixel event without relying on expiry. The same scoped rustfmt/check and `git diff --check` were repeated with exit 0; execution remains pending Actions.

Selection-fixture follow-up starts from `791537897d7b949618ea1e26b8a46b414531c1b0`. [Actions run 40003782199](https://github.com/puzige/vega/actions/runs/40003782199) executed eleven Rust tests: ten passed, one failed, zero skipped; Clippy succeeded. The raw failure is the Markdown test's post-scroll text-selection positive control at the existing `has_selected_text()` assertion. The preceding thumb/track movement, outer Y and no-selection assertions passed; Thinking's full selection/clipboard test passed. The root and independent read-only review identified test geometry as the responsibility layer: Markdown's 20 px line plus 12 px bottom padding produces a 32 px viewport with the 16 px scrollbar track at local Y [16,32), so the old text point Y=18 still hit scrollbar chrome. Thinking's additional 12 px top padding produces a 44 px viewport whose track begins at Y=28.

The shared development helper now accepts `text_y_inset`. Markdown supplies half of `Typography::CODE * Typography::CODE_LINE_HEIGHT` (10 px); Thinking also includes `Layout::MARKDOWN_CODE_PADDING` (22 px). The X coordinate stays at left plus 18, and precondition assertions require the point to be inside the viewport and above the verified track. All existing drag-distance, parent Y, no-selection, positive selection and clipboard assertions remain intact. Only the two development test files and this record changed; no production behavior or selection hook changed. Scoped rustfmt and its check form exited 0, as did `git diff --check`. No local Vega code, compilation or tests ran. The corrected fixture remains pending Actions.

READY: the continuation writer has stopped all writes in the assigned scope. Root owns Git integration, independent review, cloud execution, development-test evidence/cleanup and final CI. Spec deviations: none. The installed/running application and user data have not been touched.

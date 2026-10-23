# Code viewport implementation

Contract: [code-horizontal-scroll.md](code-horizontal-scroll.md), v1.2. Baseline: `73977c89eba718da4b2573227c44beb8bcff6911`. Implementation handoff: 2026-10-24; changes are uncommitted for the root agent to integrate. Spec deviations: none.

## Implemented scope

- `conversation_stream/code_viewport.rs` composes one clipped, tracked content area, a sibling horizontal `ScrollableMask`, and a final sibling `Scrollbar::horizontal`. `overflow_hidden` follows the pinned toolkit's own horizontal viewport helper: the native area supplies measurement/clamping but does not remap or consume wheel input. The mask owns horizontal gesture routing, including capture before the ancestor List and horizontal boundary containment.
- Ordinary Markdown obtains its ScrollHandle from keyed per-window element state. The existing message/block/line identity remains the key. The header and copy action remain outside the scrolling body; text is unchanged and unwrapped.
- Thinking retains its existing `horizontal_scrolls` map and fixed prepared block dimensions. Its code/table horizontal body uses the same viewport without adding layout height. Mask placement preserves the toolkit helper's static-position layout; it is not translated with the content.
- Scrollbars use `Hover` when the system requests auto-hide and `Always` otherwise. The pinned scrollbar supplies real handle drag and track-click behavior. Scrollbar mouse handling does not start or expand text selection; existing selection clearing behavior is unchanged.
- `ScrollFade::horizontal_viewport` explicitly enables horizontal fades on the tracked hidden-overflow viewport. Its new flag defaults to false, preserving other fade call sites. Fades wrap content only, and the scrollbar paints afterwards.

## Development regression coverage

Nine new GPUI tests exercise production `selectable_markdown_content` or Thinking `PreparedBody`/`BodyElement` under a real parent List. A development-only observation hook records the actual rendered ScrollHandles; it does not replace routing or scrolling decisions. Tests use real input dispatch, not copied algorithms or source-string checks.

| Test | Contract coverage | Execution |
| --- | --- | --- |
| `markdown_code_routes_wheel_and_keeps_viewport_capture` | Pixel/line Y, diagonal and gesture wobble, X with Shift, translated-content viewport edge, both clamp edges, parent Y | Pending Actions |
| `thinking_code_routes_wheel_and_keeps_viewport_capture` | Same routing through actual Thinking body prepaint | Pending Actions |
| `markdown_scrollbar_drags_without_selecting_and_text_still_copies` | Thumb drag, track click, parent Y, no new selection from chrome, subsequent text selection/clipboard | Pending Actions |
| `thinking_scrollbar_drags_without_selecting_and_text_still_copies` | Same through Thinking body | Pending Actions |
| `markdown_code_offsets_remain_independent_after_append` | Two actual code blocks, independent X offsets and preserved viewport bounds after incremental append/rerender | Pending Actions |
| `thinking_code_offsets_remain_independent_after_append` | Same with prepared Thinking body and retained map | Pending Actions |
| `fitting_markdown_code_contains_horizontal_input_and_passes_vertical` | No overflow: horizontal containment, finite stationary handle, ordinary Y passes | Pending Actions |
| `fitting_thinking_code_contains_horizontal_input_and_passes_vertical` | Same through Thinking body | Pending Actions |
| `same_markdown_identity_in_separate_windows_has_independent_offsets` | Same message identity in distinct windows keeps separate handle state | Pending Actions |

The drag fixture waits 12 ms between down and move because the pinned scrollbar uses wall-clock `Instant` to enforce its existing 120 fps update limit. This is cloud-test timing support only; production timing is unchanged. TestPlatform reports always-visible scrollbars, so the system Hover preference and physical-device Shift behavior still require manual acceptance. The Thinking fixture uses actual preparation/rendering and stable map state; it is not evidence for the full asynchronous Thinking controller lifecycle.

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

READY: implementation writer stopped. Root owns Git/index, commit, rebase, PR, independent review, cloud checks, development-test evidence/cleanup, and final CI. No CI result has been obtained by this implementation agent. CI must validate Rust types and behavior; local static success is not behavioral validation. The current running Vega has not changed. No user database, credentials, other worktree, #548 branch, or scheduler state was modified.

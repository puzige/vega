# Conversation status NSH leaves

## Scope and source

Specification written before implementation, against HEAD `0b5ea426abfd8d4358489e916273d89474fb190f` in `vega-907-nsh-full`. Only `conversation_stream/conversation_status.rs`, its child directory and this document belong to this writer. Parent owns module declaration and both render call sites; no shared NSH/shimmer changes, business decisions, dependencies or i18n changes belong here. The user-provided updated baseline supersedes fetch/update requirements: no network or Git mutations are authorized.

## API and integration

- `pub(super) fn empty(window: &mut Window, cx: &mut App) -> AnyElement`
- `pub(super) fn location(label: String, color: Rgba, window: &mut Window, cx: &mut App) -> AnyElement`

Parent declares `mod conversation_status;`, calls `conversation_status::empty(window, cx)` instead of the old empty-state text block, and lays its optional location element out separately. The empty leaf includes the former 24px bottom margin. Parent retains Searching/Deferred/NotFound/Failed label and color selection, Located suppression, and supplies mutable window at both location call sites. The location leaf includes former 8px horizontal and 4px vertical padding. No control, callback or new wording is added.

## Omitted Thinking continuation acceptance

The parent extends this existing passive, measured NSH text host to the actual `RunActivityGroup::render_segment_with_top` omitted-thinking notice. This is within the full production migration; no dependency, scrolling protocol, controller, i18n wording, or source-selection change is required. A dedicated `OmittedThinking(group_entity, segment_index)` key prevents simultaneous timeline rows from sharing host state. Preserve the existing `truncated` condition, exact `ui.earlier_thinking_omitted` label, tertiary theme color, METADATA typography and PROCESS_ITEM_GAP top spacing. The notice has no location-style horizontal/vertical padding and must wrap to natural height. Reuse identity/width-fenced measurements, zero-input passive prepaint, complete AX label, old-Surface retirement and non-tab-stop hosting. Development regression mounts this production renderer at narrow width and checks the exact glyph/AX content, style, no interaction events, actual wrapped height, and gap/no-location-padding layout. No native acceptance is claimed by that model test.

## Acceptance

- All glyphs and colors are produced by NSH. GPUI supplies only layout, native font-style inputs and the surface host; no native text paint/fallback.
- Empty title and subtitle use unchanged `ui.what_would_you_like_to_do_today` and `ui.enter_a_task_to_start_or_use_to`, primary/secondary theme colors, `Typography::EMPTY_STATE_TITLE`, `EMPTY_STATE_TITLE_WEIGHT` and `METADATA`. A centered column retains the 8px gap, full width, ordinary 72px height and 24px bottom margin. The 72px height is a minimum when wrapping needs more space: real text must neither clip nor overlap the separately laid-out location status.
- Location uses the caller's exact label and color, METADATA size, full width, left alignment and 8/4px padding. Both leaves allow unlimited wrap without ellipsis or rewritten text. Inherited family, fallback, weight/style and supported OpenType features are passed to NSH; native inherited line-height is resolved at each token font size.
- One leaf-local host implementation owns both variants with distinct keyed state. A shrink-disabled, auto-height NSH content subtree reports actual layout height through `on_layout`; GPUI uses that height, not a permanently fixed estimate. Initial estimates only bootstrap the first layout. Prepaint still runs to obtain NSH layout, but paint waits for an accepted current-projection measurement, avoiding a visibly clipped bootstrap frame.
- Measurement reports carry a projection identity and viewport width. Accept finite positive height only for the current identity and width. Do not reject measurements because the window is inactive, hidden or not focused. Width and source/style changes invalidate old measurements. No stale source surface may remain visible while a new projection initializes.
- Host opts into `Surface::new_transparent` and `.tab_stop(false)`. Prepaint its child under a zero-area mask centered within the inherited mask; paint under the normal element/intersection mask. No pointer/wheel/pinch handler, focus request, animation or engine timer is added. Complete noninteractive accessible labels remain in the NSH subtree.
- Stop superseded/released surfaces. Async height tasks are measurement plumbing only, not business callbacks. Parent Cargo integration, independent review and native acceptance are separate gates.

## Validation plan and restrictions

This writer may run explicit-path rustfmt and read-only static checks only; no compilation, test execution, App launch, network, Git changes or delegation. Development tests, if added, cover the actual production renderer's text/style/accessibility/update/wrap-height behavior and measurement identity/width validation. They are not native hitbox or pixel evidence and must be removed before master under repository policy after parent validation and evidence capture.

Required parent checks: compile and strict lint of the final integrated source; renderer/measurement tests; wide/narrow and translated empty/status text, source/color/theme changes while visible or inactive, stale async measurement rejection, pointer/wheel/pinch propagation, Tab/Shift-Tab, noninteractive full-label AX and parent-background continuity. No model result alone establishes native acceptance.

## Implementation and stopped evidence

Implemented only the two scoped Rust files and this document. Shared `nsh.rs`, `newshimmer`, parent `mod.rs` / `render.rs` and other writers' files were not edited. The host stops and drops the previous Surface on source/font/color/width changes rather than asynchronously publishing replacement text into the old visible Surface. It starts the new surface only after GPUI supplies a positive width. Its child is prepainted for layout/AX before measurement readiness, but normal-mask paint is withheld until current NSH height has arrived; this can produce a brief blank bootstrap rather than old-source or clipped pixels. Actual native replacement latency is unverified.

The empty leaf uses `max(72, measured NSH content height)`, with 24px bottom margin; parent must not reintroduce a fixed 72px clipping wrapper or duplicate the margin/padding. Normal content preserves 72px; narrow/localized wrapped content may expand. Location directly uses the measured padded content height, including decreases. Heights are not conditioned on active/focus/visibility state.

Pinned public APIs inspected locally: `nsh-core 0.4.0` (unlimited `AttributedText::max_lines = 0`, `on_layout` and Core frame/event dispatch), `nsh-platform-gpui 0.4.0` Surface prepaint/paint, and `gpui-fast 0.1.9` text-style/line-height and keyed-state APIs. This leaf reuses the passive transparent-host mechanism reviewed in [stage/shimmer](nsh-stage-shimmer.md); it does not claim new native input acceptance. The same supported-font boundary applies: enabled `smcp/onum/lnum/tnum/pnum` and boolean italic are mapped; arbitrary disabled/other font features and oblique distinction are not expressible by pinned NSH. Platform fallback/baseline/pixel parity remain native checks.

Four unrun development tests in `conversation_status/tests.rs` mount the production renderer with NSH Core/ApproxTextEngine and dispatch real Core layout events: location wraps beyond the bootstrap viewport and reports natural height; new source/color/width replaces the old projection; empty title/subtitle preserve token styling, centering, gap and full AX labels while expanding; invalid/old identity/width measurements are rejected. Event assertions permit Layout only, not pointer/key handlers. These are model tests, not GPUI Window, AX adapter, hitbox or native paint tests. Parent integration may run `cargo test -p vega_ui conversation_stream::conversation_status::tests` after module wiring, plus its complete required checks. Development tests and exclusive wiring must be removed before master after evidence capture.

### Actual checks

Both commands below exited 0 after final Rust changes:

```text
/Users/puzige/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/rustfmt --edition 2024 --config skip_children=true crates/vega_ui/src/conversation_stream/conversation_status.rs crates/vega_ui/src/conversation_stream/conversation_status/tests.rs
/Users/puzige/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/rustfmt --edition 2024 --config skip_children=true --check crates/vega_ui/src/conversation_stream/conversation_status.rs crates/vega_ui/src/conversation_stream/conversation_status/tests.rs
```

Formatter: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`. A read-only Python hygiene check exited 0: no comments or trailing whitespace in scoped Rust, no production unwrap/expect, no active-window rejection or animation-frame requests, four development tests counted. This is static hygiene, not behavioral verification. Read-only Git HEAD/status queries were used; no Git mutation, network, Cargo/rustc invocation, build, test execution, App launch or delegation ran. Other writers' concurrent worktree changes were observed and left untouched.

| Stopped source | SHA-256 |
| --- | --- |
| `conversation_stream/conversation_status.rs` | `a178585a9748c73ce21c837ebe3d88811cb6a520a7037785493c1f10c19e1732` |
| `conversation_stream/conversation_status/tests.rs` | `b25116f73205251fb8517f8d01bd9832b2a8127d62b4af1042b3448f81bc3e9e` |

No compilation, Clippy, tests, native acceptance or final-head CI ran in the original writer's cycle. The parent subsequently executed its existing four regressions in the application suite. Original identities above are historical handoff identities, not the later source below. Native input propagation, focus/AX, visual typography/background fidelity and platform acceptance remain unproven.

## Parent omitted-thinking implementation and actual validation

The parent implemented the continuation above. Only the `truncated` branch's visible text child changed in `run_activity.rs`; its condition, StickySection placement, owner, expansion, children and scroll controllers did not change. The shared passive text host gained one independently keyed variant, with NSH-owned top spacing and no location padding. No new product wording or control was added.

Actual toolchain: official Cargo `1.98.0 (797e8a9bc 2026-08-05)`, rustc `1.98.0 (88d9e12ae 2026-08-18)`. After scoped rustfmt, the parent executed:

```sh
CARGO_FORK_AUTO_UPDATE=0 rustup run 1.98.0 cargo test -p vega_ui --lib conversation_stream::conversation_status::tests --offline --locked -j 2 --target-dir target/official-nsh-validation
```

Exit 0, build 11.53 s, **5 executed / 5 passed / 0 failed / 0 ignored / 254 filtered**, 0.01 s. This includes the original four status regressions and the new omitted-thinking actual-renderer case. Raw local output: `target/nsh-omitted-status-test.stdout` and `.stderr`. Thirty existing application warnings remain; no strict Clippy, full-suite current-delta run, exact-head CI or native acceptance is implied. `git diff --check` on the three affected Rust paths exited 0. Parent stopped these paths for later focused independent review.

| Current stopped source | SHA-256 |
| --- | --- |
| `conversation_stream/conversation_status.rs` | `1f84386353d22776d31fda104832e0bf2e95c38175b83d7e9378c5866b990753` |
| `conversation_stream/conversation_status/tests.rs` | `bb692907e82e1197cd1d2e4d96821a117186f167fde2da435d04da123c7305ef` |
| `conversation_stream/run_activity.rs` | `c92753351b9ac4618b3f59aab2da3bc1299a0d8359b67c9de312e18a6e314c2c` |

This increment is implemented, locally validated and independently source-reviewed with no findings (`01M6WJF0BH4559WQKQVJQJM3YC`). The non-implementer checked the three hashes at both beginning and end and reported no change. That reviewer executed no tests and explicitly noted that the model fixture uses Core renderer + ApproxTextEngine, not actual GPUI/native paint. Simultaneous-row host behavior, native AX/Tab/input transparency, pixel fidelity and three-platform acceptance remain unproven. Development tests remain for recoverable archival and final pre-master removal.

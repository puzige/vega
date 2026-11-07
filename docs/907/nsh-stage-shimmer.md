# Stage / shimmer production text leaf

## Scope and pinned source

Base HEAD: `4148291cc5b377362d0ac64aa88b285cc9e06532`.

The single writer owns only `conversation_stream/shimmer.rs`, its `newshimmer/` implementation, and this document. The scoped review-fix authorization additionally permits a narrow opt-in transparent constructor in `crates/vega_ui/src/nsh.rs`; existing `Surface::new` defaults must remain unchanged. Thinking/ThinkingBody, run activity, core, conversation module wiring and all other shared NSH changes remain outside this leaf. Current production `stage_text(String, Instant, &Window, &App)` and `StageText::{new,set}` stay source-compatible. The private `text` alias initially retained for Thinking has no remaining caller after its header migration and was removed during parent strict-lint integration. Mutable host setup is deferred to a GPUI `RenderOnce`; callers and callbacks do not acquire new mutable parameters.

This specification precedes implementation and refines [the full migration specification](nsh-full.md). No dependency changes, fork, shader, private NSH renderer access, new product copy, network, Git mutations, application launch, builds, test execution or delegation are authorized for this writer.

## Acceptance

- The production `stage_text` entry point and animated/nonanimated `StageText` branches display actual NSH text, not a preview or a GPUI text child. GPUI may measure with the native text system and host/clip the NSH surface; it must not paint this leaf's glyphs.
- Keep current source text and complete accessible label. Updates to text, stage state, colors and epoch must not keep a previous title cached as the authoritative projection.
- Retain `display_layout`'s native glyph-cluster/UTF-8 boundaries, ellipsis measurement, empty/nonpositive/too-narrow suppression, original line's 1px fit allowance, and bounded logical-prefix search. Never substitute character-count truncation or per-character shaping.
- Keep the canonical highlight: `center = fract(elapsed_ms / THINKING_SHIMMER_MS) * 1.6 - 0.3`, strength `max(1 - abs(x / original_width - center) / 0.24, 0)`, RGB interpolation, unchanged base alpha. Ellipsis uses the displayed layout's position and the original full width as denominator. Native color emoji are not tinted into monochrome glyphs.
- Shape/measure using the inherited native font and conversation typography/row-height tokens. NSH text uses corresponding family, weight, italic and supported OpenType variants; do not split shaping into separate character elements laid out in a row. Nested spans belong to one text block.
- Clip to the element and inherited content mask. Empty/offscreen text, inactive windows, Reduce Motion and absent epochs are static base-color text and request no continuing animation. Hidden leaves have no engine-side interval/timer or background shimmer loop; the host drives visible frames only. Resuming uses the supplied epoch rather than restarting a local animation.
- Normal and stage entry points retain their existing theme pairs: tertiary/primary and conversation-stage/base-background. No new user-facing explanatory text.
- This passive text leaf must not occlude ancestor hitboxes, consume their clicks/wheel/pinch gestures, or add a Tab stop. Prepaint the NSH child under an empty content mask, then paint under the original visible clip; do not remove its actual NSH text paint or ancestor event handlers. Use only public GPUI APIs. Preserve readable, noninteractive text accessibility: no press handlers, focusable control roles or custom activation actions.
- The shimmer surface must opt into `AppOptions::transparent = true` so dark, hover and keyboard-focus backgrounds remain the ancestor's. Other surfaces keep the opaque default. Static source/API evidence is not native input, focus, accessibility or pixel acceptance.

## Verification and limitations

Current parent integration results are recorded in [the batch validation record](nsh-header-batch-validation.md): all five stage tests executed successfully in the 203/203 full run, and the stopped tree passed workspace fmt and strict Clippy. Writer-local UNRUN statements below are historical scoped reports, not the current parent execution status. Independent correction review statically closed the input-isolation and background P1/P2s after tracing ordinary/retained input, paint-mask timing and AX in verified pinned dependency sources; no new P1/P2 finding. Native behavior/fidelity and public API runtime coverage remain unverified.

The writer may add meaningful development regressions, but cannot execute them. Parent-owned integration must compile and exercise the fixed public APIs, Unicode/glyph-cluster truncation, canonical highlight/ellipsis, source updates, stop/resume gates, clipping, hidden freeze, native font and theme sizes. Existing baseline 175 passing tests are not evidence for this delta. Any new Rust development tests must be removed before master under repository policy.

Pinned NSH 0.4.0 public text spans support foreground colors independently of shaping metrics, and its GPUI text painter chooses each glyph's span color while preserving native color emoji. This is the intended production route, not a shader or approximate whole-label pulse. Arbitrary GPUI OpenType features outside NSH's public `fontVariant` set and exact platform font/bidi/pixel parity require explicit assessment; unsupported fidelity must be recorded as a blocker, not reported as passed.

## Initial implementation and historical stopped-version evidence

- `shimmer.rs` now routes both fixed production functions and both `StageText` branches to the native adapter. `display_layout` is byte-for-byte unchanged; only the color output replaces GPUI `DecorationRun` construction. There is no leaf glyph-paint call, canvas, shader or GPUI text fallback.
- `newshimmer/mod.rs` defers host creation to `NativeRenderOnce`, retains one Surface across text/epoch/theme projections, clears stale displayed content when input changes, publishes only changed measured snapshots, and stops the surface on release. Its rendered element holds a weak host reference to avoid a retained-render self-cycle.
- Native nested foreground spans form a single NSH text paragraph. The displayed native measurement plus the existing 1px tolerance determines the text box width; enclosing NSH and GPUI masks clip to the actual available bounds. Host frame requests require a supplied epoch, glyphs, visible geometry, platform window visibility, active window and no Reduce Motion. There is no engine-side animation/timer.
- Five development tests were added, **not compiled or executed**: shaped-cluster truncation; empty/narrow/1px fit boundaries; canonical highlight and ellipsis colors; motion gates; and actual production NSH text mounting/source replacement with typography and accessibility inspection. The text-mount test uses `ApproxTextEngine`, not native pixel/shaping evidence.

Actual static commands, all exit 0 on the stopped source:

```text
/Users/puzige/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/rustfmt --edition 2024 --config skip_children=true <three owned Rust files>
/Users/puzige/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/rustfmt --edition 2024 --config skip_children=true --check <three owned Rust files>
git diff --check -- crates/vega_ui/src/conversation_stream/shimmer.rs
```

Formatter: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`. A read-only Python inventory also confirmed unchanged `display_layout`, no comments/trailing whitespace in the three Rust files and no former `layout.paint` call. No Cargo/rustc invocation, build, test, network, Git mutation, application launch or delegation was performed. Other writers' concurrent changes were observed but not modified. HEAD remained `4148291cc5b377362d0ac64aa88b285cc9e06532`.

Stopped Rust SHA-256:

| File under `crates/vega_ui/src/conversation_stream/` | SHA-256 |
| --- | --- |
| `shimmer.rs` | `9ea8439004bb23fd43b02141d7c1594b64ea8bca43a3483b499289f3f6c8a875` |
| `newshimmer/mod.rs` | `158c47168377dc2f01a04c4375c74c1ebf455c67ae81b38ab905a8676b52e2d7` |
| `newshimmer/tests.rs` | `5f2069fe3fe7ab1dd6f88856440839a67b69fda793911730507ae94b0fdfea01` |

## Integration needs and fidelity gaps

1. Parent must compile/check the exact stopped leaf and execute the five development tests under its coordinated load. Baseline 175 passes are not this delta. No manifest changes are needed; the scoped review fix adds the opt-in shared `Surface::new_transparent` constructor described below.
2. **Fixed-public-API runtime coverage remains missing.** Existing dependency configuration has no GPUI `test-support` feature; this writer cannot change manifests/shared harness wiring or run an app. The tests exercise actual NSH production rendering and retained algorithms, but do not instantiate `text`, `stage_text` or `StageText::set` with real GPUI Window/App/Context values. Parent must supply a permitted harness and cover unchanged title/new epoch, changed title/same epoch, live-to-static and static-to-live transitions.
3. **Exact shimmer fidelity is not accepted yet.** NSH performs its own bidi/line placement after native shaping, uses packed foreground colors (channel quantization differs from GPUI float decorations), and selects platform fallback fonts internally. Original full-width highlight coordinates are retained, but exact glyph placement/baseline, emoji fallback and pixel parity require native verification on all three desktops. The additional 1px inner text allowance prevents the legacy fit tolerance from triggering an ordinary second wrap; it is not proof of equal NSH/native shaping widths.
4. **Known public typography limitation:** pinned NSH accepts only `smcp/onum/lnum/tnum/pnum` enabled variants and boolean italic. Arbitrary GPUI feature settings (including disabled features), oblique-vs-italic and some fallback font resolution cannot be faithfully expressed. If active production callers rely on these, font/shimmer fidelity is blocked under the no-fork/no-upgrade restriction; do not label the mapping an equivalent approximation. Linux `.SystemUIFont` may also select NSH's desktop UI family rather than the GPUI measurement face. Parent must establish the actual supported production font contract or escalate; no private backend workaround was introduced.
5. Parent/independent review must verify no hidden/offscreen recurring engine work, window activation/visibility recovery, Reduce Motion transitions, clipping during resize/scroll and absence of stale async pixels on source changes. Snapshot publication is asynchronous by the shared Surface contract; fresh projection code alone does not prove immediate native paint replacement.
6. ThinkingBlock's nonshimmer title and body are another writer's scope. Parent must preserve this leaf's entry points there and complete the other writer's visible production migration. Run activity/core/module/ShellChrome edits were not made by this writer; the only shared edit is the separately authorized transparent Surface constructor.

## Scoped review fixes: passive embedding and transparent background

The review-fix acceptance bullets above were recorded before changing Rust. Only `newshimmer/mod.rs`, `newshimmer/tests.rs`, the authorized constructor portion of `nsh.rs`, and this document changed in this cycle. `shimmer.rs` is byte-identical to the initial stopped source.

### Exact Rust delta from the initial stopped leaf

- `newshimmer/mod.rs`: replace `Surface::new` with `Surface::new_transparent`; immediately apply `surface.focus_handle(cx).tab_stop(false)`. Replace the single child prepaint call with the same call inside `Window::with_content_mask`, using zero-size bounds at the inherited mask's center. Native measurement, snapshot publication, visible/motion gates, and child paint under the original element/inherited clip are unchanged. No ancestor handler or legacy glyph paint is added/removed.
- `nsh.rs`: retain `Surface::new` and delegate it to a private `with_transparency(..., false, ...)`; add `Surface::new_transparent` delegating with `true`. The common existing construction body gains only the `transparent` AppOptions field. Queue, errors, locale, appearance and lifecycle remain unchanged.
- `newshimmer/tests.rs`: extend the existing production text mount/update regression with assertions that each emitted event set is empty, each emitted accessibility action list is empty, roles remain noninteractive defaults, and text is not accessibility-hidden. Existing content/style/full-label assertions remain. Still five development tests, all unrun in this cycle. These assertions exercise NSH mount output, not actual host input/AX behavior.

### Concrete pinned public-API evidence

All inspected dependency sources are the locally installed `nsh-platform-gpui-0.4.0` and `gpui-fast-0.1.9`; no dependency source was modified.

- NSH `render.rs:741–775` does real surface resize/tick/accessibility prepaint and then inserts `HitboxBehavior::BlockMouse`; `events.rs:150–188` requires `is_hovered_at` before mouse-down focus/consumption, and `events.rs:301–329` requires `should_handle_scroll` before unconditional wheel consumption. `events.rs:332–345` gates pinch initiation on hover. With a new passive surface that cannot begin a press/pinch, release/continuation paths cannot acquire a gesture independently.
- GPUI `window.rs:4213–4232` publicly permits `with_content_mask` during prepaint/paint, intersects the supplied mask and restores the original mask on return. `window.rs:5346–5359` records the input mask when inserting a hitbox. `window.rs:1180–1204` intersects hitbox/mask before testing; `geometry.rs:1468–1473` uses half-open bounds, so zero area contains no point, including its origin. `Hitbox::is_hovered_at` and `should_handle_scroll` use these hit-test results (`window.rs:869–887`). Composition also intersects masks and drops empty rectangles (`fast/composition.rs:654–661`).
- Retained list input can expand mask edges coinciding with viewport edges (`fast/layers/input.rs:317–371`). The zero-size mask is placed at the inherited clip center, interior to a nonempty visible clip, rather than the default origin; it therefore does not coincide with viewport edges in the visible leaf case. Retained hitbox translation preserves mask size (`fast/layers/input.rs:91–112`). This is source evidence, not a verified native retained-scroll result.
- NSH `AppView` starts with a tab-stop handle (`app.rs:308`) and publicly returns its clone (`app.rs:699–702`). GPUI `FocusHandle::tab_stop` updates the shared map (`window.rs:595–600`), and subsequent clones read that map (`window.rs:569–580,650–654`). No focus request is added.
- NSH public `AppOptions::transparent` defaults to false (`app.rs:162–186`); `render.rs:798–800` skips only the surface background when true. Actual NSH tree/text draw remains in `render.rs:814–829`. The shimmer GPUI child paint call and visible clip are unchanged.
- NSH AX text maps to Label and the labeled container to Group, not controls (`access.rs:199–217`); neither is in `focusable` (`access.rs:159–181`). Without press events/custom actions, `describe` does not add Click/Focus (`access.rs:535–599`). NSH still adds `ScrollIntoView` to ordinary AX nodes; this is not an activation action. The fix does not hide accessible text. The assertions and this source evidence do not establish native AX reading/focus acceptance.

### Current stopped source and actual checks

Executed successfully (exit 0), using only the three scoped Rust files as explicit paths:

```text
/Users/puzige/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/rustfmt --edition 2024 --config skip_children=true crates/vega_ui/src/nsh.rs crates/vega_ui/src/conversation_stream/newshimmer/mod.rs crates/vega_ui/src/conversation_stream/newshimmer/tests.rs
/Users/puzige/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/rustfmt --edition 2024 --config skip_children=true --check crates/vega_ui/src/nsh.rs crates/vega_ui/src/conversation_stream/newshimmer/mod.rs crates/vega_ui/src/conversation_stream/newshimmer/tests.rs
```

Formatter: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`. A read-only Python check printed the exact three-file review-fix unified diff, reconstructed and verified the initial stopped `newshimmer` hashes, verified unchanged `shimmer.rs`, and found no comments/trailing whitespace in the three changed Rust files. No Cargo/rustc/build/test/Git/network/app/delegation operation ran in this cycle. Baseline HEAD was read directly from worktree/ref metadata as `4148291cc5b377362d0ac64aa88b285cc9e06532`, without a Git command.

| Current stopped file | SHA-256 |
| --- | --- |
| `crates/vega_ui/src/nsh.rs` | `0414ef2b42b488eac82d779784da4bc9c5ca1097d92aa921681af0bf80c63136` |
| `crates/vega_ui/src/conversation_stream/shimmer.rs` (parent integration: unused private alias removed) | `3a58337940fdbc4742bb45917092b3c604e44a5b1bc976d3ca1f66072b183632` |
| `crates/vega_ui/src/conversation_stream/newshimmer/mod.rs` (parent integration: unused import removed) | `1897c1f2fd3e5293810ff35dac575850d4c6771cb71d4a56ce98e4dd4e370604` |
| `crates/vega_ui/src/conversation_stream/newshimmer/tests.rs` | `2cef626cac3ee6a5fa1ec5eeea9cd0f73f4b3b994b8ae6b4f33546d7c5330dfe` |

Parent integration must compile this exact source, run the five tests, and verify actual production header mouse-up over the title, wheel/pinch/list scrolling (including retained/recycled rows), Tab/Shift-Tab without a text stop, accessible noninteractive text/full label, and dark/hover/focus background continuity. No GPUI test-support feature or native harness was added. Actual public-function runtime coverage, exact shimmer/font/pixel fidelity, hidden freeze and the other gaps above remain unresolved by static evidence.

Builds, tests, application/native checks, independent review of these fixes and final-head CI remain unrun by this writer. The leaf is an implementation candidate with the above explicit acceptance gaps, not a fidelity/production-completion claim. Rust development tests and their exclusive wiring must be removed before master after parent validation and evidence capture.

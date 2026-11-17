# NSH conversation foreground chrome

## Scope and acceptance

This leaf replaces only the visible conversation foreground fade/base and context usage ring/tooltip. Parent owns `conversation_stream/mod.rs` and `render.rs` wiring. No vendor, dependency, wheel protocol, composer commands, model selection, accounting, source data, reading geometry, or foreground measurement changes.

Acceptance established before implementation:

- Keep the fade origin at `READING_NAVIGATION_HEIGHT - CONVERSATION_BOTTOM_FADE_HEIGHT`, its original height, transparent-to-`bg_base` direction, and the opaque base beginning below the resume-tail navigation. Parent retains the foreground height measurement, reading-owner clipping and the original foreground layout/occlusion contract.
- Every visible fade/base/ring/tooltip pixel comes from NSH. GPUI hosts only layout, clipping, pointer/focus/key events and native accessibility. Passive NSH surfaces have no input mask in prepaint, paint with the real clip, no tab stop and hidden NSH accessibility subtree. They never cover existing controls with an interactive native surface.
- Reuse `ContextUsageDisplay` without estimating or inventing usage. Preserve track radius 6, width 2, 16-point icon in a 28-point slot, the top-start clamped progress, original tooltip lines, accessibility label, theme colors, widths, spacing and centered above-trigger placement snapped to the window.
- Context usage has no click/activation command. Pointer hover and native keyboard focus reveal the local tooltip; Escape dismisses it without sending a model command or stealing focus. Hover/focus leave rearms the corresponding reveal. Preserve the existing context usage focus handle and composer Tab ordering.
- Tooltip and feedback require the current owner/root/route, current display and language, active visible window and live clipped mount. Replaced source or revoked/hidden mount cannot make queued stale callbacks show old information. No callback bypasses the root authority predicate or reads list geometry during prepaint.
- Stable host rendering does not publish identical surface state. Theme/data/focus/hover changes publish changed visual state; geometry bookkeeping does not notify or create a render/publication feedback loop.

## Design

`foreground_chrome.rs` owns transparent NSH decoration surfaces with an equality-gated palette. `foreground_chrome/context_usage.rs` owns the usage host, real native focus proxy, mount lease, and two passive NSH surfaces. Ring arcs use the fixed NSH `Painter`; fade uses fixed NSH gradient style. The tooltip consists of actual NSH text and background/border/shadow, not GPUI text/canvas or legacy controls inside an NSH wrapper. No backend patches are needed.

Only request/service isolation is relevant to other integration fixtures; this leaf sends no requests. Native pixels, platform AX and keyboard acceptance remain user validation, not inferred from static checks.

## Parent wiring

Parent-only edits, not performed by this writer:

1. Add `mod foreground_chrome;` beside `mod render;` in `conversation_stream/mod.rs`.
2. In `render_conversation_foreground`, remove the GPUI `linear_gradient` local. Keep the existing absolute fade wrapper, its exact top/height/width, and replace `.bg(fade)` with `.child(super::foreground_chrome::decoration(super::foreground_chrome::Decoration::Fade))`.
3. On the column immediately following `render_resume_tail`, replace only `.bg(colors.bg_base)` with `.relative().child(div().absolute().inset_0().child(super::foreground_chrome::decoration(super::foreground_chrome::Decoration::Base)))`. Keep this NSH background as the first child, before the notice/privacy/composer/commit content. Keep `.occlude()`, all column layout, all content and the outer foreground measurement canvas unchanged. Remove the local `colors` only if no longer otherwise used in that method.
4. In `render_model_controls`, keep `.context_usage_visible()` and `render_model_selector` exactly as they are. Replace `self.render_context_usage_indicator(&display, colors, window, cx)` with `super::foreground_chrome::indicator(cx.weak_entity())`. Remove now-unused display/colors locals. Remove the old GPUI `render_context_usage_indicator`, `context_usage_ring` and `context_usage_tooltip_element` methods plus imports used only by them. Do not retain a fallback/second visible rendering path.
5. Keep the existing `context_usage_focus`, composer control ordering, original controller close/reset paths and outer Escape route. The new mount registers a real bubble-phase key callback on its native dispatch subtree for focused-proxy Escape dismissal. Hover-only dismissal while another composer field retains focus continues through the existing conversation-root Escape route, which clears `context_usage_trigger_hovered`; the host observes owner notifications and honors that clear on its next render. Do not remove that outer route during wiring. The new focused callback refuses to dismiss an IME composition and does not blur or activate a control.

The bound source verifies the parent-installed reading-navigation authority, current display/language and binding before mounting the host; interactive/tooltip gates additionally use `ReadingOwner::root_current`, active/visible window, exact projection ID and pointer-identical live mount with nonempty clip and current viewport/scale. The authority is the existing root/stream predicate, not a new privilege. No list geometry is read. Native focus uses the original focus handle/Tab index and supplies a full accessible label without a fake Button role or click action. NSH visual subtrees are AX-hidden to avoid duplicates.

The tooltip is deferred at the original priority 2 and anchor/gap, with original 3 lines and measured minimum width. Its NSH font family matches the GPUI text measurement. Both deferred prepaint and paint recheck source/mount/reveal. Persistent element state holds the live lease; the host retains only a weak reference so unmounts do not reauthorize a hidden element by equal numeric geometry. Clip bookkeeping itself has no notify/publication. `Reveal` independently tracks hover/focus suppression until a real leave/re-entry, so unrelated repaints cannot undo Escape.

## Validation

Implemented three development-only pure-state regressions in `foreground_chrome/context_usage/tests.rs`: Escape survives repeated identical hover/focus, leaving one reveal source preserves the other, and a revoked/reset reveal does not retain old hover state. These are not mounted/native acceptance and have not been executed. Parent can run its same official offline/locked lib+host+geometry integration command after adding the module, with a focused lib filter `foreground_chrome::context_usage::tests` for these three definitions.

Scoped rustfmt parsing/formatting and `--check` passed with `rustfmt 1.8.0-stable (1159e78c47 2025-09-14)`, edition 2024, `skip_children=true`, explicitly limited to the three new Rust files. Text hygiene passed (final newline, no trailing whitespace, no code comments or unwrap/expect). Static API inspection used fixed Fast 0.1.9 and the current pinned NSH source. No Cargo, typecheck, tests, native launch, Git or network operation was run. Cargo and integrated regressions belong to the parent to avoid concurrent builds. Native visual clipping/gradient endpoints, actual hover/Tab/Shift-Tab/Escape, platform AX, engine delivery ordering, light/dark and narrow-window tooltip placement remain unverified. Development Rust tests must be removed before master under repository policy. Wheel remainder remains an existing unimplemented, unauthorized backend boundary; this leaf does not change it.

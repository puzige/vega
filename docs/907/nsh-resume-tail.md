# NSH resume-tail leaf

## Scope and baseline

Spec before implementation, based on `0b5ea426abfd8d4358489e916273d89474fb190f`, [full migration spec](nsh-full.md), [execution guide](../vega-exec-guide.md), and [design guidelines](../vega-design-guidelines.md). Only the new `conversation_stream/resume_tail.rs`, its children, and this document belong to this writer. Parent owns all module/controller/render wiring. No network, Git mutation, compilation, application execution, delegation, dependencies, keymaps, fork, or business algorithm changes.

## Acceptance

- Preserve exactly `!following_tail() && !placement.automatically_placed() && !placement.at_reading_bottom(&list)`; preserve the 40px navigation slot, full-size centered row with 4px vertical padding, 32px circular control, subtle border, elevated/hover/active backgrounds, accent focus, secondary ArrowDown icon, and localized `ui.back_to_bottom` label/tooltip. Geometry uses existing Layout tokens.
- NSH constructs visible content; native GPUI supplies only Surface embedding, clipped geometry, tooltip, focus and authoritative Button AX proxy. Hide the NSH mirror AX subtree to avoid duplicate buttons. Native Tab/Shift-Tab uses the original `resume_tail_focus`; Enter/Space, AX Click and pointer all reach the same guarded dispatch. No new keymaps.
- Freeze owner identity, thread, independent reading-navigation epoch, preview/permission state and trusted mount binding. Recheck original show condition, settings, current root/stream and modal authority. Resume-tail is a read-only navigation operation: preserve it in the existing child reading preview and after write permission revocation. The trusted root bridge must accept only the exact current main stream or the exact currently visible child reading-panel stream; matching a child thread ID against the main OpenedThread is not sufficient. A required parent-supplied live authority predicate supplies the window-root/current-stream/palette boundary that cannot be inferred inside `vega_ui`. It must be read-only and must not substitute visibility guesses for real window state.
- Require live mount, current native painted geometry, clipping, window identity/size/scale, list viewport and foreground height, matching NSH layout acknowledgement and healthy Surface. Reject inactive-window business activation, but accept valid inactive layout measurements. Consume authority before invoking original `ConversationStream::resume_tail(cx)`; never duplicate its scrolling/anchor algorithm or retry business actions.
- Hiding/recycling synchronously revokes publication and mount authority. A retained Host never creates its own new mount or adopts a changed owner epoch. Only the trusted parent `element` call may bind a new mount/epoch. Explicit parent `hide` is mandatory when the whole navigation slot/page is omitted; paint/mount guards are defense in depth, not a replacement for this call.
- Engine command sends are rooted in the original renderer Owner, not a disposable dynamic subtree. Hiding stops presentation only, never background conversation work.

## Required parent API integration

Implemented API: parent-owned `ResumeTailUi::default()`, `element(stream, authority, window, cx) -> AnyElement`, and `hide(cx)`. The required authority function receives the weak stream plus current Window/App and must verify actual root ownership/current stream and absence of palette or other blocking modal. The leaf additionally checks global settings/delete-confirm and its own frozen stream boundaries. Main-route and child-panel identity are checked by the required trusted root bridge. Missing authority must not default to true.

Keep the existing `container_query` and its outer full-width `READING_NAVIGATION_HEIGHT`/nonshrinking slot. Invoke `element` from that deferred callback with its real Window. Do not cache the old content closure as a second trusted rebind path. Parent must own persistent leaf state, call `hide` on every hidden/replaced/page-omitted path, and provide a fresh trusted element invocation after resuming.

Remove the old visible button and its direct mouse-up/action attachment. Existing `ResumeTailButton` Enter/Space bindings may remain only if a leaf native proxy intercepts `ResumeTail` and dispatches through guards. Preserve the separate ConversationStream cmd-down business shortcut and its original semantics. The leaf native proxy has its own `ResumeTail` action listener and stops propagation, as well as an unmodified-key Enter/Space handler. Keep existing `ResumeTailButton` key bindings, but remove the old direct `on_action(Self::on_resume_tail)` and `on_mouse_up(Self::on_resume_tail_clicked)` attachments with the old content. Do not attach the unguarded method above the new proxy as an alternate button activation path.

## Exact integration contract

```rust
ResumeTailUi::default()
ResumeTailUi::element(
    &mut self,
    stream: WeakEntity<ConversationStream>,
    authority: fn(&WeakEntity<ConversationStream>, &Window, &App) -> bool,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement
ResumeTailUi::hide(&mut self, cx: &mut App)
```

Types and methods are `pub(super)` inside `conversation_stream::resume_tail`. Add `mod resume_tail;` in the parent-owned module. The parent must choose persistent UI ownership and expose any minimal trusted root authority bridge needed across `vega`/`vega_ui`; this leaf cannot inspect `VegaWindow` or its private palette/stream ownership. The authority callback must check identity, not just matching thread IDs, and must return false for all window modal/palette blockers. It must remain callable while the window is inactive (measurement authorization is independent of business foreground authorization).

**Borrowing requirement:** call `element` outside a `ConversationStream::update` closure and outside any active read of its Entity. It upgrades and reads the stream itself. Likewise the authority callback must not attempt to read a currently mutably borrowed root. A safe parent approach is parent-owned leaf state captured by the container-query callback, with a trusted read-only window-root bridge evaluated during that callback and later Host dispatch. Do not simply replace the old `stream.update(... render_resume_tail_content ...)` body with `element` inside that same update. Persistent parent state must not be allocated once per frame, and `hide` must be called from every route/settings/preview/modal/page omission path before exposing replacement content. This required trusted parent integration is not implemented by the leaf and is an explicit delivery dependency.

Implementation has three files: Host/source/dispatch in `resume_tail.rs`, lease-only native Element in `resume_tail/mounted.rs`, and NSH content in `resume_tail/pixels.rs`. Native transparent proxy hover/pressed state is explicitly mirrored to NSH; these transient states do not invalidate activation identity or turn normal pointer-down/up into a new authorization epoch. Focus changes do invalidate identity. Previous positions preserve native focus while a new focus projection awaits layout acknowledgement, but cannot authorize activation until that acknowledgement matches. Native geometry is cleared in prepaint and set only in paint; geometry changes revoke published/displayed authority. Only the trusted parent entry creates a lease or replaces a retired/changed-epoch Host. Accepted dispatch consumes revision before calling the unchanged original method, then retires the Surface and lease.

No development Rust tests were added because this writer cannot compile/run them. This is not a test pass or independent review.

## Validation plan

This leaf performs only scoped Rustfmt and static inspection. Parent performs Cargo compilation and any selected development tests, independent review and final-head CI. Native Tab/Enter/Space, tooltip, pointer, AX, clipping, foreground transitions and platform acceptance remain unrun until actual validation. No inherited test result counts as this leaf's validation.

## Parent integration correction

The original leaf accidentally rejected every reading preview and permission-revoked stream, whereas the old production button was a read-only scroll operation in both contexts. Parent preserves the original eligibility, freezes those states to reject stale callbacks, and supplies actual main-route or visible child-panel entity ownership. The already-existing `set_reading_preview` transition synchronously suspends old presentations. Repeated preview rendering still revokes business presentations and rotates their run-notice epoch, preserving the former safety behavior. Read-only navigation has a separate `reading_navigation_epoch`, rotated by every full/navigation suspension. Its deferred container-query closure captures that exact epoch and cannot rearm after hiding. Parent keeps preview business controls gated while allowing this read-only leaf. Persistent state is shared by the deferred container query and controller, and omitted empty main pages hide it synchronously. Palette/delete/loading/page replacement paths also hide it without replaying scroll commands.

## Independent-review integration corrections

- Conversation-level `ResumeTail` routing is installed separately from the guarded button proxy. It validates the captured navigation epoch, active window and live root authority before invoking the original scroll operation, without requiring that the button itself be visible. The button proxy stops propagation and remains independently fenced.
- Reading authority installation is independent of delegated business binding and idempotent for the same function. Both history-only child construction and existing/cached child rendering install it; the `delegated_unbound` gate cannot suppress read-only navigation.
- Both directions of an actual reading-preview transition suspend presentations before changing state, preventing preview false/true ABA. Fullscreen workspace omission synchronously hides the main navigation and live root authority rejects omitted main streams, while the exact visible child preview can remain usable.
- Strict workspace all-target Clippy after these parent corrections exited 0 in 5.31s; log `/private/tmp/vega-907-reading-review-fixes-clippy.log`. These are static/compile corrections, not native keyboard/AX acceptance. Original independent findings remain pending incremental closure.

## READY validation record

Source HEAD observed unchanged: `0b5ea426abfd8d4358489e916273d89474fb190f`. No commit was created. Parent and other agents have concurrent unrelated worktree changes; no claim is made about their validation or ownership.

Executed successfully:

- `rustfmt +1.98.0 --version`: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`.
- Scoped `rustfmt +1.98.0 --edition 2024` and its `--check` on the three leaf Rust files.
- Scoped `git diff --check` (new files are untracked, so supplemented below).
- Explicit read-only whitespace/final-newline inspection for all four new files, and Rust static hygiene inspection for comments, unwrap/expect, and test wiring: no findings. This is static inspection, not behavioral testing or compilation.
- Read pinned cached GPUI `on_release`, canvas, focus, mouse/key/action APIs and existing shell-chrome/run-notice guard implementations; corrected the release hook to use `App`.

Not run: Cargo/rustc compilation, Clippy, Rust or renderer/authority tests, CI, application launch, native focus/keyboard/pointer/tooltip/AX acceptance, three-platform runtime acceptance. Cargo/rustc executable versions were not queried; the actual executed tool is the Rustfmt version above. No new Rust tests or dev dependencies were added.

Stopped Rust SHA-256:

| File | SHA-256 |
| --- | --- |
| `resume_tail.rs` | `684ec24d30f62666975f22758f1bec41b638f5391cb41b29793559de7c0a220f` |
| `resume_tail/mounted.rs` | `79a9c1a83700dfd0c7412264f80300af68a5e1e09ea22094499e0e00be37d920` |
| `resume_tail/pixels.rs` | `38c85c8b69ad52d60b84238310d2a35f0f6008be4e3d13fa8066debff22ddb2a` |

READY means the leaf is ready for parent integration and compilation, not production wired, independently reviewed, compiled, merged or hand-tested. Parent must first supply the trusted current-root/stream/modal authority bridge, persistent UI ownership, deferred call outside an active stream update, and synchronous `hide` paths. This writer stops after recording final file hashes; parent owns subsequent edits and verification.

# #907 Native Composer selectors

## Scope and acceptance before implementation

This leaf owns the new `conversation_stream/composer_selectors.rs` and children, `thinking_slider.rs` and its new native children, and this document. Parent owns shared module registration and render-slot switches. No backend, dependency, manifests, editor, action chrome, source-anchor, wheel or HTML changes are included. Parent fetched the stated base before this leaf started; this leaf does not run Git, network, Cargo, native UI or another agent.

- Permission trigger and picker are actual NSH content, retaining current/saving labels, warning colors, all four mode descriptions, next-run/current-run context and learn-more text. Selection invokes the original `select_permission_mode` path, hence original SelectPermissionMode event, FullAccess confirmation, save/conflict/reload and feedback remain authoritative. No direct permission mutation.
- Model trigger and grouped list are actual NSH content. Preserve Closed/Slider/List, the existing activation/previous/next/close/select controller methods, selected model versus keyboard highlight, bounded scrolling/reveal and original focus restoration. Dispatch checks the exact captured ModelOption and full current options, not only an index or name. Pending/save/trusted-busy gates remain mandatory.
- ThinkingSlider retains the existing hidden business model and ThinkingTierSelected/ThinkingSliderTitleActivated events. Only NSH draws its title/model/track/dots/knob. Preserve Off, provider default, declared tier order, strongest-tier styling and real persisted preference choice. Pointer drag, keyboard and accessible actions use the same guarded business path; no legacy overlay slider.
- Host callbacks require exact presentation epoch/source, current window and foreground, live mounted geometry, actual measured native row and current intent. Epoch changes revoke queued callbacks, including ABA; stale mount/window/scroll/measurement/selection actions do not execute. Keyboard/AX activation must request fresh measurement before dispatch. The native list, not a GPUI row list, owns visible layout and scrolling.
- GPUI is a paint-free lifecycle, keyboard/focus/AX bridge. Existing warning, save, confirmation, conflict/reload and error surfaces are not removed or replaced with success claims.
- Development checks cover currentness rejection and real NSH projection semantics plus slider selection behavior. Parent centrally compiles/tests after all parallel writers stop; local work is limited to scoped formatting and static inspection. Native pixels, actual keyboard/AX behavior and cross-platform acceptance remain unverified until separately exercised.

## Integration and results

Implemented native permission/model selectors and native thinking track. ThinkingSlider retains its original business model and event subscriptions but its Render returns Empty: production must switch both slots together. The new native track captures the responder, previews positions locally without rebuilding the track, commits the final position through ThinkingTierSelected on release, and restores the persisted selection on gesture cancellation. This avoids destroying a drag on each persisted choice update. Default explicitly emits the existing `provider_default` preference. No storage is written here.

Parent wiring, in owned shared files (all three entries confirmed present during this continuation; no parent-owned file was edited here):

```diff
 mod composer_actions;
+mod composer_selectors;
```

Replace the body of `render_permission_status(&self, cx: &mut Context<Self>) -> AnyElement` with:

```rust
self.render_nsh_permission_selector(cx)
```

Replace the body of `render_model_selector(&self, _window: &Window, cx: &mut Context<Self>) -> AnyElement` with:

```rust
self.render_nsh_model_selector(cx)
```

Keep the existing context-usage wrapper and feedback/confirmation surfaces. The old picker drawing helpers become unreachable and can be removed by the shared-file owner; keep `toggle_permission_picker`, `select_permission_mode` and the original controller methods used elsewhere. The selectors' paint-free wrapper captures raw keys and model actions before the shared Composer handlers, while the NSH surface owns all visible text, icons, track, hover/press/selected/focus styling and scrolling. The existing permission trigger/options and model trigger FocusHandles are reused.

Currentness uses full Thread/options/defaults/reasoning projection and intent revisions, input entity/revision, save owner/pending state, busy flags, appearance, shared presentation epoch, fresh native publication and intent identities, exact host window/scale/viewport/foreground and a live per-mount geometry lease. A synchronous shared atomic revision changes on native row/content layout and scroll, so a queued activation is rejected even if a scroll notification has not reached the GPUI receiver yet. Rows are measured against the current NSH viewport; keyboard/AX request a new measured activation rather than dispatch by index. Parent must retain the old FullAccess warning/confirmation and thread-permission feedback surfaces.

Five development cases were added: exact provider replacement, publication/layout/intent ABA, keyboard versus AX intent, slider Off/default/declared centers, and actual NSH permission text/description/learn-more mounting. These cases have NOT been compiled or executed by this leaf. They are development-only and must follow the repository test-removal policy before master.

## Continuation author self-check

Resumed from the six exact byte hashes supplied in the handoff; no old edit was replayed. The parent's existing `target/nsh-selector-wiring-check.stderr` reports two leaf API errors, not a passing build. Read-only inspection of the fixed `vendor/nsh-core/src/element/builder.rs` confirms `Accessible::label` at line 1221; both unsupported `accessibility_label` calls were replaced with that API. Disabled controls now also use the real `Interactive::disabled` contract.

The continuation adds an unforgeable per-mount identity and matches commands to the current published snapshot, source epoch, live geometry and native layout/scroll revision. Native button AX activation and adjustable increment/decrement now request a fresh intent followed by measurement; they no longer masquerade as pointer activation. Height notifications carry the same full snapshot and native revision. Popup outside clicks close through the original focus/controller paths, and permission AX focus uses the original option FocusHandle. Slider arrow/Home/End requests are made before changing the intent, so publication matching does not reject the new request itself. Clicking the model trigger while List is open forwards to the currently mounted list host for exact-option measurement, instead of selecting directly by the controller's highlighted index. Existing Off/default/order/strongest styling and responder cancellation behavior are retained; the strongest gradient explicitly retains rounded ends. Trigger capsule radius uses the existing 28px height token.

All five existing development cases and their assertions remain; only the new mount field was supplied in their snapshot fixture. No additional review or agent was started. Final validation commands use direct official `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`, edition 2024 and `skip_children=true`, on the five owned Rust files only. A first auxiliary hygiene script falsely matched Rust dereference lines as comment starts; this was an observation-script error, not a product failure, and the corrected script checks actual comment prefixes. Neither formatting nor these static checks constitutes a compiled or executed behavior result.

Local validation is scoped official-toolchain rustfmt plus static source inspection only. No Cargo/rustc/build, Git, network, native execution or delegation ran. Parent must compile the registered module and run these tests, then check mounted Composer keyboard/AX, dragging outside track, cancellation, outside-click close, group order/reveal, busy/save/conflict/reload, and existing action/focus regressions. Actual native pixels and three-platform behavior remain unverified. Model/permission controls are not a source-anchor backend extension; that boundary remains untouched.

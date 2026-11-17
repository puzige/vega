# #907 Pending messages native presentation

## Approved scope and acceptance before implementation

This increment replaces the remaining pending status, pending queue rail, row controls and pending options popup pixels with same-version NSH presentation. Only `conversation_stream/pending_messages.rs`, its private child modules and this document are owned. Shared render/core/module wiring, running-send native controls, vendor, dependencies and the already delivered editor are not modified. Parent already owns running-send integration. No builds, Git, network or native launch are authorized for this leaf.

Required acceptance:

- Keep the authoritative pending projection, visible queue order, committed-generation filtering, pending echo reconciliation, stream ownership and durable promotion unchanged. Queue acceptance is not durable persistence and must not acquire a new success/durable label.
- Preserve resume/retry, recovered edit, edit/discard/steer and queue-default business entry points, submit barrier, backend and operation gates, original localized text, message identities, first image preview and extra-image count.
- NSH paints pending status/error/disabled reasons, row text/icons, background/border, menu and actionable control hover/pressed/selected/focus states. GPUI is only layout, scrolling, popup positioning, input/focus and paint-free accessibility bridge; the existing image thumbnail is already NSH.
- Every delayed action requires the original source entity, current epoch, full current projection and input/operation context, current live mount, measured viewport/foreground geometry and a displayed lease for that exact presentation. Explicit message identity, not row index, authorizes business actions. Stale menu/actions, replaced projections, hidden/unmounted surfaces, changed draft, inactive windows and failed native transports fail closed without replay.
- Keep original focus handles so existing composer traversal still works. Enter/Space, existing action bindings, Escape/outside-dismiss, popup traversal, AX focus/click and disabled behavior must use the same guarded path. A keyboard action requires current focus; composition must not become a shortcut side channel. Scroll/reveal must precede activation of an offscreen control.
- Keep truthful errors and recovery available until resolved; do not add implementation explanations or redundant success UI.

Development verification should cover production freshness/authorization and actual NSH rendering, including disabled reason/error, stale projection/source/epoch/mount, explicit item identity and no mount-time activation. Parent runs compilation and regressions after all writers stop. Native focus, AX, pointer, popup clipping and pixels remain actual integration checks, not implied by static validation or process-local rendering.

## Compile-wiring correction acceptance

Parent's actual `cargo check -p vega_ui --lib --offline --locked` diagnostics in `target/nsh-selector-wiring-check.stderr:111–177` identify five pending-module errors against the preceding READY version (`native.rs` SHA256 `90eab3cd41cb64aa0bd88e714b0670a1083ed1a9012b150d60dff06d5f8b5d28`, `native/pixels.rs` SHA256 `02f7ceade66cdb690e4f8332c5ee410a67246b6e00a60584bf728309401283fb`). Correct only the missing Core `Element` trait import, the three fixed-version `Styled::text_size` calls and the private stopping-state read through the existing `ComposerActions::is_stopping()` accessor. Preserve all production behavior, freshness gates, UI tokens and development test assertions. No dependency, shared-file or acceptance change is authorized. Parent owns Cargo verification; scoped formatting and source/API checks here do not constitute a successful compilation or test run.

Correction completed: imported Core `Element as _`, replaced exactly three `.font_size(...)` calls with `.text_size(...)` and changed the stopping snapshot to `.is_stopping()`. Actual local source confirms the trait export, `Styled::text_size(impl IntoReactive<Length>)` and `pub(crate) fn is_stopping(&self) -> bool`. Both `pending_messages.rs` and `native/tests.rs` remain byte-identical to preceding READY. No assertion, production authorization or rendering behavior was removed or relaxed; no new exports or parent wiring are required.

Current correction validation: two-path scoped rustfmt and four-path `rustfmt --check` exit 0 with the tool version recorded below. A read-only Python inverse-patch audit verifies that reverting only these five wiring corrections reproduces all four preceding READY Rust SHA256 values exactly; newline/trailing-whitespace checks also pass. The first audit attempt failed because its expected import string did not account for rustfmt wrapping `Styled as _` onto a second line. After reading the actual formatted import, only the observation script was corrected; the production files were not rewritten to satisfy that script. No Cargo command or Rust test was executed by this leaf, and parent's reported check failed before this correction. Further compiler/type errors are not ruled out by static checks.

## Implementation handoff: stopped, not validated by Cargo

Production `render_pending_status`, `render_pending_queue` and `render_pending_menu` keep their existing signatures and use the private `pending_messages::native` module declared in the owned parent file. No shared export/runner/module change is needed for these four ordinary Rust unit tests. The unused old GPUI `render_running_send` and its Submit-only helper variant are removed; parent already routes that control to its own native implementation. Queue authority, accepted-submission cleanup, edit recovery, echo reconciliation and durable promotion remain existing business methods. No new persistence acknowledgement or queue-accepted success label was added.

The native host captures the actual owner entity, epoch, full ordered projection, visible message identities, input entity/revision, recovered drafts, committed generation, errors, both global/default-with-draft-override strategies, run ownership, permission and operation context. A weak mount lease carries full bounds, clipped bounds, viewport, foreground and current menu anchor; activation requires exact current display publication and complete control visibility. Geometry/focus changes invalidate displayed state and publication identity. Pointer and AX click share guarded dispatch; keyboard additionally checks current focus, modifier/held-key policy and composition. Existing handles, composer traversal, menu traversal, Escape/outside dismissal and original image preview entities remain. Keyboard focus requests autoscroll before any new displayed acknowledgement. Errors and menu reasons use measured NSH content height; glyphs, text, background/borders and feedback pixels are native.

Static validation actually executed: scoped `rustfmt --edition 2024 --config skip_children=true`, then the same four-path `rustfmt --check`; both exit 0. Tool version: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`. A local read-only Python audit passed final-newline/trailing-whitespace, no-code-comments, no production unwrap/expect, removal of old visible GPUI paint calls and exactly four test definitions. The actual fixed Core API and GPUI Fast focus/AX APIs were read from local source; no dependencies were changed.

Development tests defined, ALL UNRUN (0 executed, not a pass):

- `conversation_stream::pending_messages::native::tests::delayed_action_rejects_reordered_projection_replaced_source_epoch_and_presentation`
- `conversation_stream::pending_messages::native::tests::action_requires_display_focus_and_exact_message_identity_not_row_index`
- `conversation_stream::pending_messages::native::tests::real_native_menu_keeps_disabled_reason_and_never_executes_on_mount`
- `conversation_stream::pending_messages::native::tests::real_native_status_keeps_original_error_without_durable_success_claim`

The two renderer tests mount production pixels through the actual Core `mount_root`/`commit`/component-event loop and inspect real text/accessibility output. They do not prove live GPUI hook scheduling or native focus. Archive/remove development Rust tests before final master delivery under repository policy, after recoverable validation evidence exists.

Parent next: compile and run the pending native test filter and necessary queue/edit/submit/durable regressions against a coherent stopped tree. Validate real resume/retry, edit/discard/steer, default toggle, draft recovery, queue order and thumbnails, disabled controls, errors, narrow/long localized text, both themes, pointer and AX actions, keyboard navigation/reveal, focus departure, stale popup/scroll/resize/hide/remount and IME safety. Current bounds equality intentionally refuses partly clipped controls until revealed; actual reveal ordering and native popup clipping are unrun. Transport failure is fail-closed but this leaf adds no new surface-error retry controller; parent must not count unverified failure recovery as accepted native integration. Compilation, Clippy, Cargo tests, native execution, CI and independent review are all unrun here by assignment. No Git, build, network, install/config or vendor/shared-file write occurred.

Stopped Rust SHA256 (relative to `crates/vega_ui/src/conversation_stream/`):

| File | SHA256 |
| --- | --- |
| `pending_messages.rs` | `01793c5bdaf1b8b38d20718cd4eee3605b4fcd0199fc34dfec4dfda5240cd590` |
| `pending_messages/native.rs` | `38dd42da8f5e0e4c7774e8c4a6918c762c9aba4adcd97c0988ad65491aa8b792` |
| `pending_messages/native/pixels.rs` | `2518285e344cdfbb7a434df11c854808f2101f2539d4e8a105f189385e4cf587` |
| `pending_messages/native/tests.rs` | `e6de4a79ded0953a8ac7613963de20567fd434174a13ef67d8265c862f073d38` |

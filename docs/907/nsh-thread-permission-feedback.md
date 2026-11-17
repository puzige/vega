# NSH thread permission feedback

## Scope and acceptance

Replace the production `render_thread_permission_feedback` failure message and explicit reload control with genuinely NSH-painted leaves. Only `thread_permissions.rs`, its new `thread_permissions/native*` implementation and this document are writable. Shared mod/render/core, other lanes and previous deliveries remain frozen. Supplied source HEAD is `0b5ea426abfd8d4358489e916273d89474fb190f` plus preserved dirty state; parent has already fetched and confirmed master ancestry. Initial `thread_permissions.rs` SHA-256 is `15d2517cb470348ee53b5f36b5269b7c117288a5422b6271c1b417608b3f38ca`.

- Keep all original permission business methods, error variants, focus handles and operation ownership. Reload calls only `reload_thread_permissions`; never change permission policy, silently retry, or treat accepted/pending/unknown as saved.
- Preserve `Phase::Saving` / `Phase::Reloading` and `progress_label`: these statuses are already rendered by the NSH composer permission selector. Do not duplicate status text in a new second feedback row or display failure recovery while saving/reloading. Idle has no feedback.
- Failure messages retain all five existing localized error mappings. The reload control retains its localized name, Enter/Space, Tab/Shift-Tab, existing `permission_editor.reload` focus and original return to `permission_editor.trigger` upon reload. No new product wording or editor.
- Bind every leaf to complete current Thread, complete Phase, operation counter, captured optional source Thread, owner/root/navigation epoch and reload FocusHandle. A repeated identical error after a newer save/reload must invalidate old actions. Compare complete source state, not an index or the business method's intentionally narrower source comparison.
- Before any pointer/keyboard/native AX action, require active current root/environment, current projection, healthy mounted Surface, matching host geometry/viewport/scale, live row lease and finite positive measured per-leaf bounds fully inside the clip. Reject stale, hidden, inactive, partially clipped and unmounted commands.
- Each message/button gets an independent NSH ElementRef measurement and native AX proxy with its actual leaf bounds, not composite bounds. Keep focus reveal and visible NSH focus/hover/pressed styling. Reject composition, modified activation and held repeats.
- Displayed acknowledgements must converge without invalidating their own projection or a publish loop. Renderer recovery is an explicit NSH reload-view action, rebuilding only presentation, never replaying permission I/O.

## Verification boundary

Parent owns Cargo/typecheck, integrated behavioral validation, final CI and centralized independent review after all lanes implement. This lane runs scoped rustfmt and static inspection only; no Cargo, Git, network, native app/AT, dependency/vendor/Kit/Fast/install/config changes or delegation. Static checks are not typecheck or native behavior evidence. Native per-leaf AX Focus/Click/bounds, keyboard/IME, focus reveal, scrolling, delayed actions, all errors and operation replacement, renderer recovery, narrow windows, themes/locales and all three platforms require actual later execution.

## Implementation followup — author handoff

This is implementation, not independent review. Parent assigned this stage after the prior writer failed with `source_version2`, released, and no reported unknown effects. All three supplied preserved-source hashes were verified before editing:

| File (relative to `crates/vega_ui/src/conversation_stream/`) | Preserved SHA-256 |
|---|---|
| `thread_permissions.rs` | `a42e48987919a0328188c805c93fa29bb3fa14c63da1c74b4cc5d9d926ac6f3b` |
| `thread_permissions/native.rs` | `33f72813d71c07f0425a482fad564c5ecc485d5cf15008dd8b17edec272affed` |
| `thread_permissions/native_leaf.rs` | `790be81cc9c82dc05f0e569e21891d965ea9c9c0b7910eadfb6a71dcf8fc6835` |

Only `thread_permissions/native_leaf.rs` and this handoff changed during the followup. The main file and native binding/render file remain byte-identical. All five existing error keys, localized reload, Saving/Reloading progress behavior, complete Thread/Phase/operation/source binding, root/navigation checks and original permission reload method remain intact. No new permission I/O or duplicate progress row was introduced.

### Defects corrected

- Activation now checks current composition and every modifier at the shared actionable boundary, covering queued NSH requests and native AX Focus/Click as well as the existing keyboard check. Pointer down/up inside this host also reject invalid actions before the Surface receives them, including modified/composing gestures; captures do not consume events outside the host. The existing key handler still rejects held repeats and character-input preference.
- Prepaint geometry changes now immediately invalidate the projection and displayed row, rather than only scheduling a host notification. Geometry A → B → A cannot resurrect an old acknowledgement between host renders. Mount, current geometry/viewport/scale and row-lifetime checks are retained.
- The production measured-bounds decision now rejects invalid host/clip/viewport/scale, nonfinite or nonpositive leaf dimensions, and any leaf not fully inside its own host, paint clip and viewport. Native AX bounds and activation use this same decision; no composite or fabricated leaf rectangle is substituted.

### Renderer recovery finding

`Host::live` correctly requires a healthy Surface, including for recovery. The preserved error watcher stops and removes the failed Surface, sets recovery, and invalidates. The next render creates a **new** Surface carrying the existing localized `ui.reload_view` button; that fresh Surface supplies its own mounted geometry, live row and measurement before recovery can activate. The healthy-Surface requirement therefore does not require the failed renderer to recover itself. It must not be removed: permitting commands from the failed/unmounted Surface would weaken the delivery fence.

Recovery `invoke` stops presentation, clears recovery and invalidates; the next render rebuilds ordinary feedback. It does not call `Binding::reload` or emit `ThreadPermissionReloadRequested`. Root/owner/navigation, complete source state, environment, mount, geometry, row and input fences remain required. Persistent failure of the replacement renderer is not claimed to be solved; actual fault-injected native execution remains required.

### Development regressions and actual verification

Three inline Rust development regressions call the production `Geometry::measured_bounds` decision (no source-string assertions or duplicate algorithm):

1. `measured_leaf_keeps_its_own_offset_and_size_in_window_coordinates`: independently expected translated origin/size and exact-edge acceptance.
2. `partially_clipped_or_out_of_host_leaves_are_not_action_targets`: partial horizontal/vertical clip, host overflow and viewport overflow rejection.
3. `invalid_leaf_measurements_and_host_geometry_are_rejected`: NaN/infinity, zero/negative dimensions, invalid scales, invalid clip and zero viewport rejection.

**These tests were added, not executed in this stage.** Parent must compile and run the filtered `thread_permissions::native::native_leaf::tests` set, retaining actual discovered/executed counts; remove these development tests/support before master per repository policy after preserving source evidence. They do not establish native AX/input/recovery acceptance.

Executed successfully (exit 0):

```text
rustfmt --version
rustfmt --edition 2024 --config skip_children=true crates/vega_ui/src/conversation_stream/thread_permissions/native_leaf.rs
rustfmt --edition 2024 --config skip_children=true --check crates/vega_ui/src/conversation_stream/thread_permissions.rs crates/vega_ui/src/conversation_stream/thread_permissions/native.rs crates/vega_ui/src/conversation_stream/thread_permissions/native_leaf.rs
```

Actual formatter: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`. `skip_children=true` confines formatting to the explicit assigned files. Final source hashes:

| File | Final SHA-256 |
|---|---|
| `thread_permissions.rs` | `a42e48987919a0328188c805c93fa29bb3fa14c63da1c74b4cc5d9d926ac6f3b` |
| `thread_permissions/native.rs` | `33f72813d71c07f0425a482fad564c5ecc485d5cf15008dd8b17edec272affed` |
| `thread_permissions/native_leaf.rs` | `f6a64e4cb07619459fcdb21c8bb152b20b7ff7374a8814a73e3b8cb71a18f814` |

The existing `target/nsh-permission-notice-check.stderr` ends with the application dev-profile check succeeding in 7.87s. That is parent-provided **pre-followup** evidence, not a check of these changes and not native acceptance. This stage ran no Cargo, Git, network, application launch, delegation, configuration/dependency/vendor mutation, or independent review. No nested AGENTS were found under crates/docs/scripts.

Remaining parent gates: current-version application/test compilation and execution of the three regressions; delayed stale/ABA/unmount actions; all five failures and busy/idle transitions; actual per-leaf AX Focus/Click/bounds; keyboard Tab/Shift-Tab/Enter/Space, IME/modifiers/repeat and focus reveal; geometry/scroll/partial clipping; injected renderer failure and presentation-only recovery with no permission I/O; narrow windows, themes/locales and macOS/Windows/Linux acceptance; centralized independent review, development-test cleanup and final required CI.

**READY — author stops writing after final hash verification.** This handoff claims implementation plus scoped formatting only, not compilation, executed regressions, native acceptance or independent review.

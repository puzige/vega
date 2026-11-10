# NSH automation and permission feedback

## Acceptance and scope

This lane implements the production automation strips, permission trigger and permission feedback in `conversation_stream/automation.rs`, `automation_permission.rs`, `automation_permission/render.rs` and new native children of `automation_permission`. Shared module/render/core wiring belongs to the parent. Thread permissions and the existing automation editor are not modified.

Acceptance: every visible glyph, label, error, button, panel and permission detail is drawn by NSH 0.4.0. GPUI retains only layout, Surface hosting, trusted controller dispatch, focus and native accessibility proxies. Preserve the complete AutomationProjection, task identity/revision, elapsed/running and wake-up state, all existing control/editor events, all permission phases, complete instruction/schedule/budget/risk details and the existing authorization-bearing confirmation object. No permission capability is added.

Pointer, keyboard and AX activation must use the same current-source, root/navigation epoch, mounted lease, finite/full-clip geometry, measured engine row and projection fence. Delayed actions after replacement, navigation, unmount, operation change or permission change must fail closed. Layout observations are engine geometry acknowledgements, not proof of compositor pixels or native accessibility success. Enter/Space respect IME and modifiers; Tab/Shift-Tab and Escape preserve the original controller behavior. Detail scrolling must remain real NSH scrolling with keyboard access.

## Implementation plan

1. Preserve business control methods and replace visible GPUI controls with native NSH leaves.
2. Reuse the existing mounted Surface pattern with independent source bindings for automation tasks and permission operations. Add ordered layout-observation revision checks rather than copying stale-measurement acceptance.
3. Keep timing publication live without drawing the old GPUI clock. Preserve complete permission details with a bounded native scroll surface.
4. Run only scoped rustfmt and read-only static checks. Parent owns Cargo, integration, centralized independent review and final CI; no intermediate independent review is requested.

## Validation status

Implementation written; final scoped static checks passed. No Cargo, native application, platform/AT, CI, Git or network execution is authorized or was performed in this lane. No behavior or typecheck success is claimed by static checks.

## Implementation delivered

- Automation status/summary, reason/error, elapsed or wake-up label and controls now instantiate NSH leaves. Clear and pause/resume use embedded NSH SVGs with full native accessible names. Summary remains the keyboard-accessible editor entry; the existing automation editor implementation is untouched. The complete AutomationProjection remains the business source and is captured in action bindings, not replaced by a reduced task model.
- The GPUI AutomationClock and its map were removed. `apply_automation_timings` stores the real timing map and notifies when it changes, including elapsed-only changes; NSH leaves render `format_elapsed` from the actual map with task elapsed fallback. This changes notification granularity, not the timing source. Running and wake-up conditions remain real controller state.
- Preparing/cancel, Confirming/cancel/confirm/details, Saving, Reloading and Failed/reload are preserved. Full-access risk is a separate visible warning-colored NSH leaf above the scrollable details. Persistent effects, scope, interval/goal, optional budget and complete instruction remain in a real bounded NSH Scroll. Keyboard supports arrows, PageUp/PageDown, Home/End, Space/Shift-Space. Range comes from NSH measurement/scroll events and offset commands use ElementRef; no wheel remainder or backend changes.
- Each action checks weak owner, ReadingOwner root authority/navigation epoch, full Thread and AutomationProjection equality, task identity/revision/full task equality, operation/phase, live focus handle, enabled predicate and current business gate snapshot. Host checks active visible window/environment, Surface errors, mounted lease, finite measured bounds and full clip, viewport/scale and current projection id. Engine row leases carry an atomic layout observation; stale asynchronous measurements and queued pointer/AX actions are rejected. NSH pointer activation remeasures before ordered Displayed/Activate dispatch. AX callbacks capture the observation too. Unmount stops only the leaf Surface, not background automation.
- Permission request additionally rejects a task not in the current projection or belonging to another thread. The original request Thread is retained through confirmation and checked before confirmation emission and save. The original AutomationPermissionChange is emitted unchanged; no authorization is manufactured and service validation remains authoritative.
- GPUI divs in the public render methods are layout-only. Leaf wrappers supply event/focus and transparent native AX proxies, not visible text/icons/backgrounds. NSH draws the actual pixels and focus/hover/pressed/disabled states. Surface failure offers a native reload-view leaf, distinct from permission reload. No old visible-control fallback exists.

## Parent integration: exact points

Existing shared wiring already calls all three methods; no shared edit is needed for this lane if those calls remain:

1. `conversation_stream/render.rs`, `render_composer`: while `self.automation_permission.busy()`, return `self.render_automation_permission(window, cx)` (observed line 235).
2. `render_composer_column`: retain `.child(self.render_automation(cx))` (observed line 296).
3. Main render focus preparation: retain `self.prepare_automation_permission_focus(window, cx)` (observed line 585). Focus handles now target native-leaf hosts, including task authorize focus restoration.
4. Existing `mod automation` / `mod automation_permission` and core `AutomationUi`/`AutomationPermissionUi::new` remain unchanged. `automation_permission.rs` declares its own `native` child, so no shared module declaration, core field, initializer or new dependency is required.
5. Parent should include all five Rust files plus this document in integrated check/Clippy and final centralized independent review. `thread_permissions.rs`, shared mod/render/core, automation editor, dependency pins and vendor were not written by this lane.

## Actual verification and limits

Supplied source HEAD is `0b5ea426abfd8d4358489e916273d89474fb190f` plus preserved dirty implementations. Parent supplied the fetched origin/master ancestry confirmation; this lane did not run Git or repeat it. No commit or merge was made.

Executed on final Rust bytes:

- `rustfmt --version`: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`.
- `rustfmt --edition 2024 --config skip_children=true` on exactly the five Rust files below; then the same invocation with `--check`: exit 0.
- Read-only Python static audit over those five files: zero comment syntax, production unwrap/expect, try_send, test markers or trailing whitespace; exit 0. Additional static scan of automation.rs and permission/render.rs found no former visible GPUI text/icon/background/button render patterns. These are structural checks, not type or behavior tests.
- No Rust tests added. Cargo/rustc/Clippy, behavioral tests, native app/AT, CI and three-platform validation are unrun, parent-owned. Cargo/rustc versions were not queried under the lane restriction. No timing/performance claim is made.

Remaining verification: compilation and strict linting; narrow-window wrapping and minimum action geometry; first-frame/post-scroll layout convergence; repeated clock updates and loop waiting/running changes; task replacement/revision and permission-source changes while actions are queued; all permission phases and outcomes; Tab/Shift-Tab traversal and task-trigger focus return; keyboard detail scroll and its initial/max range; pointer/detail focus handoff; native AX bounds/Focus/Click on macOS, Windows and Linux; theme/locale, actual IME and Surface reload. Full clipping intentionally disables activation and removes actionable AX until a current measured leaf is fully inside the clip; offscreen focus uses host autoscroll. Engine geometry acknowledgement is not compositor evidence. No independent review occurred during implementation, as requested.

## Frozen Rust artifact SHA-256

Paths below are relative to `crates/vega_ui/src/conversation_stream/`.

| File | SHA-256 |
| --- | --- |
| `automation.rs` | `9d2c40ec043d205bf0c00b2a5b16d520b9f92a6a3a738b87735d641dd5f32cb4` |
| `automation_permission.rs` | `135836282971bd054a9943e5f11a048efabced837cb457b31b32747f4beffa49` |
| `automation_permission/render.rs` | `fabf8eadb0b8598718ed5a2db62eaee86226b472756ccd80f9f3652be39d8620` |
| `automation_permission/native.rs` | `3a291ca5bd565286037438090b5ddf4b79c996dfa626d38e2f24e2d3d88b718b` |
| `automation_permission/native/leaf.rs` | `b8b237f6469e0d742a3b50400a8ece839e0393e1eb385d64f36dce01a36600a2` |

The document's own final hash is provided in the parent handoff, avoiding a self-referential checksum. This lane stops writing after the final artifact read/hash verification; the parent continues centralized integration and review.

# #907 CUA preparation-error leaf

## Scope and acceptance frozen before implementation

Base supplied by parent: `ee3947ac0f17b503e6efb635e0c383cea99576af`, dirty worktree with other leaves frozen. This writer owns only minimal preparation-error integration in `crates/vega/src/window/cua_onboarding.rs`, new `crates/vega/src/window/cua_onboarding/**`, and this document. No parent render/mod/shared/startup/conversation/loading changes, Git/index operations, dependency changes, network, builds, app execution, or delegation. Parent coordinates validation.

1. Replace all visible preparation-error pixels with a real NSH Surface. Preserve the exact existing `app_localization::translated` fallback and original retry/dismiss labels and accessible labels. Error text wraps; controls remain reachable at compact widths. No unrelated guidance-card redesign and no explanatory product copy.
2. Retry and dismiss carry the exact observed failure identity, original error and origin. Repeated identical failures have different identities. Compare Settings versus task and task stream/thread/project/TaskContext exactly; retain `Origin::is_current` and the original `open_preparation` and return business flows. Never take the current failed value before validation.
3. Bind every action to the currently mounted owner, current failure, current route/project/thread/stream/opening state, route epoch, geometry, appearance/language, and healthy Surface. Task failures act only on the matching visible task outside Settings; Settings failures act only while Settings is open. Hidden, replaced, inactive, stale, clipped, superseded, or unhealthy presentations reject actions without changing business state. Geometry is acknowledged from actual NSH layout against host clip/viewport/scale; acknowledgment is not claimed to prove compositor pixels.
4. Use the fixed public GPUI Fast/NSH APIs. Keep `render_preparation_error(&self, cx) -> Option<AnyElement>` unchanged; a native RenderOnce obtains Window. A paint-free tracked native focus owner provides Tab/Shift-Tab traversal, nonrepeating Enter/Space activation, and a named Button active-descendant AX proxy. NSH alone draws text, controls, backgrounds and focus. Do not install keymaps, intercept IME globally, fork/upgrade dependencies, or substitute old visible GPUI content.
5. NSH pointer, native keyboard and AX activation converge on one trusted dispatcher. Validate observed enabled control, displayed presentation/selection, mount and geometry before forwarding retry or dismiss. Mount/repaint/focus/recovery must not implicitly retry or call preparation services.
6. Surface failure uses a separate NSH recovery presentation with the original error and essential explicit UI reload action; reload changes presentation only, never replays business. Failures of the recovery Surface fail closed. Queued old commands cannot affect the new Surface.

## Parent integration boundary

The leaf exposes `VegaWindow::hide_preparation_error(cx)` to revoke presentation authority synchronously by dropping the current owner slot. The trusted dispatcher also checks the slot's exact host EntityId, so old retained elements and queued commands lose authority immediately without re-entering an updating host. `sync_preparation` invokes the hook when the failure is absent or its route is not visible. `OnboardingHost::close` also revokes it without changing guidance-window business behavior.

Read-only inspection of `window/render.rs::Render for VegaWindow` found calls to `sync_preparation` near entry and immediately before the error slot, with no current early return bypassing the slot. No specific additional parent edit was found necessary on that inspected render path. Parent must still call this hook synchronously before any hide/rebind/early-return path that omits both synchronization and the error slot, including a parent-only transient stream/route A-B-A replacement not observable through Settings/OpenThread/SelectedProject globals. Global observers invalidate an epoch for those published route changes; same-transition private parent changes require the hook, not an eventual frame lease. Parent owns lifecycle completeness validation; this writer did not modify parent files or weaken hidden-callback acceptance.

## Verification planned

Parent: scoped compile/format/strict Clippy, necessary behavioral regressions and independent review on stopped hashes. Native handtest: English/Chinese, light/dark, long wrapped errors, narrow layout, real focus and Tab/Shift-Tab/Enter/Space/AX, pointer-to-keyboard handoff, route/Settings A-B-A, exact repeated failure replacement, clipped/resize/scale changes, Surface failure and recovery, hide/reopen while commands are queued. No source-string or fabricated semantic tests. No validation execution is authorized for this writer beyond read-only static inspection and scoped formatting/whitespace checks.

## Implementation and stopped evidence

- `cua_onboarding.rs`: failure identity wrapper at the existing three assignment sites; original origin/current and open/return flows retained; render signature unchanged; scoped synchronization/hide and guarded retry replace the old unguarded `failed.take()`.
- `preparation_error.rs`: local native RenderOnce host, exact failure/current-route/host identity guards, invalidating epoch, mount lease, host clip/viewport/scale checks, NSH layout acknowledgment, one dispatcher, first-consumption fence, healthy-Surface checks, independent NSH recovery Surface and explicit presentation-only Reload. Failure origins and WeakEntity business handles remain on the trusted host, not in NSH watch snapshots.
- `preparation_error/view.rs`: NSH-only translated error, Retry/Dismiss or Reload/Dismiss, existing localized names and theme tokens, bounded scrollable error and separate action row, natural-height acknowledgment. Pinned `on_layout` is parent-relative; control coordinates compose root + action-row + control offsets before geometry validation.
- `preparation_error/mounted.rs`: paint-free element delegates layout/prepaint/paint and retains a frame-scoped mount lease. Two real native FocusHandles provide forward/reverse traversal and each has a paint-free named Button active-descendant AX proxy. NSH alone paints their focus indication. No keymaps or IME bindings added.

Executed successfully (exit 0): `rustfmt --version` = `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`; scoped `rustfmt --edition 2024 --config skip_children=true` and final `--check` on the four owned Rust files. Read-only Python static check: all four files have final newline/no trailing whitespace; the three new Rust files contain no unwrap/expect, comment openers, or Rust test wiring. This is a policy/format check, not a behavior test. No tests were added solely to inflate coverage.

Stopped Rust SHA-256:

| File | SHA-256 |
| --- | --- |
| `crates/vega/src/window/cua_onboarding.rs` | `547325f23a5399b861076b5cff87e6b29a21727a1a8b9b7befc7f2bc4d058ff4` |
| `crates/vega/src/window/cua_onboarding/preparation_error.rs` | `d3795c2675647e6af5dc9a933aef8ac7550169aeb1e21ce728831cfce8bcf625` |
| `crates/vega/src/window/cua_onboarding/preparation_error/mounted.rs` | `03115b4281af6920b1193fe24140d2d7042e0c1a572e31004f176e8e0cc55086` |
| `crates/vega/src/window/cua_onboarding/preparation_error/view.rs` | `7a5ca23afe415bd2b48b7a8ae4418b4d5bf3e9642d59668c75d9ac1910dd68b3` |

Unrun: compile/check, Clippy, Rust/Python behavior tests, CI, app launch, native focus/AX/paint/scroll/layout handtests and three-platform execution. Cargo/rustc versions were not queried because no compilation was run. Base HEAD is parent-supplied, not independently verified with Git. No Git commands, network, dependency/index changes, app operations, nested delegation, or edits to other leaves occurred. Parent must compile and independently review this version; API source inspection and rustfmt parsing do not prove type correctness or native behavior. Recovery-Surface failure intentionally fails closed; no third GPUI-rendered fallback or automatic business replay is supplied.


## Parent integrated validation (current development batch)

Baseline `ee3947ac0f17b503e6efb635e0c383cea99576af` plus this stopped delta. Format, workspace all-target Clippy `-D warnings`, and Nextest run `4eb222ed-ad9e-4ff9-8cd6-1ccea4c84ec8` passed: **175 discovered /175 executed /175 passed /0 skipped** across16binaries; runtime0.294s. Existing Python release/preparation/general groups passed5/15/40. Actual tools: rustc1.98.0, Cargo1.101.0 (huacnlee0.102.0-9), Nextest0.9.151; CI versions/dependency pins unchanged. This is headless development regression evidence, not native focus/AX/IME/pixels or Windows/Linux acceptance. Final exact-head CI is pending. Rust tests and exclusive wiring must be preserved as recoverable development evidence, then removed before master.

Integration corrections: fixed-pin NSH uses a real `scroll()` for bounded long error content rather than nonexistent View overflow helpers. Independent review found recovery selection could become Reload while native focus remained Dismiss; render now reconciles selection from the actual native focus handles and invalidates before creating/publishing the snapshot. The reviewer statically closed that finding; native focus-Dismiss → primary Surface failure → Enter and traversal remains unrun. Current preparation_error.rs/view.rs SHA256: `68b331e7577da22a3622a9260abdee185eec50fbb80adc9e06433c4c151e6a23` / `9a3eeb83fb19bbc670cfc51c56a9fcc53b4f28fae6c982a73e67ff15c160cc73`. The document was relocated from the accidental repository-root name to this canonical docs path; historical writer hash tables remain initial-stage evidence, not current assertions. No direct native recovery test was added or claimed.

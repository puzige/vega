# Settings tail mounted development validation

## Scope and version

Implementation/development validation lane for #907, not independent review. Starting dirty HEAD: `0b5ea426abfd8d4358489e916273d89474fb190f`, branch supplied as `feat/907-nsh-full`. Parent owns Git integration, compilation, test execution and consolidated review. Parent originally reported remote fetch/update complete and newly available master `f3688f5ae0b7becf1f46434c7b7d732e3a3c759a` (#916). For this runtime followup the parent reports #916/core/render integrated into the dirty worktree without Settings overlap; this lane performs no Git integration or independent HEAD verification.

Acceptance follows [the Settings tail contract](nsh-settings-tail.md): actual production passive widgets, natural wrapped height, current measurement authority, input pass-through and non-participation in focus. No business/service/controller authority changes and no dependency upgrade. No nested AGENTS.md was found under `crates` or `docs`.

## Authored delta

- `crates/vega_ui/src/settings/nsh_tail.rs`: extract the existing measurement condition and update into private `Host::apply_measurement`; the actual asynchronous receiver delegates to it. Same projection/width/finite/positive/change gates and notification behavior. Add one development-feature-gated child module.
- `crates/vega_ui/src/settings/mod.rs`: feature-gated Settings-local exports for three cases; no shared export change.
- `crates/vega_ui/src/settings/nsh_tail/mounted_validation.rs`: new mounted fixture and three cases.
- `crates/vega_ui/tests/nsh_settings_integration.rs`: new main-thread, real-platform-text-system runner, fail-fast with honest remaining-case reporting.
- This document.

No writes to helpers.rs, render_impl.rs, Cargo manifests, shared exports, other implementation lanes or vendor sources. Existing dirty changes remain owned by their writers. The second runtime followup below changes only the production label adapter in `nsh_tail.rs` and this document.

## Cases and evidence boundary

1. `narrow_fields_wrap_labels_and_preserve_natural_height`: mount actual `settings_group`, `section_title`, two `settings_field` widgets, both production backgrounds and existing `TextInput::render_nsh_input` controls. Observe actual Host heights and GPUI prepaint row/control bounds using a delegating Element wrapper. Assert multi-line label height, side-by-side controls at 700px, growth and stacked controls at 180px, no overlap/clipped control width, and restored natural height on widening.
2. `changed_content_font_theme_and_width_reject_retired_measurements`: on the mounted description, change long-to-short content, inherited font weight, light-to-dark theme and width. Require current naturally measured layout, fresh projection and replacement Surface. Inject a delayed old-projection height into the same private production receiver decision, require no height mutation, then reject current-projection wrong-width and invalid-height reports. This is deterministic measurement-delivery fault injection after real mounting, not a claim that a delayed engine report was naturally scheduled. The acceptance predicate is not duplicated.
3. `passive_pixels_forward_parent_input_once_and_skip_cross_surface_focus`: dispatch GPUI platform mouse/wheel events over an actual field label, group padding and overlapping page/content backgrounds. Require one parent click, one parent wheel callback and exactly one native parent scroll displacement per target. Preserve focused native control; use GPUI forward/reverse focus traversal across the two real native editor Surfaces, excluding their GPUI proxy handles and every passive Surface. Require unchanged input values.

The fixture observes production Host creation through `App::observe_new`, retaining weak handles only. It does not reconstruct NSH layout, substitute a renderer or inspect source strings. The native editors are existing production controls in a helper-level fixture, not the actual Defaults permission/model pickers or full SettingsView/controller. Synthetic text is fixture content, not new product copy. Native editors and passive Surfaces are stopped and the fixture window removed at teardown.

Budgets remain **2 seconds per drain, 512 ticks maximum, 10 seconds per stage**, with 10ms wait steps. No retry/ignore, lint suppression, ordinary code comments or dependency changes were added. The stage timeout cannot preempt a synchronous backend draw/tick; the original drain deadline continues to assert on its return.

## Parent registration and execution

Parent reports registering this exact development target in `crates/vega_ui/Cargo.toml` (parent-owned file; not changed by this lane):

```toml
[[test]]
name = "nsh_settings_integration"
harness = false
required-features = ["nsh-reading-integration"]
```

Reuse the existing `nsh-reading-integration` feature and existing dev dependencies. No new feature or shared re-export is required. Without `harness = false`, Cargo's automatically discovered target will not execute this main as the intended three-case runner.

From the task worktree, using the parent's selected toolchain/cache/concurrency:

```sh
cargo test -p vega_ui --features nsh-reading-integration --test nsh_settings_integration --offline --locked -j2 --target-dir target/official-nsh-validation
```

Expected runner discovery is three explicitly enumerated mounted cases, not libtest `#[test]` discovery. Require all three PASS lines and the final `Settings integration: 3 passed; 0 failed; 0 ignored` footer. The runner intends to catch a case panic and exit 1 with remaining cases unexecuted; the original destructor double panic instead aborted before that report. Parent must preserve the first failure and route concrete compiler/runtime diagnostics to this lane rather than increase budgets or weaken assertions. Compile/Clippy and existing native/reading/scroll regressions remain centrally coordinated.

## Runtime followup: failure evidence and scoped correction

Inspected actual parent logs `target/nsh-settings-mounted-cycle.stdout` (empty) and `.stderr`: first assertion at original fixture line 389, `control must wrap below natural label height`; while unwinding, `VisualTestContext::update` unwrapped `window not found` at pinned GPUI Fast `app/test_context.rs:871`, causing destructor panic/SIGABRT. Parent reports exit 101. **No PASS: first case started and failed/interrupted; remaining two cases were not executed. This was not a timeout.** No actual row/control bounds or label heights were logged, so the original run does not establish the final settled production geometry.

Production relation checked: actual `render_impl.rs::render_defaults` invokes `settings_group`, `section_title`, and `settings_field`; the fixture invokes these same functions with existing `TextInput::render_nsh_input` controls instead of business permission/model pickers. `helpers.rs::settings_field` retains native flex-wrap with 208px/max-full label column, 16px horizontal gap and 220px/max-full control flex basis. `nsh_tail.rs` receives height asynchronously, changes `Host.height`, and notifies GPUI. No helper/production layout or business changes were made for this followup.

A concrete observation-layer gap exists: the old `frame()` drew bounds, then drained measurement delivery; `ready()` tested the newly ready Host state without requiring a subsequent draw using those heights. It could therefore compare current natural height with older prepaint control bounds. This is a confirmed ordering gap, **not yet runtime proof that it caused the original geometry failure**.

Only the mounted fixture and this document changed in the followup:

- Snapshot actual measured Hosts (identity, projection, width, height, readiness) before draw and after its final drain. Readiness now additionally requires all these values unchanged across that boundary, with both snapshots ready; this ties measured state to the drawn bounds without duplicating geometry or waiting for the disputed assertion to become true.
- Keep every original wrap/natural-height/control-width/next-row assertion unchanged. Keep 2s/512-tick/10s budgets unchanged.
- Bounded `[settings-layout]` diagnostics: first three readiness observations plus the settled observation for each readiness call (at most four per call). Report actual group/row/control prepaint bounds, requested group width, actual label width/height/readiness, drawn-state currency, and expected stable layout. There is no per-tick log flood or mirrored layout algorithm.
- Fallible teardown through `visual.cx.update_window(visual.window_handle(), ...)`, matching existing scroll-composition/group fixtures. Stop Surfaces, suspend native editors and remove the window when available. Cleanup errors panic normally; during an existing unwind they are logged rather than unwrapped into a second panic. No normal cleanup error is treated as success.
- Remove only the unused `AppContext as _` import; no lint suppression. Runner remains unchanged.

**Next verification:** parent reruns the exact command above, retaining fresh stdout/stderr separately from the original cycle. If the original assertion still fails with `drawn_measurements_current=true`, return the actual narrow group/row/control bounds and label widths/heights from the new diagnostics; this would distinguish a stable production layout defect from the repaired observation ordering without weakening acceptance. A compile error or cleanup error must be routed as its own concrete failure. No runtime outcome is claimed for this correction.

## Runtime followup: current measurements with stale parent geometry

Read fresh `target/nsh-settings-measurement-current.stderr` after the parent's original three-case command. First case fails at fixture line 470, the unchanged `control must wrap below natural label height` assertion. Runner reports first case failed, remaining two not executed, 0 ignored; process exits 1 rather than SIGABRT. Parent reports 0 PASS. Cleanup logs `settings fixture cleanup during unwind: window not found` without a second panic. This is not a timeout.

Decisive actual geometry at `ready: current measurements drawn`, `drawn_measurements_current=true`: requested group width 180px; group 180×176px; first row origin (17,17), size 146×73px; second row origin (17,90), size 146×69px; first control origin (17,53), size 146×20px; second control origin (17,122). Ready label measurements at width 146px: title 22px, long field label 61px, long description 149px, second field label 21px. The parent layout cannot contain the actual measured text. The previous readiness repair did not resolve this production failure.

**Exact cause:** GPUI Fast 0.1.9 `elements/container_query.rs::request_layout` requests a styled layout node with no child layout IDs. Its callback creates the child only in `prepaint`, then calls `layout_as_root` under already assigned bounds. The contents explicitly cannot influence the query's own size. Settings used this query to capture inherited text styling; therefore `Host::apply_measurement` correctly stored height and `cx.notify()` invalidated the Host, but its height remained outside the parent row's layout dependency tree. Retained parent layout could continue using the query's content-independent geometry even after a fully current draw. Waiting or notifying more cannot connect a detached child layout.

**Small production fix:** replace only that `container_query` with a private Settings-local `Label` Element. During `request_layout`, under the enclosing Div's inherited text style and field-ID namespace, it constructs the same keyed Host with the same font/heading-weight projection and returns the Host child's actual layout ID. Store that child as request-layout state and delegate prepaint/paint normally. `Host.height` now participates directly in the real parent flex layout, so existing Host notifications can propagate natural height through the label column, wrapped row, next row and group. No manual height mirror, forced window refresh, native-control change, business-authority change, shared API or dependency change. Full-wrap NSH text, stale-report rejection, passive masking, non-Tab configuration and Surface lifecycle remain untouched.

Only `crates/vega_ui/src/settings/nsh_tail.rs` and this document changed in this followup. Fixture and runner hashes remain identical to the previous handoff: original geometry assertions, bounded diagnostics, readiness predicate, teardown and all 2s/512-tick/10s budgets are unchanged. The product fix has not been compiled or run by this lane.

**Next verification:** parent reruns the original three-case command above with fresh logs, using its centrally coordinated toolchain. Require all original cases and final three-pass footer; preserve the actual geometry diagnostics if any assertion still fails. In particular confirm the 180px layout now places the control below measured label/description content and grows the actual row/group, then restores natural height at 700px. The unchanged second and third cases still need first execution for stale content/font/theme/width reports, passive input and cross-Surface focus. Do not interpret static checks as a runtime pass.

## Actual static checks

- Read pinned GPUI Fast 0.1.9 APIs for new-entity observation, Element delegation, tab traversal, input events and ScrollHandle/default scroll listeners; read actual Surface and native editor implementations. No shared API extension was needed.
- `/Users/puzige/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/rustfmt --version`: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`.
- Final four-source command, exit 0 with no diff:

```sh
/Users/puzige/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/rustfmt --edition 2024 --config skip_children=true --check crates/vega_ui/src/settings/mod.rs crates/vega_ui/src/settings/nsh_tail.rs crates/vega_ui/src/settings/nsh_tail/mounted_validation.rs crates/vega_ui/tests/nsh_settings_integration.rs
```

The followup four-source `rustfmt --check` also exited 0 with no diff; Python confirmed fixture hygiene and recomputed source hashes. Its first check found readiness-predicate indentation only, corrected through a scoped edit. Earlier checks found formatting-only diffs, corrected through scoped file edits. One attempted read-only `rustfmt --check --emit stdout` was rejected as incompatible options; it changed no file. Python static checks confirmed final newlines and no trailing whitespace; new Rust files contain no ordinary comment, lint suppression or ignore marker. These are static checks, not compiled or executed regressions.

| Source path under crates/vega_ui | SHA-256 at lane stop |
|---|---|
| `src/settings/mod.rs` | `e9e26ff27521e186b6082b6d3b2666372c3e9294347ea149a1128f69cfafee20` |
| `src/settings/nsh_tail.rs` | `41d503ad62c7a00beaa6f1f3d76e8a4fce0171eefcbc1e244298bbb738fd5b41` |
| `src/settings/nsh_tail/mounted_validation.rs` | `7124adf338ab4104add23d9ed01455b683947d290561a2fb6d8c0bb9b5f5fad3` |
| `tests/nsh_settings_integration.rs` | `d98939e7cd10ce74ec56c35676fd48465937c8e8871ed54dd6360c9d49bf8670` |

The second runtime followup's final four-source `rustfmt --check` exited 0 with no diff. Python confirmed final newlines, no trailing whitespace, no ordinary comments or lint suppressions in the production adapter, and exact unchanged fixture/runner hashes. Source was reread after editing. No Cargo/rustc/test command was executed by this lane.

## Limitations and next gate

**Production layout fix: all three cases remain UNRUN at the final bytes.** Both parent runs failed the first case and did not execute the remaining two; the fresh run established a production layout defect and confirmed unwind cleanup no longer aborts, as recorded above. No Cargo, rustc, compilation, application, test, network or Git mutation was executed by this writer. No runtime pass, independent review, CI, merge or native acceptance is claimed. Parent central recompilation/execution of this correction is pending; no unavailable shared capability was worked around.

The cases do not validate actual external service success, full Settings business flows, permission/model popover focus restoration, OS-level keyboard routing, real native accessibility/AT acceptance, precise pixels, platform parity or locale/font-family matrix. Those remain pending, alongside actual full-page narrow-window behavior and original Settings-tail manual acceptance. GPUI test-context input/focus is not OS-level synthetic-event acceptance. Required final-head CI and non-implementer consolidated review remain independent gates after all implementation lanes finish.

These Rust tests, their feature-only module/exports and parent target registration are development-only and must be preserved as recoverable validation evidence, then removed before master delivery under repository policy. Keep the production measurement acceptance behavior intact during cleanup. READY means this lane has stopped writing for parent compile/run, not that its regressions passed.

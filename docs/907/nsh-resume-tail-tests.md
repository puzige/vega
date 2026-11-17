# Resume-tail development regressions

## Scope and acceptance plan (before test implementation)

Baseline supplied by parent: `0b5ea426abfd8d4358489e916273d89474fb190f` plus concurrent integrated dirty worktree. This writer may create only `crates/vega_ui/src/conversation_stream/resume_tail/tests.rs` and this document. Existing files, production wiring, dependencies, configuration, Git, network, builds, application execution and delegation are out of scope. Parent owns wiring, compilation, test execution, evidence archival and eventual removal of development Rust tests and exclusive wiring before merge, per AGENTS and the development-test lifecycle.

Read current root AGENTS, execution guide, design guidelines, full migration spec, leaf spec/parent correction, production leaf and pixels renderer, and the working conversation-status Core harness. There are no nested AGENTS in crates/docs. Current implementation preserves read-only preview and permission-revoked states as frozen identities, and navigation has its own `reading_navigation_epoch`; the older leaf spec's run-notice-epoch sentence is superseded by current production and the user's correction. These host authorization boundaries must not be simulated by a renderer test.

### Intended observable regressions

1. Mount the actual `pixels::render` using pinned `#[ns::component] fn Fixture() -> impl IntoElement`, NSH `Core<ApproxTextEngine>`, and real layout event delivery. Check the mounted circular control, root clipping, centered layout, 1px border, token dimensions, theme colors and actual parsed ArrowDown drawing geometry rather than source-code strings or a copied renderer.
2. Check that the actual NSH outer ancestor is inaccessible and hides its descendants, with no duplicate NSH Button AX authority. Inspect the mounted tree, not merely whether an AX flag appears anywhere in a batch.
3. Deliver real layout callbacks and inspect the resulting `Request::Layout` values from production `OrderedCommands`. Check original projection identity, localized label preservation, and measured root/control rectangles. Resize the Core and reconcile incremental mounts; synthetic `ROOT_ID` is not a created view.
4. Project focus/hover/pressed/theme/language changes through a runtime-owned channel, reading/writing signals only inside Core tasks. Observe actual style changes and press the current production listener. Check that `Request::Activate` retains its displayed projection, including localized label, and that disposed old listeners cannot act on the replacement.
5. Exercise a pending real Press across dynamic-owner disposal with bounded command-channel backpressure. The queued command must survive disposal of its dynamic content owner because production roots sends in the original renderer Owner; the queued old observation must remain old, never refresh to the replacement identity. This tests renderer command lifetime, not host acceptance of stale actions.

Numeric acceptance uses existing tokens: navigation height 40, vertical padding 4, control 32, icon 16. At width 320 the centered control x is `(320 - 32) / 2 = 144`, y is 4; at width 160 it is 64, y remains 4. The icon is centered within the 32px control, at local x/y 8 (including its 1px border and remaining 7px alignment space). Radius is 16 and the border is 1. Arrow authored viewBox is 24 square: shaft `(12,5)` to `(12,19)`; left branch `(5,12)` to tip `(12,19)` to right branch `(19,12)`. Tests must inspect the actual parsed drawing, its transform and these path points.

## Explicitly not covered

No fabricated GPUI Window/TestAppContext or new test dependencies. Native Tab/Shift-Tab/Enter/Space, actual OS AX proxy, tooltip, pointer routing/hit testing, inactive measurement versus activation, trusted root/current-stream/modal dispatch, preview/permission acceptance, independent reading epoch, consume-before-business, clipping/scale/window guards, `Geometry::visible`, and original `resume_tail` scrolling/anchoring remain untested by these renderer regressions. A Core Press callback is not an end-to-end native interaction test. ApproxTextEngine is a real NSH headless layout engine, not native rasterization or screenshot validation.

## Parent wiring required

Parent must add `#[cfg(test)] mod tests;` to existing `conversation_stream/resume_tail.rs` (only the parent may edit that file). No production API or dependency change is requested. Then parent runs its unified build/strict Clippy and targeted development tests, records exact source/toolchain/commands/results, archives the test source/patch and hashes, and removes tests and exclusive wiring before merge. This writer does not inherit the parent's earlier 5.78s all-targets strict-Clippy result as validation of the new tests.

## Execution record

Plan recorded before creating the Rust test file. Four development tests are now authored, all **uncompiled and unrun**:

| Exact test name | Observable assertions |
| --- | --- |
| `production_pixels_draw_token_sized_arrow_under_hidden_ax_ancestor_and_report_layout` | Actual production renderer across light/dark and supplied English/Chinese projections; mounted hidden AX ancestor and clipping; 32px circle, 1px border, radius/colors; actual parsed Canvas shaft/branches, round cap/join, stroke color/width and 24-to-16 fit transform; real layout requests and 320-to-160 resize. |
| `production_projection_updates_focus_hover_pressed_theme_and_localized_press_observation` | Actual projection channel updates focus/hover/pressed/theme/language; live control styles, real layout acknowledgement and production Press callback retain the current identity/label/state. |
| `disposed_dynamic_listener_cannot_emit_or_refresh_to_replacement_projection` | After Core applies replacement and disposes the dynamic owner, delivery to the old Press listener emits no activation; the current listener emits only the new observation. |
| `pending_ordered_press_survives_dynamic_owner_disposal_without_adopting_new_snapshot` | Capacity-one production command channel blocks a second real Press; replace/dispose dynamic content while blocked, then drain. Both old observations arrive in order before new layout, and the current listener remains usable. No copied command dispatcher. |

Harness observes incremental MountItems including AccessibilityLabel, and delivers both JsEvent forms for layout callbacks. Signal access is confined to fixture/runtime tasks. The first commit precedes retrieving fixture channels, matching the working status/run-notice harness. Sequential layout callbacks can transiently combine a new root and prior control: every observation must preserve identity and finite positive/token dimensions, while the final acknowledgement must exactly match settled geometry. No host guard is weakened or reproduced in the harness.

English/Chinese labels in the fixture are explicit projection inputs matching the existing catalogs; assertions prove production callback preservation, **not** invocation of Host's i18n lookup, native tooltip or native AX label. Focus/hover/pressed are supplied projection states, not native input simulation. The drawing assertion intentionally requires the pinned native SVG Canvas path; it does not silently fall back to source-string inspection or skip shape coverage.

Executed static checks:

- `rustfmt +1.98.0 --version`: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`.
- `rustfmt +1.98.0 --edition 2024 <absolute-new-tests.rs>` and explicit-file `--check`: exit 0; only the new test file was formatted.
- Scoped Python inspection of the two new files: final newline and no trailing whitespace; Rust file has no comments and exactly four test attributes. This is static hygiene, not test discovery/execution.
- Inspected pinned cached NSH 0.4.0 Core/mount/canvas/SVG and production OrderedCommands APIs. No network or dependency changes.

Not run: Cargo/rustc, compile, Clippy, test discovery/execution, CI, application/native acceptance. The user's existing parent 5.78s strict-Clippy result predates these tests and is not a pass for this artifact. Baseline HEAD is user-supplied; this writer ran no Git commands. No existing file was edited. Parent still must add the cfg(test) module wiring before discovery and run its unified checks; a compile/harness failure must be corrected in the responsible test layer rather than bypassed with ignores or weaker production acceptance.

Stopped Rust artifact SHA-256: `9389cd3670e92b71da1cdffdf0be06b2fef747b05be74b00c5ac41cce34a1b1b`. The fixture uses the pinned component attribute macro; no derive macros are used anywhere in the new Rust file.

READY at the author's return meant authored and statically formatted for parent integration, not compiled or accepted. The author stopped before parent changes below. Parent owns subsequent wiring/run/archive/removal.

## Parent execution and harness correction

Parent registered `#[cfg(test)] mod tests;` and executed the actual four tests. Run `278a999f-5f10-4294-b0c2-8ec2def3a7e9` executed four: three passed, resize failed, 180 filtered. The original test incorrectly called `Core::set_root_size(ROOT_ID, ...)`, an API for independently presented modal roots. Pinned Core documents main-root resizing as `set_viewport` (`nsh-core/src/core/mod.rs:1461–1470`); the actual Engine Viewport message calls it (`nsh-engine/src/lib.rs:2957`). Parent changed only the resize input API, keeping the expected x=64/control32×32 and exact final layout acknowledgement unchanged. No production change or assertion weakening.

After correction: `cargo nextest run -p vega_ui -E 'test(resume_tail::tests)' --locked --offline --build-jobs 2`, run `48facd0c-a6f0-4841-945b-11c15b614360`, **4 executed / 4 passed / 180 filtered**, execution0.036s; build4.81s. Raw logs `/private/tmp/vega-907-resume-tail-tests.log` and `/private/tmp/vega-907-resume-tail-tests-fixed.log`. Scoped rustfmt also passed. Full final-version suite, strict all-target Clippy and exact-head CI still need the completed batch. Native/root-dispatch limits above remain unchanged.

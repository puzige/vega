# Composed scroll gesture private-host regressions

## Scope and status

This author lane adds only `vendor/nsh-platform-gpui/src/input/scroll_host_tests.rs` and this record. It does not change production input, bridge, native scope, geometry, scroll, manifests, exports, or runners. NSH 0.4.0, Fast 0.1.9, and Kit 0.7.3 remain fixed. The parent owns module declaration, host-runner exports, compilation, execution, integration, and consolidated review.

The fixture mounts actual `Surface` / `NshView` trees through `MountItem`, shapes selectable text with `GpuiTextEngine` and a font-backed `Arc<TextSystem>`, captures real `ReadingGeometry`, invokes production composed input / bridge / route / scope controls, and observes actual core viewport offsets. The mixed-axis fixture uses the production fixed-native-target `consume_from` path because its pre-scrolled content need not remain under the pointer; ordinary pointer ownership cases use `compose_remainder`. No mock geometry or copied scroll algorithm is used. These are process-local host regressions, not native event-dispatch or outer List acceptance.

## Intended callable coverage

All six functions accept `Arc<gpui::TextSystem>` and must run on the existing main-thread host runner:

1. `composed_zero_motion_boundaries_revoke_geometry_leases`: actual Began, Ended, Cancelled, no-target event, and explicit retirement revoke old leases without requiring viewport motion; rejected zero anchor requests preserve a fresh lease. An active fresh lease cannot actuate an anchor, and retirement cannot resurrect it.
2. `composed_owner_and_mouse_latch_keep_original_mounted_path`: precise movement and mouse notches crossing a sibling retain the first target and its nearest-first ancestor path; an expired mouse latch selects the new target.
3. `composed_route_and_native_scope_retirement_revoke_authority`: route cancellation, detached owner observation, native scope drop, and stable/replacement parent scope identity exercise production ownership controls. Replacement scope construction/drop is the native route rebind boundary; this does not claim to execute `AppView::set_scroll_route` without a real AppView/window.
4. `composed_generation_visibility_and_drop_retire_route_owner`: real route ownership rejects generation changes and hidden Surfaces, and a route cannot retain a dropped Surface.
5. `composed_real_offsets_preserve_mixed_axis_remainder_and_units`: finite mixed-axis partial consumption traverses a mirrored horizontal scroll and two vertical ancestors through production bridge forwarding, checking real per-view offsets, total consumed/remainder, pixel and line units, and horizontal vertical-wheel fallback.
6. `composed_terminal_delta_and_system_momentum_do_not_add_glide`: terminal delta and later OS momentum each move the retained owner exactly once, cancellation clears ownership, and subsequent real core frame steps do not add an NSH fling.

Parent declaration: feature-gated `pub(crate) mod scroll_host_tests;` in `input.rs`; parent re-exports the six names under the existing host-regression namespace and adds six calls to the existing font-backed main-thread runner. This lane does not write any of those shared files or change existing assertions/time budgets.

## Geometry boundary implementation handoff

The inspected parent `composed_from` already calls `crate::reading::geometry_invalidated(surface)` immediately after finite event validation and before phase/owner processing. Retain it: zero Started/Ended/Cancelled and no-target events must not rely on an offset change to revoke old geometry. The real scroll wrappers also invalidate in `wheel_begin` and `wheel_end`.

The inspected `end_composition` removes `surface.input.composition` and then calls `wheel_end` only for `g.views`. Add `crate::reading::geometry_invalidated(surface)` inside `if let Some(g) = gesture`, before iterating `g.views`, so editor-only/empty-view ownership retirement cannot preserve an old lease. `retire_scroll_gesture` also clears legacy ownership, momentum and latch; it needs an ownership-boundary invalidation before clearing them when any of those authorities existed. An unconditional invalidation at retirement entry is a conservative alternative if the parent accepts invalidation on redundant retirement. In particular, do not use an idle `reading_anchor_blocked` result after teardown to infer that a previously active lease is current again.

Event entry and scroll wrapper invalidation are already present in inspected bytes; this is not a claim that the teardown addition is missing in the final concurrent parent version. Parent must inspect its final bytes and execute the regressions. The tests do not fabricate an editor or claim coverage of an editor-only composition without a mounted native input; the empty-view teardown requirement follows from the actual `ScrollGesture.views` path and belongs to the parent implementation.

## Verification and delivery boundary

Author lane is restricted to focused rustfmt and static checks; no Cargo/rustc, Git mutation, network, app launch, dependencies, delegation, or independent review. Runtime results are not acquired here. Parent must compile and run all six functions, existing required regressions, final-head CI, and native acceptance. Listener order / exactly-once outer `ListState::scroll_by`, native callback delivery, real AppView route rebind, and nested cross-Surface dispatch still require parent integration validation.

These are development-only Rust regressions. Archive exact sources, hashes, and actual run results before removing the tests and exclusive wiring prior to master according to the repository delivery policy. Do not weaken assertions or mark an unexecuted function passed.

## Author stopped handoff

Focused formatting and `--check` passed with `/Users/puzige/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/rustfmt`, `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`, edition 2024, `skip_children=true`, for the new Rust file only. Read-only Python static checks passed for UTF-8, final newline, trailing whitespace, conflict markers, exactly six callable functions, and no Rust comment syntax, lint suppression, or ignored tests. Public callable descriptions use `#[doc = ...]` attributes, following the existing anchor-host fixture pattern without comment syntax or lint allowances.

Rust source SHA-256: `6657c280e16d7fa6372f91b9ae601e418e40adcba6ebed8225e1864d5dfa2cb7` (20,827 bytes). This lane authored six / executed zero regressions. Cargo/rustc/Git/network/app/delegation were not run; original assertions, tests, budgets, dependencies, and shared files were not modified by this lane.

Final read of shared `input.rs` still showed `end_composition` and `retire_scroll_gesture` without direct geometry invalidation. The first regression includes a real mouse-latch-only retirement lease specifically to expose that remaining boundary, not a source-string assertion or a fabricated composition. Parent next step: add the production teardown invalidation in its shared lane, declare/export/call these six functions, then compile and execute them against its final integrated bytes. Scope identity replacement is covered through real `NativeScope` construction/matching/drop; actual AppView rebind and Window listener delivery remain separate parent acceptance work. Writes stop after final hashes and static checks.

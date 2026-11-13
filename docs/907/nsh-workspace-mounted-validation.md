# Workspace passive NSH mounted development regressions

## Status and scope

**READY — author implementation stopped. This is implementation plus development regressions, not independent review.**

Observed dirty task HEAD: `0b5ea426abfd8d4358489e916273d89474fb190f`. Parent owns remote/master #916 integration, Cargo/rustc execution, Git mutation, final review, cleanup and CI. This lane executed no Cargo/rustc, network or Git mutation. The earlier controller extraction used `workspace.rs` and its `nsh_workspace/` subtree; this harness-only follow-up writes only the four Rust files listed below and this document. No workspace.rs, mod.rs, controller, render.rs, Cargo, vendor, backend, provenance, dependencies or other lanes were written in this follow-up.

Design basis: [workspace separator section](nsh-full.md#工作区拖拽分隔线残余迁移), AGENTS, execution/issue workflow guides and design guidelines. No added product wording, focus/AX targets, dependencies, ordinary code comments or lint suppressions.

## Harness-only wiring correction acceptance

This follow-up is implementation, not independent review. Its write scope is only `nsh_workspace/{separator,mounted_regressions,mounted_entry,mounted_observations}.rs` and this document; all other dirty/untracked files remain parent-owned and preserved. No Cargo/rustc, network or Git mutation is authorized for this lane.

The ordinary app-bin test with `nsh-workspace-integration` must compile only the shared observation support, not standalone fixtures/cases. Preserve the existing `all(test, feature = "nsh-workspace-integration")` gate and five hook behaviors. Load the existing mounted cases only from `mounted_entry.rs`; retain all six case names, order, assertions and scheduler/native-paint timing. No lint suppression, dropped case, new feature or production behavior change. Widen observation visibility only to the nearest common parent needed by the sibling runner cases. Parent must rerun strict Clippy including the app-bin test and the complete six-case custom runner against stopped final bytes.

## Prior integrated execution reported by parent

Parent reports both real `workspace.rs` drag-region calls and temporary Cargo target/feature wired. First compile found E0502 from constructing a listener while passing `cx`; parent bound listeners first. Parent reports:

```sh
cargo test -p vega --test nsh_workspace_mounted --features nsh-workspace-integration --offline --locked -j2 --target-dir target/official-nsh-validation
```

Result: **3 passed, 0 failed, 0 not executed, 0 ignored**, build 50.62s; logs `target/nsh-workspace-mounted-cycle-2.{stdout,stderr}`. This is parent-reported earlier-cycle evidence, not execution by this lane or validation of the new controller extraction. Actual Cargo/rustc versions for that execution are not independently obtained here. Parent also reports production turn-placement wiring and test-only scroll import gating; those are outside this lane.

## Controller specification recorded before implementation

The following acceptance was added to this document before controller implementation; implementation does not change the original constraints:

- Extract actual raw-extent drag transitions, resize-triggered tab-reveal request and displayed-dimension decisions into a small production controller module loaded by real workspace and the existing custom development runner. No duplicated clamp algorithm, full application/services fixture or new dependency.
- Preserve raw extents and clamp only displayed dimensions: right minimum 270px, maximum `max(270, available - 300)`; bottom minimum 150px, maximum `max(150, window_height - 240)`. Preserve right default `available * 0.43`, bottom `Layout::BOTTOM_WORKSPACE_HEIGHT` (currently 272px) supplied by production, and maximized sizing.
- Preserve left-down orientation, left-held move storing only that orientation's extent and requesting both tab reveals, left-up release, move without left held cancelling without extent/reveal mutation, and inert moves after release. Active left move notifies; cancellation/release add no notifications.
- Preserve right visibility at available width >=610px and bottom at window height >=480px, requiring selection and not hidden. Fullscreen selects the first maximized, selected, nonhidden pane independently of these thresholds; fullscreen suppresses both dock separators and makes only that pane visible. Production and regressions share decisions, not mirrored predicates.
- Changed window size requests both tab reveals and invalidates NSH; repeated size does neither. Header reveal consumption remains unchanged; no focus/AX semantic change.
- Add three controller regression callables; preserve all original three mounted assertions and timing budgets byte-for-byte. Static checks do not replace the parent's fresh six-case execution. Native dragging, actual tab scrolling/focus, fullscreen compositor behavior and cross-platform acceptance remain pending.

## Actual shared production path

`nsh_workspace/controller.rs` is production code, not a test double:

- `Geometry::{begin_drag, move_pointer, end_drag}` now receive the real separator down and workspace root move/up callbacks. `move_pointer` accepts actual `MouseMoveEvent` and window size; its boolean controls the existing `cx.notify()`.
- `Geometry::dimensions` contains the original render-time formulas. Raw requested extents remain retained when rendering clamps them.
- `Geometry::window_resized` returns whether NSH invalidation is required and requests both reveals. `Geometry::for_route` preserves requested dimensions but resets drag/window-size tracking, as the prior Workspace reconstruction did.
- `Workspace::panes` projects current selection presence, hidden and maximized state into `Panes`. `dock_visible`, `fullscreen`, `separator_visible` and `visible` are used by actual right visibility, fullscreen lookup, both separator branches, subagent-pane visibility and NSH header visibility.
- The actual header reveal consumer remains unchanged in `nsh_workspace/mod.rs`: it selects the active tab when accessibility recovery does not own selection, then clears the request. The tests assert requests, not rendered tab scrolling or focus effects.

Parent previously wired `drag_region` at both production sites; listener binding before passing `cx` is preserved. Sidebar retains its distinct `Layout::SIDEBAR_RESIZE_HIT_AREA`; no sidebar-controller extraction. No further parent render.rs hook is required for these controller regressions. The existing root/main/sidebar backgrounds continue using their real passive Surface path.

## Exact development callables

The `harness = false` main-thread target loads real `separator.rs` and `controller.rs` directly by path. The namespace adapter does not duplicate implementations or construct full VegaWindow/services. `mounted_main.rs` uses real platform text system/fonts, asserts a nonempty font inventory, initializes GPUI Kit, and reports passed/failed/not-executed explicitly.

The runner calls these six cases, in order:

1. `mounted_regressions::five_pixel_region_and_background_deliver_pointer_and_wheel_once(&mut TestAppContext)`
   - Both orientations; actual outer region 5px and inner native line Host 1px.
   - Down/up/move/wheel at offsets 0.25, 2.5, 4.75, 5.25 and 40px. First three invoke drag callback once each; latter two do not. Each injected event reaches parent exactly once.
   - Retained distinct AppView identities and zero-area passive input masks.
2. `mounted_regressions::resize_theme_and_clip_retain_surface_and_native_paint(&mut TestAppContext)`
   - Both orientations; eight stable frames require zero repeat publications.
   - Changes host extent 100→160px, dark→light theme, inherited cross-axis clip to 2.75px.
   - Requires actual native scene quads/colors and resized bounds; unchanged AppView identities, exact changed-color publication counts and clip observations remain asserted.
3. `mounted_regressions::unmount_stops_and_releases_surface_before_rebuild(&mut TestAppContext)`
   - Unmounts actual retained separator/background; observes stop/release and weak-handle death, then same-key rebuild with new AppView identities/native painting and no extra retirement.
4. `controller_regressions::displayed_extents_preserve_clamps_defaults_and_raw_drag(&mut TestAppContext)`
   - Default 430×272 at available1000/window-height900, narrow 270×240 and tiny 270×150.
   - Drags beyond both minima/maxima through actual move method; observes clamped dimensions, enlarged-window recovery of original raw 1100×800, maximized override and restoration.
5. `controller_regressions::drag_release_cancel_and_window_resize_preserve_reveal(&mut TestAppContext)`
   - Inert pre-drag moves; orientation-specific 500×272 then 500×400 results; both reveal requests on accepted move; independent maximized axis outputs.
   - End-drag and cancellation with no button/right button prevent further held-left moves and preserve dimensions/reveal flags. Return values preserve notification decisions.
   - First size/change on either axis requests reveal; identical size preserves consumed flags. Route reconstruction preserves extents but resets active drag and size tracking.
6. `controller_regressions::visibility_thresholds_and_fullscreen_suppress_dock_separators(&mut TestAppContext)`
   - Just-below/exact 610 and 480 thresholds, selection absence and hidden panes.
   - Both maximized prefers right; hidden/unselected right falls back to bottom; fullscreen survives below dock thresholds and suppresses both separators. Restoration re-enables separators.

Original mounted deadlines are unchanged: 2 seconds/512 scheduler ticks per drain; 10 seconds per native-layout/painting stage. The new controller cases are synchronous method regressions, not additional mounted/native observations. No weakening of the three existing cases or their budgets.

## Parent execution and wiring

Existing temporary Cargo wiring is parent-owned and reported installed:

```toml
[features]
nsh-workspace-integration = ["gpui_kit/test-support"]

[[test]]
name = "nsh_workspace_mounted"
path = "src/window/nsh_workspace/mounted_main.rs"
harness = false
required-features = ["nsh-workspace-integration"]
```

No new target, feature, export or dependency is needed. Parent should execute against the stopped source:

```sh
export RUSTUP_TOOLCHAIN=1.98.0 CARGO_FORK_AUTO_UPDATE=0 TMPDIR=/tmp
cargo test -p vega --test nsh_workspace_mounted --features nsh-workspace-integration --offline --locked -j2 --target-dir target/official-nsh-validation
cargo check -p vega --offline --locked -j2 --target-dir target/official-nsh-validation
cargo clippy -p vega --test nsh_workspace_mounted --features nsh-workspace-integration --offline --locked -j2 --target-dir target/official-nsh-validation -- -D warnings
```

These are **provided, not run by this lane**. Expected custom-runner accounting is **6 passed, 0 failed, 0 not executed, 0 ignored** (three mounted plus three controller); no such fresh result is claimed. Production check is separate because the standalone runner compiles shared modules, not full VegaWindow wiring. Record exact toolchain/fork versions, dirty-source identity, logs, duration and failures. Use Cargo directly, not inferred Nextest discovery. Final-head gates remain separate.

## Earlier controller-extraction source identities (historical)

SHA-256, paths relative to worktree root:

| File | SHA-256 |
|---|---|
| `crates/vega/src/window/workspace.rs` | `264cc442038ae3f0089811bd83cac3f76a2b5a0befe355083c9da4780dec3457` |
| `crates/vega/src/window/nsh_workspace/mod.rs` | `c06a3ed1a68adcce8b808a878331ec172a534af920e54d741c31dc88145d6f1d` |
| `crates/vega/src/window/nsh_workspace/controller.rs` | `15eb4adeb343126c7a86b5dac96dbaf9c65b333a97c39f02353d0a5809809370` |
| `crates/vega/src/window/nsh_workspace/controller_regressions.rs` | `079c739e0c7a5c288bdf4e9c6e1c7b79bea6ad23876f2a7b545c5f5aa878fcc6` |
| `crates/vega/src/window/nsh_workspace/mounted_entry.rs` | `230e412a8ad473a088581fe60ce6af9bdca159830382ca7fd9670d7494ef78f9` |
| `crates/vega/src/window/nsh_workspace/mounted_main.rs` (unchanged) | `9a18bffd4132e894d6ad67b63df340fae535ca412c91a47339ca80f2d0e6bc04` |
| `crates/vega/src/window/nsh_workspace/mounted_regressions.rs` (unchanged) | `9aaff4201242a9e4e66a1cd6a0600387a6406494c4535f16718ed5d5d3cec8d5` |
| `crates/vega/src/window/nsh_workspace/separator.rs` (unchanged) | `dc15960ecb8591f6cbc1e13797b635deae0fccc8b65265a22ff813a7df0bea67` |

Pre-follow-up parent-wired `workspace.rs` was `e318039bd1faac5b6d3757392052a896a2168b3b130203157b919a074eee7c8c`. Git diff versus HEAD includes earlier parent changes, not solely this extraction. No commit/tree identity is invented for dirty bytes.

Previously stopped provenance remains unchanged:

- `docs/907/nsh-authorized-backend-patches.md`: `85a977a54323013cda2e9c4079e1d6ddeb8d5bc8ddc7ed216b3eecd01e7c4fea`.
- `docs/907/nsh-backend-current-manifest.json`: `92484b391f7942d44a3a521503e5a5bd3c7c2b1f5836fb53cad5521502e97348`.

## Earlier controller-extraction static checks and limitations (historical)

Executed in the task worktree:

```sh
/Users/puzige/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/rustfmt --edition 2024 --config skip_children=true crates/vega/src/window/workspace.rs crates/vega/src/window/nsh_workspace/{mod,controller,controller_regressions,mounted_entry}.rs
/Users/puzige/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/rustfmt --edition 2024 --config skip_children=true --check crates/vega/src/window/workspace.rs crates/vega/src/window/nsh_workspace/{mod,controller,controller_regressions,mounted_entry}.rs
git diff --check -- crates/vega/src/window/workspace.rs crates/vega/src/window/nsh_workspace docs/907/nsh-workspace-mounted-validation.md
sha256sum crates/vega/src/window/workspace.rs crates/vega/src/window/nsh_workspace/{mod,controller,controller_regressions,mounted_entry,mounted_main,mounted_regressions,separator}.rs docs/907/nsh-{authorized-backend-patches.md,backend-current-manifest.json}
rg -n '//|/\*|#\[allow' crates/vega/src/window/nsh_workspace/{controller,controller_regressions,mounted_entry}.rs
```

Formatting and scoped whitespace checks returned exit0; SHA results above; comment/suppression search returned no matches. One atomic workspace edit was rejected for ambiguous matching; source was inspected unchanged before applying uniquely contextual edits. A read-only token lookup initially hit shell glob expansion; a focused lookup found the real 272px token. Neither was a product/test failure. Rustfmt parsing is not type checking. Cargo/rustc execution and fresh six-case results remain unrun by this lane.

No OS-native drag/wheel, compositor output, cursor, real tab reveal scrolling/focus, AX traversal, fullscreen native window behavior, cross-platform, security review, consolidated independent review, archive availability, integration, merge or CI success is claimed. No unrelated code finding was turned into extra scope.

## Development-only cleanup

Keep tests until parent records actual execution and durably archives exact original source/patch, full hashes and output. Before master remove `mounted_{entry,main,regressions,observations}.rs`, `controller_regressions.rs`, all associated observation fields/hooks/cfg/modules and temporary Cargo feature/test stanza. Retain production `controller.rs`, its module/wiring, `drag_region`, passive masks and release-stop behavior. Do not roll back shared production decisions or weaken assertions during cleanup. Parent owns final zero-Rust-test inventory and final-head CI.

## Harness-only correction: final stopped evidence

Original failure inspected: `target/nsh-resumed-workspace-clippy-fixed.stderr` reports nine dead-code errors for `LINE`, `BASE`, `Events`, `Root`, `Fixture`, its methods and the three mounted callables in the ordinary `vega` bin test. This is harness wiring, not a changed product contract.

`separator.rs` now loads only `mounted_observations.rs` behind the unchanged test/feature gate and routes the same five hooks there. The new module contains only observation state and recording helpers. `Observed`, its fields and the `Observations` tuple storage use `pub(in super::super)`, the nearest common parent of separator and runner cases. The recording function is private and the five hooks remain `pub(super)`. `mounted_entry.rs` alone loads `mounted_regressions.rs`, with private imports preserving the original fixture's `super::background`/`super::drag_region` references. One module declaration was relocated and one shared-support declaration added; no Cargo or feature changes.

Prior resumed outputs were read, not executed here: `target/nsh-resumed-vega-regression.stdout` records app **4 passed / 0 failed**, and `target/nsh-resumed-nsh_workspace_mounted-regression.stderr` records workspace **6 passed / 0 failed / 0 not executed / 0 ignored**. These predate this correction and do not validate these final bytes. Source14/14, recovery2/2 and policy26/26 remain parent-reported, outside this lane; no overlap or rerun.

### Exact diff proof

A read-only Python reconstruction reinserted the extracted observation block and original imports, then verified the full original SHA-256 values before emitting unified diffs. Original/final differences are limited to the moved support block/imports, separator module/hook namespace and entry module/import wiring. Reconstruction matched the original full files:

| Original file | SHA-256 |
|---|---|
| `separator.rs` | `dc15960ecb8591f6cbc1e13797b635deae0fccc8b65265a22ff813a7df0bea67` |
| `mounted_regressions.rs` | `9aaff4201242a9e4e66a1cd6a0600387a6406494c4535f16718ed5d5d3cec8d5` |
| `mounted_entry.rs` | `230e412a8ad473a088581fe60ce6af9bdca159830382ca7fd9670d7494ef78f9` |

Before/after invariant fingerprints matched:

| Unchanged bytes | SHA-256 |
|---|---|
| `mounted_regressions.rs` from `#[derive(Default, Debug, PartialEq, Eq)]` through EOF: all Events/Root/Fixture definitions, methods, assertions, timing and three cases | `91249902fa65cf25ff5b30ced5c8700866b26851b13eba3ce733385f80773848` |
| Three mounted case functions from `pub fn five_pixel_region...` through EOF | `4c66526791feeda0b8e63a333d6be7539a024a46a140fdb4fd794237bba52a8a` |
| `mounted_entry.rs` from `super::run_cases(&[` through EOF: exact six names, callables and order | `42a0018f6c49c8930dfea6b236e1ef03adc7d72b92704ac0a6a64027beaeced6` |
| Extracted observation block, after removing only added visibility and normalizing the wrapped tuple declaration | `944984e35e575017c1f35692b40fa85e157460291078e1f4be46d4df0b842a1d` |

### Final Rust file identities

Paths are relative to `crates/vega/src/window/nsh_workspace/`:

| File | SHA-256 |
|---|---|
| `separator.rs` | `ef206e6156014b1ee136d5f3220cef59b7685ca22fb25e8608d24e0495b031cc` |
| `mounted_regressions.rs` | `76db729afb4cde3fad3693e2a4f2735d4b12b0840ac21b5aec78e4c9cf9f01f1` |
| `mounted_entry.rs` | `e16d2df6f89c584fc27118cab188b2ee5ef102a062de9012e84b102f2e66a822` |
| `mounted_observations.rs` (new) | `86deab2f362e736fb219f7db69cfc4362f8ec313ad6ea7dc69ecd929cd408f7c` |
| `controller_regressions.rs` (unchanged) | `079c739e0c7a5c288bdf4e9c6e1c7b79bea6ad23876f2a7b545c5f5aa878fcc6` |
| `mounted_main.rs` (unchanged) | `9a18bffd4132e894d6ad67b63df340fae535ca412c91a47339ca80f2d0e6bc04` |

Executed this follow-up: absolute 1.98.0-toolchain rustfmt with `--edition 2024 --config skip_children=true`, formatting then `--check` on exactly the four authorized Rust files; both exit0. Read-only Python SHA/reconstruction/invariant assertions passed, including unchanged full controller-regression and main-runner hashes and no code comments, allow/expect or ignore attributes in the four files. Scoped `git diff --check` exit0; these scoped paths are untracked, so the original-reconstruction diff and explicit whitespace inspection provide the meaningful changed-source evidence. Rustfmt parsing is not type checking. No Cargo/rustc/toolchain-version command, network, Git mutation, cleanup or unrelated write was executed.

### Parent next commands (provided, not run here)

The strict command must include the ordinary app-bin test; checking only `--test nsh_workspace_mounted` would miss the reported failure. From the task worktree, using the parent's existing pinned environment/cache:

```sh
export RUSTUP_TOOLCHAIN=1.98.0 CARGO_FORK_AUTO_UPDATE=0 TMPDIR=/tmp
cargo clippy -p vega --all-targets --features nsh-workspace-integration --offline --locked -j2 --target-dir target/official-nsh-validation -- -D warnings
cargo test -p vega --test nsh_workspace_mounted --features nsh-workspace-integration --offline --locked -j2 --target-dir target/official-nsh-validation
```

Required fresh accounting remains **6 passed, 0 failed, 0 not executed, 0 ignored**. Parent records actual Cargo/fork/rustc versions, commands, duration and outputs against these final hashes; neither strict Clippy nor the fresh six-case execution is claimed passed here.

READY: harness correction implemented; writes stopped. Parent strict Clippy and six-case regressions are next. Native acceptance, later development-test archival/cleanup and final-head gates remain parent-owned.

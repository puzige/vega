# Production icon residue — implementation lane

## Authority and acceptance before implementation

Assigned worktree `/Users/puzige/Workspace/worktrees/vega-907-nsh-full`, branch `feat/907-nsh-full`, parent-reported HEAD `0b5ea426abfd8d4358489e916273d89474fb190f`, fetched branch divergence 0/0 and ancestor master `34555cec9c0c4c4c1f7a528d739752f414e53edf`. The dirty worktree contains other implementation lanes. Do not repeat Git/network operations. Writes are restricted to `crates/vega_ui/src/icons.rs`, a new `icons/` implementation module and this record. Menu/sidebar files are read-only. Parent owns all compilation/tests and final integration; the older green compilation predates this implementation.

Replace both shared legacy renderers (`KitIcon` and GPUI `svg`) with a genuine NSH SVG Surface, instantiated lazily in the custom GPUI element lifecycle. Preserve exact existing inline bytes/embedded Kit asset selection, caller size and RGBA including alpha, fixed non-shrinking centered slot, parent opacity and clipping. No dependency, Kit/Fast, unsafe/global engine, controller, event, layout or theme contract changes. Retain the Surface across stable frames, publish only changed icon state, synchronize appearance/locale, disable its tab stop, hide its decorative AX descendants and stop it on release. Its NSH hitbox must not consume parent pointer/scroll gestures: prepaint with an empty content mask and paint with the actual intersected mask. No extra UI text or comments.

## Exact route trace

- Confirmed production shared-icon route: `conversation_stream/render_rows.rs::markdown_blocks` creates `code_header` for code blocks; `code_header` calls `icon(Icon::Code, colors.text_primary)`. The header's text and copy/receipt behavior belong to the parent/body lane; only its icon renderer changes here.
- `menu_list.rs::search_field` calls Search, `action_row` calls the requested glyph, and `selection_marker` calls Check. A full `crates/vega_ui/src` symbol search finds definitions only for these three helpers: they are not proven live production paths. Current branch/utility menus use their own NSH views and share only menu geometry constants. No menu edits are required for the shared-icon replacement.
- `icons.rs::icon_button` and `shell_icon_button` call the shared helper; repository source search finds definitions only, not current production invocations. Preserve their APIs/handlers. Their native wrappers are not evidence that all their visible decorations have migrated.
- `sidebar/threads_block/organization/projections.rs::render_project_thread` uses a shared Pin and `Spinner::new().icon(IconName::LoaderCircle)` (around lines 618/658–662). `organization_thread` reaches it only with `actions_enabled == false`, from the older project/timeline projection methods in that same file. No external calls to `render_group_projection`, `render_project_projection` or `render_timeline_projection` exist. Live `ThreadsBlock::render` calls `render_organization`, which uses `render_pinned_pi`, `render_projects_pi`, `render_recents_pi` and NSH leaf rows instead. Therefore the LoaderCircle is dormant source, not a confirmed live production residue; do not modify sidebar or claim it is fixed by shared SVG migration. If parent restores those projections, exact additional scope is `organization/projections.rs::render_project_thread` and its old row text/background/unread-dot/spinner, not a blanket icon-name replacement.
- NSH `native/pixels`, `newrunactivity/pixels`, `newthinking/pixels`, footer, composer, sidebar leaf, navigation controls and utility views construct `ns::svg(ImageSrc::svg_markup(...))` themselves. Their similarly named glyph/icon helpers are already native and are not calls to the legacy shared renderer.

## Tooltip ownership to preserve

At the initial icon-only handoff, `IconTooltip` in `icons.rs` still painted GPUI text/chrome. The incremental implementation below replaces that body. Confirmed live trigger routes:

- `conversation_stream/render.rs::render_resume_tail` → `resume_tail_ui.element` → `resume_tail.rs::Host::render`, visible-geometry-gated `nsh-resume-tail-proxy` tooltip at line 488: back-to-bottom label. Parent scroll lane owns the trigger; the shared body remains `IconTooltip`.
- `conversation_stream/pending_messages.rs::pending_button` → `native::Chrome::button` → `pending_messages/native.rs::Host::render`, `Content::Button` native proxy tooltip at line 579: disabled reason when present, otherwise action label. Parent core/control lane owns trigger changes.
- Live `organization/render.rs` pinned/projects/recents renderers call `render_pinned_row`, `render_pi_row`, `render_recent_row` → `threads_block.rs::render_row_at_height`; waiting-for-answer rows attach the tooltip at lines 1436–1438. Exact sidebar scope, if needed, is this tooltip trigger/body integration; no dormant organization projection rewrite is required.
- `menu_list.rs::action_row` disabled-reason tooltip remains source only: its helper has no callers. `icon_button` and `shell_icon_button` tooltip routes likewise have no callers.

Do not remove these labels or disabled explanations. The initial handoff assigned tooltip replacement to parent; the user's subsequent explicit implementation authorization below supersedes that assignment. Shared icon migration alone did not close tooltips, code-header text or other GPUI chrome.

## Validation boundary

Only scoped rustfmt and read-only static checks are authorized in this worker. No Cargo, rustc, Git, network, app launch, dependency/configuration changes or test cleanup. Existing development tests/helpers are preserved. Compilation, behavioral tests, final-version CI and real macOS/Windows/Linux visual/input acceptance remain the parent's work; none is claimed passed here.

## Implemented handoff

`icons.rs::sized_icon` now dispatches exclusively to `icons/nsh.rs`. The new custom element requests only the original fixed-size non-shrinking native layout slot. During prepaint it uses a hash of that retained Fast `LayoutId` as its window-local host key, not icon kind, raw geometry or a single shared callsite. This matters because multiple code headers use the same helper callsite without distinct ancestor element IDs. Fast retains the slot's layout node across stable frames; distinct live slots receive distinct node identities. The native child is laid out at the slot's actual origin/size with an empty input mask; paint uses the slot/ancestor clip intersection. The retained host owns the transparent NSH Surface, equality-gates `Glyph` publication, synchronizes environment, and stops the engine on release. The NSH subtree builds the exact SVG source with supplied size/text+tint RGBA and hidden decorative AX descendants. There is no visible GPUI icon child or legacy fallback. GPUI paint primitives used by the NSH platform retain the ancestor element-opacity context; no extra alpha multiplication is introduced by this host.

Only touched stale renderer comments were removed. Existing source assets, public helper signatures, tooltip bodies/control handlers and all other lane files were preserved. No tests/helpers were added or removed. This is an implementation handoff, not independent review or a claim of whole-production migration completion.

### Author validation and exact content

- `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`.
- Ran `rustfmt --edition 2024 --config skip_children=true crates/vega_ui/src/icons.rs crates/vega_ui/src/icons/nsh.rs`, then the same command with `--check`: both exited 0 on final code. No workspace formatter or Cargo invocation.
- Read final code and actual cached GPUI Fast 0.1.9 retained-layout, `LayoutId: Hash`, state, detached-layout, content-mask and opacity contracts plus the current NSH platform Surface implementation. No dependency changes.
- Read-only focused wiring checks found no `KitIcon`, `kit_icon`, `inline_icon` or GPUI `svg()` rendering in the shared helper; the only new SVG constructor is `ns::svg`. Verified presence of clipped passive prepaint/paint, environment synchronization and release stop. These are source checks, not behavioral tests.
- `crates/vega_ui/src/icons.rs` SHA256: `733a3751cd914fd061597ab857d5014fd9e488ebc4113cc1704a9aa918c90d04`.
- `crates/vega_ui/src/icons/nsh.rs` SHA256: `345e7220a75d05999b24bdc6f2029da5d1a8a362d2111c790f21bb2504749e5b`.
- Parent next verification: compile these exact hashes, then verify several differently colored/sized shared icons in one parent, stable-frame retention and release, theme/locale update, parent alpha and partial clipping, and click/hover/wheel/drag-through plus Tab/AX parity on the existing production routes. Current author execution is static only; parent pre-edit green does not validate these hashes.

Remaining confirmed live paths at the first handoff were the three tooltip routes above plus parent-owned code-header text/chrome. Dormant menu/LoaderCircle source is explicitly not a live blocker. No additional menu/sidebar write scope was exercised.

## Incremental shared tooltip implementation — acceptance before code

The user explicitly authorized continuing implementation of the three confirmed live `IconTooltip` routes, not independent review. Allowed writes now include `icons.rs`, `icons/` modules, the actual tooltip definition and only necessary adaptations at the three callsites, plus this record. The earlier read-only callsite restriction is superseded only for those necessary adaptations. Body source-anchor, vendor, shared scroll, Cargo, code header, dependencies, application execution, Git/network and other lanes remain untouched. Parent owns compilation/testing and integration; four implementation lanes finish before centralized independent review.

Preserve the shared `AnyView` tooltip factory and existing Fast tooltip manager. Keep its request, hover delay/cancellation, pointer-relative placement/window-edge avoidance and trigger keyboard/AX/action lifecycles unchanged. Keep original labels, optional shortcut, metadata font size, inherited family/weight/style/line-height, rem-derived gap/padding/radius, one-pixel borders and semantic colors. Preserve the original flex/text layout as measurement only: no GPUI body text/background/border may enter prepaint/paint. Project the measured label/chip rectangles into a retained transparent NSH Surface which alone draws text, background, borders and optional shortcut chrome. Intersect paint with the original tooltip/ancestor mask, retain ancestor opacity, suppress Surface hitboxes/tab stops, synchronize appearance/locale and stop the engine on release. Expose the complete tooltip content without duplicate decorative AX children or new actions. No new UI copy, comments, allowances, test changes or product behavior.

Expected minimal integration: replace `IconTooltip::render` centrally; the three original trigger files remain unchanged unless actual integration evidence requires adaptation. Static rustfmt/source inspection may be executed; Cargo/rustc/runtime/CI and native tooltip/keyboard/hover/AX/three-platform parity remain explicitly unverified in this lane.

### Implemented incremental handoff

- `icons.rs::IconTooltip::render` now delegates to `icons/tooltip.rs::element`; the public `tooltip(label, cx) -> AnyView` factory and original `IconTooltip` entity construction stay unchanged. Removed only the touched legacy tooltip-body comment; no new code comments or allowances.
- `icons/tooltip.rs::Body::request_layout` creates the former flex layout, with metadata-size GPUI text elements retained solely for measurement. Those measurement elements are never prepainted or painted; no old visible GPUI body is embedded. Actual computed label/chip/text rectangles are read in prepaint and projected to NSH. Rem-based gap/padding/radius and inherited family/weight/italic/line-height are carried forward. The one-pixel outer/chip borders and five original semantic colors are rendered by NSH. The NSH outer decoration is a sibling of the absolutely placed text/chip, so its border cannot add another content-position offset.
- A per-retained-layout-key `Host` lazily owns the transparent `Surface<Snapshot, ()>`, disables its tab stop, publishes only changed snapshots, synchronizes environment and stops on release. NSH descendants are decorative to AX; a non-drawing Fast `Role::Tooltip`/`aria_label` exposes the complete original label plus optional shortcut. The body adds no activation, hover, keyboard or focus handlers. Prepaint uses an empty input mask; paint intersects tooltip bounds with the actual ancestor clip. Original tooltip manager placement/edge avoidance and ancestor paint opacity remain host-owned.
- All three original trigger files are unchanged by this lane: `conversation_stream/resume_tail.rs:488`, `conversation_stream/pending_messages/native.rs:579`, `sidebar/threads_block.rs:1438`. Existing disabled reason selection, waiting-answer accessible label, geometry gating, pointer/hover state and keyboard/action proxies remain at those owners. No callsite adaptation was necessary for the unchanged factory signature.
- `icons/nsh.rs` remains byte-identical to the previous handoff. No source-anchor, vendor, shared scroll, Cargo, code header or other implementation-lane file was written. Existing tests/helpers were preserved without additions, cleanup or weakened assertions.

### Incremental author validation and hashes

Executed only `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)` and read-only source/hash checks. `rustfmt --edition 2024 --config skip_children=true crates/vega_ui/src/icons.rs crates/vega_ui/src/icons/tooltip.rs` and `rustfmt --edition 2024 --config skip_children=true --check crates/vega_ui/src/icons.rs crates/vega_ui/src/icons/nsh.rs crates/vega_ui/src/icons/tooltip.rs` exited 0 on the final code. Focused source inspection found no comments, `allow`, unsafe or test changes in the new module; only the actual Surface child enters prepaint/paint. Verified the three live factory routes and retained Surface environment/equality/release/input-mask wiring. These are parsing/format and source checks, not compilation or behavioral tests. Read pinned Fast 0.1.9 text/layout/tooltip placement contracts and existing NSH style/render contracts without modifying vendor/dependencies.

Final code SHA256:

- `crates/vega_ui/src/icons.rs`: `8df3fb951c8c127eaafde6a8dd5a94b6f6089e918c61cbf5f8c2c4535ee3e90f`.
- `crates/vega_ui/src/icons/nsh.rs`: `345e7220a75d05999b24bdc6f2029da5d1a8a362d2111c790f21bb2504749e5b` (unchanged).
- `crates/vega_ui/src/icons/tooltip.rs`: `0cad6d6194f1a77aed609a8e0eb0308ec8d80b6aa096ca0a0174b37238e987fb`.

Not run: Cargo/rustc compilation or version queries, Clippy, tests, CI, Git/network, app launch, native tooltip delay/cancellation/hover/focus/AX checks, pixel geometry/clipping/opacity verification or macOS/Windows/Linux runtime acceptance. Parent's earlier green compilation predates these exact bytes and does not validate them. Parent's next verification is compile/type-check the exact code, then exercise the three live routes including disabled reasons, theme/locale, optional shortcut/long CJK+emoji text, window-edge placement, partial clipping/alpha, repeated hover show/cancel/release and unchanged Tab/Enter/Space/AX action discovery. No new review gate is introduced: centralized independent review remains after all four implementation lanes finish.

Implementation closure in this lane: the three confirmed live shared-tooltip bodies now use NSH text/chrome; their actual production behavior is pending the above parent validation. The known code-header text/chrome remains parent-owned and untouched; dormant menu/LoaderCircle source is not promoted to a live blocker. No further route expansion was performed. Stop writes at this incremental handoff. The record's own final SHA256 is supplied out-of-band in the handoff to avoid a self-referential digest.

## Integration risk correction and diagnosis — current handoff

This is implementation/diagnosis, not independent review. User returned exclusive ownership of `icons.rs`, `icons/nsh.rs`, `icons/tooltip.rs` and this record. Parent's two `ns::bind` → `ns::dynamic` corrections are preserved. Parent reports successful library compilation, compilation of 14 mounted/24 geometry/11 nested cases, and 24 font/gesture cases passing; these reports predate this correction and are not author-run validation of its final bytes. No body/source-anchor, vendor, scroll, code-header, Cargo, dependency or test file was written. `Fragment.row` is outside this lane.

### Original failure and attribution boundary

Read `target/nsh-source-anchor-mounted.stderr`: lines 129–155 show a 1.879229083-second drain tick, later 206.585333-ms height update, then the unchanged 2-second deadline at tick 13 (`queued=(4, 0)`); the drain elapsed 2.118527667 seconds. Runtime counters include repeated repaint requests for surfaces 2/4 and five initially numbered surfaces, but do not map those numbers to icon hosts or record repeated Surface construction. They do not by themselves establish an icon recreation loop.

Read-only fixture trace: `conversation_stream/selection_menu/body_integration_tests.rs::Fixture::new` inserts `ThinkingDelta { delta: source() }`; `source` at lines 230–239 is a measured Markdown link plus 60 ordinary paragraphs, with no code block. `FixtureRoot::render` mounts the Thinking reading list, not the sidebar tooltip host. `newthinking/pixels.rs:111–139` builds Bot/disclosure `ns::svg` inside its own existing Surface and only calls `nsh_source` for asset bytes; it does not call `icons/nsh.rs::element`. No shared tooltip is requested during fixture initialization. Thus the observed initial failure does not exercise the new shared code-header icon or a shown `IconTooltip`. This is a source-route exclusion for the reported initialization failure, not a claim that the scheduler failure is fixed or all icon runtime risks are excluded. Body diagnosis stays with its owner.

### Fast 0.1.9 identity evidence and minimal correction

Read the actual pinned registry source, not an assumed upstream API:

- `window.rs:4392–4428` documents and implements `use_keyed_state` as consecutive-frame state under `with_global_id`; `use_state` merely supplies a caller-location key, not an automatic per-instance identity. `window.rs:7378–7407` reads state from next/rendered frame by `(GlobalElementId, TypeId)` and observes explicit notifications. Updating the host does not unconditionally call `notify` in this lane.
- `window.rs:5257–5279`, `taffy.rs:58–80`, `fast/layout_key.rs:180–236`, and `fast/layout.rs:388–410,999–1057` show that this Fast fork retains Taffy nodes by element path, including positional unidentified children. `clear` releases unclaimed nodes; it does not recreate every node on each frame. A repeated key claimed twice within one frame instead creates an unretained transient node.
- `NativeIcon::request_layout` makes one direct layout request. For consecutive stable-path frames, its LayoutId is actually retained in this pinned fork, so its existing layout-hash state key is supported by the inspected implementation. It is not asserted stable across structural/path changes or duplicate keys; leaving/re-entering a path can recreate and stop its Surface as before. Replacing it with a fixed/caller key without an additional per-icon identity would incorrectly alias multiple icons below the same named ancestor. No such change was made.
- The previous tooltip measurement made two direct requests under the `Body` layout key when `shortcut` exists: chip first, root second. The root is consequently transient; hashing it into `use_keyed_state` could recreate Surface/engine/root on every such frame. Removed the root from stored measurement and changed only the host lookup to the fixed `nsh-icon-tooltip-surface` key under the existing `IconTooltip` entity view and `nsh-icon-tooltip` named AX parent. There is one Body per such host, so separate tooltip views remain distinct without a layout identity. With or without shortcut, geometry/theme changes now publish to the existing Surface rather than choosing a new one. The measurement layout itself still follows the original size calculation; no geometry or scheduling budget was changed.

### Empty prepaint mask, pixels, AX and nested retained paint

- `vendor/nsh-platform-gpui/src/render.rs:741–853` uses full supplied bounds for origin/resize/layout and inserts its blocking hitbox during prepaint. Its SVG/text/chrome pixels are emitted during paint, not prepaint. `access.rs:921–993` builds AX from NSH root/origin/scale without consulting the Fast content mask. `window.rs:4218–4231,5346–5360` and `fast/layers/input.rs:317–330` show that an empty prepaint mask removes the child's hit region. Keep this passive-input behavior; no new event, focus or pointer handler was added. Decorative NSH descendants remain hidden; the complete tooltip label/shortcut remains on the original non-painting Fast Tooltip/aria-label parent.
- There is a genuine nested-cache risk if only the actual ancestor clip changes while the empty prepaint mask, bounds and opacity remain equal: `fast/retained.rs:607–616,954–969,807–829,1915–1936` compares the prepaint context but can copy an earlier paint range with its old clip. Added `icons/nsh.rs::paint_id`, hashing `bounds.intersect(window.content_mask().bounds)` into the **child drawing wrapper identity only**, and applied it to both icon and tooltip children. Host lookup and Surface creation happen before/outside this wrapper identity. A clip change therefore selects a fresh nested paint record without recreating the Surface/engine/root; identical clips retain the drawing identity. Parent opacity remains in Fast's retained-context comparison and normal paint propagation.
- The wrappers do not scroll; `fast/layers/paint.rs:109–125,228–235` bypasses layer culling for them. Their prepaint still receives the zero input mask and their paint still receives the actual current intersected clip. No GPUI icon/text/chrome rendering fallback was introduced. Source inspection is not a pixel/AX runtime pass; repeated clipping/opacity and assistive-technology behavior remain for parent to exercise.

### Current-cycle author validation and hashes

Executed focused read-only source/log inspection and `rustfmt --edition 2024 --config skip_children=true --check crates/vega_ui/src/icons.rs crates/vega_ui/src/icons/nsh.rs crates/vega_ui/src/icons/tooltip.rs`. The first check reported only the new hash helper's chained-call formatting; corrected that exact formatting and the final check exited 0. Focused module scan found both `ns::dynamic` calls and no `ns::bind`, new comments, `allow`, unsafe or test changes. SHA256 of final code:

- `crates/vega_ui/src/icons.rs`: `8df3fb951c8c127eaafde6a8dd5a94b6f6089e918c61cbf5f8c2c4535ee3e90f` (unchanged this cycle).
- `crates/vega_ui/src/icons/nsh.rs`: `fd55d1d1b70906fdbf4d120b5423c0eb571969ce6b9ba7678f6832c0a4dbfafc`.
- `crates/vega_ui/src/icons/tooltip.rs`: `069253cebd585c86c097e965ceebb22d87dc213ea3b7cfda3fe21e1b220faac5`.

Not run: Cargo/rustc or their version queries, compilation/tests/CI, Git/network, app launch, frame instrumentation or macOS/Windows/Linux runtime/pixel/AX validation. No deadline/assertion/budget was relaxed and no regression removed. Parent owns compilation and the unchanged mounted failure rerun; this lane does not claim to repair that source-anchor initialization failure. For this correction parent should verify repeated shared-icon frames, tooltip with/without shortcut across hover show/cancel/release, geometry/theme updates, ancestor clip/opacity changes and parent pointer/scroll/keyboard/AX behavior. Earlier compilation/pass reports do not cover these final code hashes. All four implementation lanes precede centralized independent review; no additional gate is introduced. Stop writes after this handoff; document hash is provided out of band.

## Necessary mounted regressions — implementation, not independent review

### Scope and exact runner exports

Current user authorization adds only `icons/host_regressions.rs` and feature-gated observations in the owned icon modules; no Cargo, vendor, source-anchor/body, runner or shared-scroll edits. Parent-reported base remains `0b5ea426abfd8d4358489e916273d89474fb190f` plus concurrent dirty changes; no Git command was used to reassert the base. Root AGENTS, execution/design guidelines, full migration contract and the existing icon evidence were read; the worktree scan found no nested AGENTS under crates/docs.

Existing feature `nsh-reading-integration` exposes `vega_ui::icons::host_regressions`. Parent can add these exact three `fn(&mut gpui_kit::TestAppContext)` functions to the central mounted runner:

- `shared_icons_retain_native_surface_across_frames_and_theme`
- `shared_icon_clip_motion_and_opacity_preserve_parent_input`
- `shared_tooltip_shortcut_toggle_retains_native_layout_and_accessibility`

Reuse the existing main-thread runner setup in `crates/vega_ui/tests/nsh_reading_integration.rs`: real platform TextSystem, `TestAppContext::build_with_text_system`, and `gpui_kit::init`. Each fixture asserts real fonts are available, opens an actual test window and mounts the production `sized_icon`/`icon_button_with_id` and actual `IconTooltip` entity. It uses `Window::draw` and real NSH engine/Surface creation, not mocked drawing or source-text assertions. The fixture closes its window and removes its per-app observation global on drop. No runner registration was performed in this lane.

### Cases and observation boundaries

1. Co-mount embedded Search and inline Pin SVGs plus a shared tooltip. Check selected SVG bytes, eight unchanged frames without another native projection, one Surface entity per slot and one renderer/root setup, distinct entities between slots, then 16→24 logical-pixel native layout, exact RGBA including alpha, and light-theme publication to icons and tooltip. The renderer setup count is measured at the actual `pixels` entry point; Surface identity comes from its actual AppView entity. The backend does not expose a public generic engine/root token, so these observations detect Surface recreation and repeated renderer/root setup but are not a direct private-engine-pointer assertion.
2. Move and narrow the actual ancestor overflow clip while changing opacity 1→0.35→1. Check moved host bounds, the actual intersected paint clip and restoration, zero prepaint input mask, unchanged Surface/setup identity, exactly-once parent mouse-up/wheel delivery, and original Enter/Space activation with host focus retained. Paint-path observations are taken in the real custom Element paint method. They do not inspect final compositor pixels or prove alpha blending.
3. Mount the actual shared tooltip entity without a shortcut, add `Ctrl+End`, then remove it. Wait for native renderer state and native layout to agree with the real measured host bounds, require natural-width growth and exact size restoration, unchanged Surface/setup identity across repeated frames, and parent click/wheel delivery through the tooltip body. The existing non-painting tooltip chrome construction is factored without behavior change into `tooltip::chrome`; the regression invokes its real Fast `a11y_role` and `write_a11y_info` methods and checks the Tooltip role plus complete label with/without shortcut.

Observations are compiled only with the existing integration feature: per-fixture App global plus an Arc/Mutex recorder carried with the real Surface snapshot. There is no static/global engine. Host reads, renderer setup, dynamic projection, native `on_layout`, passive mask and paint clip update only that recorder. Setup-time Signal reads use `get_untracked` so instrumentation does not subscribe the root component and accidentally change recreation behavior. Neither observation method publishes state or requests a redraw. Both parent-fixed `ns::dynamic` calls remain intact. Product trigger, delay, cancellation, hover/keyboard lifecycle, clipping, input policy and native AX hiding remain unchanged.

Budgets remain 2 seconds and 512 scheduler ticks per drain, 10 seconds per mount/update wait. Input uses the real `Window::dispatch_event` followed by the same bounded drain; `VisualTestContext::simulate_event` was deliberately not used because its implementation calls unbounded `run_until_parked`. No existing assertion/test/fixture or scheduling budget was changed.

### Current evidence and limits

Author executed focused API/source inspection, scoped rustfmt formatting and final `rustfmt --edition 2024 --config skip_children=true --check crates/vega_ui/src/icons.rs crates/vega_ui/src/icons/nsh.rs crates/vega_ui/src/icons/tooltip.rs crates/vega_ui/src/icons/host_regressions.rs`; the final check exited 0. The new icon modules contain no ordinary comments, allow or unsafe, and no dependencies were added. These are callable regression implementations, **not executed passes**. No Cargo/rustc (including version queries), Git/network, App launch, compilation, tests or CI were run by this lane. Previous parent passes predate these bytes and cannot validate them.

Remaining limits: this main-thread test platform path has no pixel readback wired here; final SVG/text/chrome raster pixels, clip-cache output and opacity blending still need actual renderer/platform verification. AX checks exercise the real Fast tooltip role/label construction, not an active screen-reader tree or NSH private AX builder. Decorative native AX hiding and lack of native tab stops are retained production settings, not an independently observed platform AX pass. The tooltip is deliberately mounted directly to cover its real body and entity retention; delayed show/cancel, dismissal/release/remount and all three live trigger routes still need parent runtime validation. No claim is made that the separate source-anchor initialization timeout is repaired. No new review gate or unrelated path investigation is introduced.

Final code SHA256 for parent compilation:

- `crates/vega_ui/src/icons.rs`: `2a87d60acb500e0d8005bd6fe2975064a0a243250ca1f3437b460aa4247f236c`
- `crates/vega_ui/src/icons/nsh.rs`: `f1b574e5ac52d6f8af008801e91a78a299a3502cd72771706302ab8934e51197`
- `crates/vega_ui/src/icons/tooltip.rs`: `8742b44cd31f1435500be0ef0291712c031b2a76e2cb5db2cb39f46e98f96d93`
- `crates/vega_ui/src/icons/host_regressions.rs`: `53e03d35d2b9da321fff59ec4451247fc2ef26364936d583f8b8de075df0f513`

Document SHA256 is handed off out of band. Stop writes after this handoff; parent owns central compile/run and subsequent integration.

## Dedicated icon runner continuation — implementation contract

Current authorization extends this lane to a new `crates/vega_ui/tests/nsh_icons_integration.rs` and only its `[[test]]` registration in `crates/vega_ui/Cargo.toml`, in addition to the owned icon modules and this record. Parent already fetched and fast-forwarded the branch before this continuation; do not repeat that operation against the shared dirty migration. Read-only Git inspection confirms branch `feat/907-nsh-full` at `0b5ea426abfd8d4358489e916273d89474fb190f`. No nested AGENTS files were found. Required root instructions, execution guide and issue workflow were inspected before edits.

Wire exactly the three delivered exports above into one `harness = false` executable using the existing `nsh-reading-integration` feature, without new features/dependencies. Follow the reading/scroll runners' exact setup API: retain the actual `current_platform(true)` platform, build one `TestAppContext` with `TestDispatcher::new(0)` and `platform.text_system()`, require nonempty platform fonts, enable `executor().allow_parking()` and call `cx.update(gpui_kit::init)`. All three cases receive the same mutable context. Preserve the authored assertions and 2-second/512-tick drain and 10-second wait budgets unchanged. Log each started case; count successful returns and caught panics separately; stop after the first case panic rather than reuse a potentially compromised context; report the exact remaining not-executed count and exit nonzero on failure. Setup failure must not be reported as a case pass.

This is implementation only. Parent centrally owns Cargo/rustc/version queries, compilation and execution; no independent review or native acceptance is performed. Source-anchor/body/shared NSH/backend/reading runners remain read-only, and all four implementation lanes must finish before consolidated review.

### Dedicated runner implementation and actual static evidence

Implemented the dedicated main-thread runner and single feature-gated manifest registration. An outer unwind boundary reports setup/teardown failures separately and still emits the actual case counts; an inner boundary records a failed case and stops. Only successful case returns increment passed. No filters, ignored cases, retries or budget overrides were added. No statically proven author fix was needed, so all four icon source files remain byte-identical to their preceding handoff hashes.

Actual current-cycle checks, all exited 0:

- `rustfmt --edition 2024 --config skip_children=true --check crates/vega_ui/tests/nsh_icons_integration.rs crates/vega_ui/src/icons.rs crates/vega_ui/src/icons/nsh.rs crates/vega_ui/src/icons/tooltip.rs crates/vega_ui/src/icons/host_regressions.rs`.
- `git diff --check -- crates/vega_ui/Cargo.toml crates/vega_ui/src/icons.rs`.
- Read-only Python `tomllib`/SHA256/source consistency checks: the target is registered exactly once with `harness = false` and the existing feature; removing only that stanza reproduces the original manifest SHA256 `da29d4474a6208c96f4f36d9229823b30e4334b09e29980f76d5da3e1b053f47`; runner references match exactly the three public `fn(&mut TestAppContext)` signatures; exactly one context build/parking/init sequence exists; all four icon source hashes match the preceding handoff; the new runner contains no ordinary comments or lint suppressions; runner/document whitespace and final newlines are clean. These checks are static wiring evidence, not compiled API validation or test passes.

Changed paths and SHA256:

- `crates/vega_ui/tests/nsh_icons_integration.rs`: `37eb5dc33cdc3bd76aec971e6cdd978bc6e3a83a05202f2b5f698a68a9459498`.
- `crates/vega_ui/Cargo.toml`: `61d7954417c2f82cabc33b2b2f7279a150e518a1d0c5e18d2c43cf17d5718e48`.
- `docs/907/nsh-production-icon-residue.md`: final hash handed off out of band.

Current mounted execution counts: **0 passed; 0 failed; 3 not executed**. No Cargo/rustc/version queries, compilation, tests, CI, dependency/Kit/Fast upgrades, app launch or native acceptance were performed. No Git/network mutation or commit was made. Existing runtime/pixel/AX and tooltip-trigger lifecycle limitations above remain unresolved; this wiring makes no claim about source-anchor failures or other lanes.

Exact proposed parent command, not executed by this lane:

```sh
cd /Users/puzige/Workspace/worktrees/vega-907-nsh-full
cargo test --locked -p vega_ui --features nsh-reading-integration --test nsh_icons_integration
```

Parent records its actual toolchain, result and elapsed time and returns concrete compile/runtime failures if repair is required. **READY for parent compile/run; this lane stops writing after the final read/hash handoff.** Consolidated independent review remains after all four implementation lanes, not part of this continuation.

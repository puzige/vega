# Tool / timeline scroll composition

## Newly approved protocol implementation (in progress)

The user separately approved both minimal fixed-version NSH scroll capability extensions. Cross-Surface exact consumption/remainder and gesture ownership are now authorized for implementation, not already accepted. No Kit/Fast source modification or version change is permitted. The input/events/Surface backend lane owns wheel accounting and routing; parent owns shared render/host/list wiring; a separate author lane builds actual-host regressions. Original outer List identity/follow/business/selection authority remains unchanged. Required cases include no-target/edge/partial transfer, mixed axes, start/move/end/cancel, ownership across pointer/route/mount changes, momentum and exactly-once parent scroll without replay or a second outer scroll state.

Before these new changes the integrated library/host/geometry/mounted, shell, strict workspace Clippy, Python/fmt/i18n and 370 ordinary Nextest cases passed; see `nsh-reading-editing-integrated-validation.md`. Those results are not validation of the new protocol. Native three-desktop acceptance and consolidated independent review remain pending after implementation.

### Original List host integration acceptance

The parent retains a stable shared route on the existing TurnPlacement controller. A scoped native-host route surrounds the original List child prepaint and paint; every actually mounted Surface binds to that route, including retained Surfaces, without recreating gesture ownership on unchanged frames. Scope exits restore the prior route on normal return or unwinding. Surface reparenting/unmount/route replacement revokes old gesture authority. No route is inferred from pointer coordinates alone.

The host installs its listener after the original List listener but before child Surface listeners. Thus first-event bubbling observes the child’s single-use remainder before the original List can consume the original delta. Captured owned events are delivered directly once. Forward only the remaining vertical component through the existing public ListState::scroll_by with its positive-content-offset direction, preserving the original line conversion and clamping against actual content extent, viewport and bottom padding. Preserve placement interruption, direction, scroll notification and view invalidation explicitly because public scroll_by does not call the original scroll handler. Horizontal-only remainder must not invent vertical movement. No Fast changes, replayed ScrollWheelEvent, second outer scroll state or unconditional queued scroll.

This is implementation acceptance, not a passing behavioral result. Real mount regression execution, stale owner/remount checks and consolidated independent review remain pending.

## Previous full-lane implementation delta

The earlier Agent4 sections below are historical. Latest authorization gives lane 1 run_activity/newrunactivity/sticky_disclosure, ToolActivityGroup/ToolCard and disclosure ownership; parent coordinates all Cargo and defers independent review until all four implementation lanes finish. See [timeline current acceptance/implementation](nsh-run-timeline.md#full-timelinescroll-lane-acceptance-before-implementation). No dependency, registry, vendor, core, Fast, Cargo, runner or shared body source changes are made by this lane.

- Real NSH gradient edges **and interactive thumb paint** now replace the GPUI scrollbar/fade in run details, stage detail sections and expanded tool groups. GPUI remains the original ScrollHandle/layout/event/clip adapter, not a legacy visible control hosted inside NSH. Child prepaint supplies actual viewport/max-offset/offset. Original maximum height, child order, sole outer List, containment callback, focus/source/selection and anchor/Final decisions remain.
- `disclosure::scroll_chrome` owns the passive NSH chrome plus clipped mouse/touch track proxies; input checks a live retained lease, generation, root viewport/scale, visible active window and Surface errors. Geometry changes revoke drag; idle callbacks cannot revive a dropped/replaced mount. Stable snapshots do not publish. Old `tool_activity_group/nsh_scroll.rs` is a production alias to this renderer, not a second implementation. Its two existing fade regressions are wired to the new production code without dropping their assertions; a real extent/track/drag mapping regression is added.
- ToolCard's existing NSH detail scroll uses the existing `nsh-base` built-in Scrollbar, replacing platform indicator paint using actual engine target/metrics, real scrollbar drag/AX and system Hover/Always preference. Original detail content/selection/copy is untouched. Tint/mode equality prevents theme reinstallation on unrelated snapshots. No package added and no second scroll state.
- Tool group header stable publication now compares id/content/focus and actual used colors locally. Currentness/activation and all old tests stay intact; a convergence/content/focus/theme/recycle regression is added.
- Sticky paint migration removes only both GPUI wrapper backgrounds: NSH header background already exists. StickyDisclosure is retained for exact native placement/masks/child order. Thinking/omission/body source entry points belong to the other lanes and are unchanged here.

At that earlier leaf-stage checkpoint the cross-Surface wheel boundary was **BLOCKED, not accepted** (now separately authorized above): actual vendored `events.rs:303-329` always stops propagation after wheel/touchpad dispatch, and `input.rs::wheel`/`touchpad` do not return consumed/remaining deltas. NativeComponent also owns a BlockMouse hitbox. Required minimal authorized backend work is exact consumption/remainder and gesture ownership through native children/NSH ancestors to the original outer List without replay, double handling or a competing ancestor scroll state. This writer does not patch that boundary.

Validation: scoped rustfmt/static only; no Cargo, typecheck, tests, Clippy, CI, native, Git/network executed. Parent must run the unchanged three-case main-thread real-font runner and unified app/host/geometry tests after all four lanes stop. Prior 287/33/12 results predate this delta; the old first body case failed and third header did not execute. No passing or native acceptance claim. Native scrolling/drag/touch/AX/motion/theme/fault/clip fidelity and independent review are outstanding. Final hashes are supplied in the stopped-writer handoff.

## Historical Agent4 delivery

## Scope and acceptance recorded before implementation

Parent-supplied baseline `0b5ea426abfd8d4358489e916273d89474fb190f`, branch `feat/907-nsh-full`, worktree `/Users/puzige/Workspace/worktrees/vega-907-nsh-full`. Direct metadata reads confirm branch and HEAD. Parent reports fetch and ancestor verification of master `34555cec9c0c4c4c1f7a528d739752f414e53edf`; no Git/network operation is delegated or repeated. Existing dirty work from the other three writers must remain untouched. No nested AGENTS exists under crates/docs at inspection.

Writable scope: ToolActivityGroup and ToolCard plus their submodules; `conversation_stream/sticky_disclosure.rs`, `stream_fade.rs` and corresponding new submodules; this document only. Thinking/body/source selection belongs to Agent1. Shared Surface/core/render/mod.rs/Cargo and the independently closed seven-test timeline header batch are read-only. Existing NSH headers must not be redone. Parent owns all Cargo/build/test execution. This writer may run scoped rustfmt and static checks only.

Acceptance:

- Actual NSH child content width/height is reported with projection and native mount authority, not a text-width guess or constant row-height substitute. Height estimates remain bootstrap/list estimates, never measured geometry.
- Delayed callbacks from a previous content projection, width, unmount/remount or layout generation cannot overwrite current geometry. Parent layout uses accepted actual child height and receives ordinary entity notifications without enabling list follow or changing list anchor policy.
- The production expanded ToolActivityGroup keeps its original single GPUI ScrollHandle, children/order, maximum viewport, containment handler, child Surface render entry and parent edge handoff. No second vertical scroller is introduced to evade anchoring.
- Independently implementable visible NSH scroll chrome uses actual original-scroll metrics. No legacy visible GPUI subtree is hosted in a Surface and described as migrated. Preserve complete detail command/output, selection, expansion/Final-collapse decisions, and focus minimum reveal.
- Sticky placement/masking and omitted-state ownership remain with the existing timeline controller unless an in-scope integration can preserve its exact behavior. Do not edit the frozen timeline batch to obtain new call sites.
- Rust development regressions, if added, remain UNRUN until parent execution and remain present for final archival/removal. Three-platform native behavior is unaccepted.

## Platform blocker — stop this part, not independent implementation

Pinned registry sources inspected directly (`nsh-platform-gpui-0.4.0`):

1. `src/native.rs:823-853`: `ComponentItem::prepaint` renders at exactly `geometry.document.size`; visible native children insert `HitboxBehavior::BlockMouse` at line 836. A NativeComponent containing a ToolCard Surface therefore cannot transparently feed its ancestor NSH scroll tree.
2. `src/render.rs:750-775`: every AppView Surface inserts its own BlockMouse hitbox at line 772. Transparent paint and NSH `pointer_events(None)` do not remove this GPUI hitbox.
3. `src/events.rs:303-329`: Surface wheel handling negates GPUI content-moving deltas (`Pixels` and `Lines`, lines 308-313), dispatches wheel/touchpad, then unconditionally calls `cx.stop_propagation()` at line 328. It does not report whether any NSH scroller consumed the delta or reached an edge. Thus even without NativeComponent nesting, a tool Surface can swallow wheel before the original parent GPUI ScrollHandle sees it.

A correct platform change would need native-child wheel ownership/ancestor dispatch plus a consumed/remaining-delta result for Surface wheel/touchpad handling, stopping GPUI propagation only for the consumed gesture and preserving boundary transfer, direction and gesture phase. A paint-only Surface also needs a supported input-transparent hosting mode instead of treating visual transparency as input transparency. This is a requested API boundary, not an implemented or approved patch. Exact design requires platform-owner review; simply deleting BlockMouse or stop_propagation risks duplicate child/parent scroll and broken pointer/selection routing. User authorization covers only NSH reading-selection/native-edit source packages, **not scroll/platform patches**. No dependency edit is performed; full nested NSH scrolling stops here and is reported to the parent.

## Implemented bounded production delta

- `tool_card/nsh_tool/layout.rs`: a real GPUI layout wrapper supplies actual available child width and window scale to the existing ToolCard NSH Surface. Width/scale changes rotate an opaque layout identity, including change-and-restore. The retained mount has weak ownership from the presentation; recreated mounts have distinct identities.
- `tool_card/nsh_tool.rs`: actual NSH root `ElementRef::measure()` returns width/height. Both projection changes and layout callbacks request measurement. The callback's old bounds are not relabelled as a newer projection: the callback explicitly measures the current commit and sends its captured projection/authority through original-owner ordered commands. The host rejects obsolete content identity, wrong native width/scale/mount, nonfinite/zero/negative sizes and Surface errors. Identical publications stabilize. Accepted geometry updates the existing host height and notifies the original ToolCard entity; no follow-bottom or scroll offset mutation is added.
- ToolCard expansion, summary refresh, subagent updates and presentation release synchronously invalidate pending geometry, including ABA transitions. Content equality covers the full rendered detail including output/copy/footer, not a row-count approximation. Existing estimates and last host height remain provisional bootstrap geometry while a new layout is pending; they are not exposed as measured geometry.
- `tool_activity_group/nsh_scroll.rs`: production expanded groups now paint real NSH top/bottom linear-gradient views, using the original ScrollHandle's actual viewport, clamped offset and max offset after child prepaint. The original fade length, quarter-viewport cap, epsilon/strength and scrollbar gutter are preserved. Equivalent chrome publications stabilize. The NSH Surface contains only new gradient views, not the GPUI child tree.
- The decorator prepaints its paint-only Surface with a zero-area content mask, then paints with the actual clipped viewport, matching the frozen header divider's existing passive-host pattern. Pinned GPUI `window.rs:5346-5359` stores the current content mask in hitboxes, and `fast/layers/input.rs:317-329` intersects it with the viewport. This is source-level input-isolation evidence, **not native pointer/keyboard/AX acceptance or a general platform passthrough API**.
- Original ToolActivityGroup header implementation, child order, ToolCard render call, one GPUI vertical scroller/ScrollHandle, containment callback, interactive scrollbar, max height, estimated height, business aggregation and expansion persistence remain. Original ToolCard detail scrolling, complete command/output and selection rendering remain; no new second vertical scroller is introduced.

## Parent API and integration

New callable contract: `ToolCard::measured_size(&self, cx: &App) -> Option<gpui_kit::Size<gpui_kit::Pixels>>`. `Some` means positive finite actual content geometry for the current content projection and current native mount width/scale, with no Surface error. `None` means pending/stale/unmounted/failed; do not use an estimate as an acknowledged size. Production ToolCard rendering already consumes this contract. Ordinary ToolCard entity notifications continue through existing parent observers. No shared-file or Cargo wiring is required for this bounded implementation.

A future parent native-child adapter must supply the actual assigned child width, use the acknowledged returned height, invalidate after child/source/layout changes, and keep the original anchor/Final-collapse owner. That adapter is **not implemented** because wheel handoff still requires the unauthorized platform boundary above. Do not use `estimated_height()` as NativeComponent measured height or create an independent ancestor NSH vertical scroller.

No timeline API/call-site change is requested for this bounded delta. Full timeline decorative migration would require an approved parent integration point for shared measured scroll chrome and omitted-state rendering while retaining existing sticky mask/region semantics; the frozen `newrunactivity` batch must not be reopened just to obtain those hooks.

## Remaining limits / explicit noncompletion

- Full nested NSH tool/timeline scrolling is BLOCKED by the pinned platform wheel path above. Existing ToolCard Surface wheel swallowing is a pre-existing limitation retained here, not repaired or accepted. Original GPUI parent scroll edge logic is retained, **not proof it is reached over native child pixels**.
- Interactive group scrollbar is still GPUI. NSH edge-fade paint is implemented, not native scrollbar migration. No platform patch, synthetic wheel replay or second vertical scroller was added.
- Sticky positioning/clip/background, omitted-thinking visible text and timeline body scroll/fade remain unchanged and unmigrated. Their production calls live in the frozen timeline/Agent1-owned body paths; no GPUI text is wrapped in a Surface and labelled NSH. `sticky_disclosure.rs` and `stream_fade.rs` were not modified; stream fade is text-appearance animation, not the scroll-edge fade implementation.
- The layout contract reports content size, not compositor paint acknowledgement or header input authority. Retained hidden rows/current route/owner focus eligibility remain governed by existing parent integration. This delta does not add route/owner/frame acceptance to ToolCard activation or claim native clipping/focus lifecycle parity.
- Focus minimum reveal, selection, Final collapse and anchor controllers are left in place, not newly validated. Dynamic child sizes, narrow/wide wrapping, scrolling both directions at all nested boundaries, scrollbar dragging/system always-show preference, Tab/Shift-Tab/AX/focus reveal, inactive windows, clipped/recycled rows, pixels/theme and macOS/Windows/Linux require parent/user native checks.

## Actual verification and stopped handoff

Status: **READY for parent unified compilation and independent review; Agent4 stopped writing.** This is a coherent bounded production delta, not a compiled/full-migration/native acceptance claim.

Executed successfully:

- Explicit seven-file rustfmt and `--check`, edition 2024, `skip_children=true`, using `/Users/puzige/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/rustfmt`: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`. The default PATH formatter was observed as 1.8.0 and was not used for mutations.
- Read-only inline Python static check: UTF-8, final newline, no trailing whitespace/conflict markers; production files contain no comments or unwrap/expect; original group single-scroller, ScrollHandle handler, ToolCard render call and interactive scrollbar remain. This is source hygiene, not a behavioral regression run.
- Pinned public API/source cross-checks for ElementRef measurement/Copy semantics, OrderedCommands, NSH gradient builder, MountItem::Events, GPUI ScrollbarHandle and hitbox masks. Corrected a test-only guessed enum name to the actual `MountItem::Events` before final checks. A historical no-change edit record did not authorize replay; current file contents were re-read before continuing.

Five development regressions authored, **0 executed / results not acquired**:

1. `resized_and_restored_width_rejects_old_layout_authority`
2. `scale_change_and_remount_cannot_adopt_previous_geometry`
3. `only_actual_finite_positive_geometry_for_current_width_is_accepted`
4. `fades_only_show_actual_hidden_content_and_respect_short_viewports`
5. `production_fade_geometry_keeps_scrollbar_gutter_and_no_input_actions` — mounts the actual production NSH renderer with ApproxTextEngine, checking short-viewport frames/gutter and zero event actions; not native/pixel evidence.

Parent should compile the exact final files, execute the above and necessary existing tool/group/anchor/Final regressions under its coordinated Cargo load, then obtain independent review and final-head CI. Keep the five Rust sources/test wiring for now; parent archives original source/hashes/results and removes them before master. This writer ran **no Cargo/rustc/check/build/Clippy/Rust tests/CI/App/Git/network/configuration/installation/delegation**. The user's closed seven-test timeline batch is untouched and is separate evidence, not validation of this delta.

Final Rust SHA-256 (relative to `crates/vega_ui/src/`):

| File | SHA-256 |
| --- | --- |
| `tool_card.rs` | `9deaafec47a36b78e044703ca8146499262ba3ffc9f7cc35f8fd3034580f8b86` |
| `tool_card/nsh_tool.rs` | `614922df9c29ea6767f38d1bcd1969ad956801cca799b0f178c2397203303529` |
| `tool_card/nsh_tool/layout.rs` | `17d8fe467197781be1ad8bd6831d0501e34cb9f050cd0af2310d9a7fbd2d5e66` |
| `tool_card/nsh_tool/layout/tests.rs` | `1ce9998b392afb791fef78b0541736df0848ddb2fd7be8f0f92f0ab79bffb83c` |
| `tool_activity_group.rs` | `91c9864f956791069b259ac911da37d9566cfd964a6e5811234ab2d8e6db1910` |
| `tool_activity_group/nsh_scroll.rs` | `1b7068cd5228c083c5e864847b513ce4c01534fe3be71f16e2ea74937cbc6389` |
| `tool_activity_group/nsh_scroll/tests.rs` | `be8168f828d1a5d3caed4cd0dff82262dfd135178d9782d8c77ec338e2286323` |

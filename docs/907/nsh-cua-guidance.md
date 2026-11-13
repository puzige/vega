# #907 NSH CUA GuidanceCard

## Scope and implementation contract

The complete migration is authorized by `nsh-full.md`; this independent leaf owns only `cua_onboarding/card.rs`, `card/view.rs`, new `card/*`, and this document. Baseline HEAD supplied by the integrator is `9b9541eb88274e1020ca024a2d45fbb80a72720e`. Upstream fetch/update was completed by main. Other live writers and previously completed leaves are outside scope. No Git operations, builds, app execution, installation, package/release, dependency changes, or subagents. Main owns compilation/tests/CI.

Before implementation, acceptance is fixed as follows:

- All overview and guide text, controls, warnings, statuses, app icons, card background/border/shadow, focus indications and drag-preview pixels are NSH 0.4 Rust content. GPUI remains only the native window/event host, Surface mount, and invisible external-drag hit region. No old visible GPUI view is embedded.
- Preserve public GuidanceCard APIs and existing CardController, PreparationModel, observation/check-ticket and return-to-task authority. No permission success is inferred from a click, command receipt, opening Settings or drag completion.
- Preserve both permission rows and actual checking/retained/unknown/granted/denied states; feedback, screen recording restart notice, has-task Return, Close, Back, Finder fallback, diagnostics, geometry/path/waiting faults and retry/open Settings paths remain available where applicable.
- Preserve native window creation, activation/floating/transparency/spaces, placement, stage resizing, 150ms follow/gesture-lifetime behavior and close cleanup. Mount/reload itself performs no new controller actions or permission checks.
- Every NSH business action carries the observed Surface identity, stage identity, stage/pane and action authority. The owner rechecks actual openness, stage/route and action availability. Stale callbacks from a previous stage or Surface cannot act. Recheck rechecks current shared availability, not only the observed status. Close/recovery remain independent of another action's pending receipt.
- External drag retains `on_drag` followed by `external_drag_payload`, original source lease and native Files export. The invisible region exists only over the NSH app target in an active, placed, fault-free Guide. Begin and export revalidate Surface/stage/pane, current drag availability and existing current app path. No arbitrary URI/file path is supplied by NSH. Gesture lifetime still suspends/resumes follow using the original controller.
- Encode the existing bundled SVG once into a safe data source, shared by snapshots and drag preview. No Entity or Rc/native lifetime lease crosses into NSH snapshots; no per-frame file reads/encoding or new unsafe code.
- P1 keyboard repair acceptance (fixed before implementation): pinned NSH 0.4 ordinary View controls are not keyboard stops and ElementRef::focus only focuses Input. Replace that ineffective path with a paint-free GPUI native keyboard owner. Tab/Shift-Tab traverse only enabled displayed controls in visual reading order, wrap within the card, and Enter/Space activate the exact selected observed action through the existing authority checks. Escape remains the native Close path. Initial Overview selects Close; Guide selects Back; recovery selects Close. Stage/Surface/recovery replacement resets selection; removed or disabled controls cannot activate. No timer/task/layout callback may steal native focus later.
- Pointer activation synchronizes the selection and native keyboard owner, including the external-drag overlay, without changing native drag export/lifetime. NSH paints the selected focus ring only while the host actually owns focus. Preserve accessible button names/roles/disabled state and report the selected button through Fast's supported active-descendant accessibility bridge. No new dependency, backend fork, shared-file edit or GPUI product pixels.
- Focus regressions must cover actual displayed order for Overview/Guide/fault/recovery, disabled skip and wrapping, initial/stage/recovery selection, stale selection rejection and observed request rejection. These development tests remain unrun by this writer and require archive/removal before merge.
- Theme tokens and existing i18n keys remain authoritative. Keep existing card dimensions, overview scroll and compact Guide; no unrelated redesign or explanatory product copy.

## Architecture

A card-owned `Presentation` holds a `Surface<Snapshot, Request>`, unique projection identity, focus bridge and independent recovery Surface. Main-thread projection copies nonsecret view/feedback/fault data only. NSH callbacks send observed actions through the bounded Surface channel, await owner receipt, and show only failure feedback when needed. A separate minimal NSH recovery renderer retains safe controls and reload without initiating permission operations automatically.

The native drag overlay is transparent, non-focusable and paint-free. Its bounds follow the NSH app target's observed layout, validated as finite and inside the current card. It receives the original native drag gesture, not a replacement click or reveal-only implementation. NSH remains the keyboard-accessible app control; a native overlay click routes through the same fenced owner action handler. Preview itself mounts an NSH Surface and retains the Rc gesture lease only on its GPUI owner.

## Validation plan and evidence limits

Scoped rustfmt and whitespace/static API review are allowed here. Main must run coordinated compile/Clippy and any development regressions; no checks are claimed until run. Native external drag, keyboard/focus, overview scroll, compact layout, both themes/languages, Settings geometry/follow/spaces and permission failure/recovery require real runtime handtest. Synthetic input is not authoritative for the native focus chain. Any development Rust tests and exclusive wiring must be archived and removed before merge under repository policy.

## Implementation and integration handoff

Implemented the complete visible Overview/Guide in `card/nsh_view.rs`; `card/view.rs` now contains only paint-free native mounts and the external-drag host. No shared wiring or public caller changes are needed. Existing `GuidanceCard::{close,is_open,seek_settings,show_overview}` and `open` signatures are unchanged. Main may integrate these files directly after independent review and coordinated checks.

- `card.rs` adds the leaf module, card-owned presentation, once-encoded bundled SVG `ImageSrc`, shared-model observation generation, and drag-geometry invalidation on stage transitions. Window configuration, native placement/follow, effects, cancellation, check-ticket owner and lifecycle remain the original implementation.
- Each control captures its displayed Snapshot, rather than borrowing a newer snapshot when clicked. Owner-side policy rechecks projection identity, stage identity, pane, openness and current/observed action availability. Recheck additionally fences the observed shared-model generation and current CardView, preventing an old ready-state callback from starting another completed check cycle.
- The invisible native host follows the NSH app target's root-coordinate measurement, intersected with its measured scroll viewport; finite, in-card, below-header bounds are required. Layout and scroll update measurement; stage changes clear cached bounds. Begin/export check current stage/pane/Surface and actual current-path existence; native Files payload remains GPUI-only. Rc gesture lease stays exclusively on the native DragPreview owner, whose pixels are a separate NSH Surface. The immutable controller still owns the current bundle path; NSH does not choose or transport a path.
- The Surface background is made transparent using pinned NSH's public per-AppView host Theme with `background = AdaptiveColor::TRANSPARENT`; no shared Surface change. Card pixels/shadow are NSH. Existing icon source support is used for functional SVGs, and the original bundled app icon is encoded once in construction.
- A separate NSH recovery Surface retains Close, available Recheck, diagnostics, has-task Return, Guide Back and explicit reload. Reload only recreates presentation. No mount/reload controller actions, permission-success messages or implicit checks were added. Escape retains native Close. The initial handoff incorrectly assumed ordinary NSH View controls participated in native Tab/activation; the P1 followup below supersedes that claim. Controls retain Button role, accessible labels and enabled state.

## Main integrated validation (supersedes writer-only unrun notes)

Main compiled the full workspace and ran strict all-target Clippy and fmt on the final P2 content above, including latest master `34555cec9c0c4c4c1f7a528d739752f414e53edf` merged as `692c19c44d410614dd091abdec9e276eb4253fb5`. All succeeded. Workspace Nextest run `f0af3e7a-4bcf-4c00-a53f-8a4f9ceb39a5` discovered/executed/passed 72, skipped 0, including all thirteen CUA regressions; test build30.60s, execution0.356s. Toolchain rustc1.98.0, Cargo1.101.0/fork0.102.0-9, Nextest0.9.151; fixed environment `RUSTUP_TOOLCHAIN=1.98.0 CARGO_FORK_AUTO_UPDATE=0 TMPDIR=/tmp`.

Non-implementer static review closed the ordinary-view keyboard P1 and subsequent offscreen-control P2 at the supplied final P2 hashes, without additional concrete findings. It checked actual pinned measurement/scroll/AX APIs and unchanged native drag path. Static review and policy tests do not prove native keyboard/VoiceOver/overflow/manual-scroll/drag behavior. No application execution, packaging, install, master merge or release occurred. Exact-head cloud validation remains pending; the complete-product migration is not finished.

## Initial implementation static checks (historical, superseded for P1 files)

- `rustfmt --edition 2024 --config skip_children=true --check` on exactly the four Rust files listed below: exit 0 after final formatting.
- Scoped Python whitespace check of all owned Rust files: no trailing whitespace; final newline present. This is a whitespace/diff-equivalent check, not `git diff --check`: no Git command was run per allocation.
- Scoped static policy scan: every `Layout`/`Typography` reference resolves in the actual theme source; no unwrap/expect calls or comment openers; no visible GPUI image/text/icon children or background/border/text/shadow styling; original `on_drag` and `external_drag_payload` present; no native-host Tab interception. Exit 0.
- First rustfmt attempt found a malformed new permission-row delimiter, corrected before passing. First policy scanner incorrectly classified multiplication/dereference `*` as comments; the scanner was corrected to recognize comment openers and rerun, without weakening production constraints.
- Actual pinned registry inspected: nsh-core/base/platform-gpui 0.4.0, GPUI Fast 0.1.9, Surface renderer/command bridge, NSH ImageSrc safe SVG encoding, ElementRef measurement/layout/scroll/focus, accessibility/press/disabled, AppView host theme and native drag API ordering/lifetime.

## Initial implementation validation inventory (historical)

No compilation, Clippy, tests, CI, app execution or native interaction was run by this writer. Main owns coordinated validation. Five development regressions in `card/nsh_view/tests.rs` exercise production policy: replacement Surface/stage/pane/closed rejection, completed check-cycle stale recheck rejection, independent recovery routes, local-fault drag/recovery policy, and invalid/overlapping native geometry. They are present but **unrun**, not passed.

Before merge archive/remove `card/nsh_view/tests.rs` and its exclusive `#[cfg(test)] mod tests;` declaration in `card/nsh_view.rs`; no test dependency or other fixture/wiring was added. Preserve production policy methods. Main should compile/Clippy this leaf, run the five development regressions before cleanup, independently review, and validate the final test-free integrated head through required CI.

Native external drag to System Settings is an observation-route contract requiring runtime handtest, not proven by API/source inspection. Also unverified: drag preview visibility and source lease teardown, actual NSH focus restoration/Tab/Shift-Tab/Enter/Space/Escape, hover/focus through the native overlay, bilingual compact layout and warning scroll, light/dark transparency/shadow, path removal during gesture, stage change/close during gesture, Settings movement/spaces and permission failure/recheck/cleanup. No known API blocker or deliberate lossy fallback remains; these are evidence gaps, not claims of runtime success. Platform-native preparation support is unchanged.

## P1 keyboard/focus repair implementation and integration

Pinned source evidence: `nsh-platform-gpui-0.4.0/src/input.rs::collect_stops` admits only Input/Native; `text_input.rs::focus` resolves only Input. Ordinary View `focusable/on_press` is not a keyboard implementation. The old assumption and no-Tab-interception scan above are historical, not current guarantees. Fast 0.1.9 `StatefulInteractiveElement::{aria_active_descendant,on_a11y_action}`, `window/a11y.rs::set_active_descendant`, `FocusHandle::is_focused`, `canvas`, and paint-phase `Window::on_mouse_event` provide the supported replacement without dependencies or backend/shared edits.

- New `card/nsh_view/keyboard.rs` lists the currently displayed control identities, labels and reading order. Overview: Close, Accessibility, Screen Recording, Recheck, optional diagnostics, optional Return. Guide: Back, Close, app/Finder when shown, fault-specific Settings/Finder when shown, diagnostics when shown. Recovery: Close, Recheck, diagnostics, Reload, optional Back, optional Return. Recheck remains displayed but is skipped while disabled; optional/hidden controls never enter traversal. Tab/Shift-Tab wrap the enabled list, not a small shortcut subset.
- Existing `GuidanceCard::focus` is the real native owner, tracked by the paint-free root. Initial/stage/reload focus remains synchronous on original lifecycle paths; recovery synchronously focuses the host. Overview/recovery select Close and Guide selects Back. Stage transition explicitly clears old selection and pending pointer state. No NSH `target.focus`, Surface focus redirect, focus task, deferred focus or layout focus request remains. Focus/blur subscriptions only invalidate rendering.
- Native Enter/Space dispatch the selected observed Snapshot through the exact same `nsh_action` acceptance and controller path as NSH press/native overlay click. Held activation keys do not repeat. Modifier shortcuts are not substituted for traversal. Before activation, current displayed membership, enabled state, Surface/recovery/stage identity/pane, openness and Recheck revision are checked. A stale selection cannot become a new-stage action. Escape remains the original native Close dispatch.
- The paint-free canvas registers native pointer listeners without adding a hitbox or consuming/replaying events. Left-down clears old selection; left-up inside the host synchronously restores native owner focus before the NSH press receipt. Accepted mouse actions update selection only at the observed selection revision, so later Tab/stage/recovery changes cannot be overwritten by delayed receipts. Receipts never request native focus. The existing native drag region, drag/export callbacks, path checks and gesture lease are unchanged.
- NSH Snapshot carries selected identity and actual `FocusHandle::is_focused` state; NSH alone paints the selected accent border. Focus-only snapshot changes do not reconstruct the displayed NSH subtree during a pressed gesture. The callback retains its original business authority and captures only the current selection revision when pressed.
- The native focused root has Group role. A paint-free, non-focusable, non-hitbox Button active-descendant proxy names the selected enabled control and supports fenced assistive Click. Fast only reports this descendant while the real focused ancestor exists. Original NSH buttons retain their labels, roles, disabled state and assistive activation for the full card. No GPUI product text/image/background/focus pixels were added. Native focus ownership and accessibility use public Fast APIs, not merely logical selection.

No main/shared integration changes are needed. Public GuidanceCard APIs and controller/model/native modules remain unchanged. Compile/Clippy the leaf, run the eleven development regressions, independently review, then archive/remove `card/nsh_view/tests.rs` and its exclusive two-line cfg/module declaration before final test-free head CI. The new `keyboard.rs` is production, not removable test wiring.

### P1 executed checks and evidence limits

- `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`: scoped format and `rustfmt --edition 2024 --config skip_children=true --check` on all five owned Rust files succeeded (exit 0).
- Scoped whitespace/policy check succeeded: final newlines, no trailing whitespace, no production unwrap/expect or code comments, paint-free GPUI host/proxy, native `on_drag`/`external_drag_payload` retained, no obsolete target/Surface focus calls. No Git command was run.
- Six new development tests (eleven total) cover displayed Overview/Guide/fault/recovery order, disabled skip, forward/reverse wrap, initial/stage/recovery reset, stale Surface/stage/completed-check selection, and removed mouse-selected diagnostics/Return. Added but **unrun** here. They test production selection/authority policy, not source strings or fabricated native input.
- No builds, Clippy, tests, CI, app execution or runtime accessibility/keyboard/drag checks were run in this allocation. Main's earlier Clippy and 62/62 Nextest results cover the old version only and are not evidence for this P1 code. Runtime handtest must cover Tab/Shift-Tab/Enter/Space, actual focus/VoiceOver selected names, mouse-to-keyboard behavior, stage/recovery transitions, compact/scrolling layout and drag/export lifetime. Source review establishes API support, not native runtime success. No known concrete API blocker remains; compilation and runtime evidence are pending main/user validation.

## P2 observed keyboard selection reveal acceptance (before implementation)

- Tab/Shift-Tab selection of a control inside the Overview, Guide body or recovery scroll viewport must reveal its visible name and NSH focus ring with the smallest necessary vertical scroll. Already visible controls and controls outside that scroll viewport (Overview Close, Guide Back/Close) must not move it.
- Use pinned NSH 0.4 ElementRef measurement/scroll commands and actual ancestor content offsets; never treat parent-relative frames as page coordinates. Scroll only for a new keyboard-selected revision, not ordinary status/theme/focus snapshots or pointer activation. Initial/reset reveal is optional; their existing visible Close/Back targets remain unchanged.
- Revalidate Surface/stage/pane/recovery identity, enabled displayed action, actual host focus, selected action and selection revision after asynchronous measurement. A replaced selection or route, disabled/removed control, pointer transition or stale geometry must not scroll the current page. No asynchronous native focus request is allowed.
- Preserve the existing Guide drag target measurement, viewport clipping, scroll-triggered bounds refresh and native drag/lifetime authority. No controller/model/native behavior, accessibility semantics or product pixels move to GPUI.
- Development regressions cover visible/above/below/oversized control geometry, nonzero ancestor offsets, invalid geometry, and current-selection/stale/disabled reveal policy. Main owns compilation/tests; prior strict workspace Clippy/fmt and 70/70 Nextest (11 CUA) are supplied evidence for P1, not this P2 revision. Native overflow keyboard reveal/drag handtests remain required.

## P2 implementation and handoff

- Added production `card/nsh_view/reveal.rs`. Each scrollable control registers its ElementRef with the current Overview/recovery viewport or the existing Guide drag viewport. Overview Close and Guide Back/Close remain outside registration. `watch_scroll` retains the actual ancestor offset/content extent without replacing the original Guide `on_scroll` callback.
- Tab/Shift-Tab marks `reveal_revision = selection_revision`; pointer-down, accepted press and stage reset clear it. A page-owned attempted-revision signal survives ordinary content rebuilds, so status/theme/focus updates do not repeat the scroll. Initial/reset Close/Back already sit outside overflowing content and intentionally do not request reveal.
- Both control and viewport measurements are issued together. Geometry uses `Measurement.page` (ancestor scroll offsets already applied) plus the watched content offset, preserves horizontal position, and moves vertically only by the missing distance. Oversized targets align their leading edge; destinations clamp to content bounds. Nonfinite/empty geometry is rejected.
- Before scrolling, a non-business `Request::Reveal` asks the GPUI owner to verify actual host focus and the current Surface/stage/pane/recovery, complete authority, displayed/enabled action, selected identity and exact revision. The NSH task rechecks its current snapshot and unchanged scroll observation after the response. Tasks are scoped to the rendered subtree owner and cancel on replacement. No delayed native-focus call or controller operation is introduced.
- Existing drag clipping/measurement code and native host/export/lifetime paths are unchanged. Pinned NSH 0.4 `scroll::command` uses `moved_to` plus `report` for a nonanimated ScrollTo; the existing Guide scroll listener remains attached to refresh measured/clipped drag bounds. Native runtime behavior is not claimed from source inspection.

### P2 validation and integration

- No shared integration or caller changes required. `card.rs` and `card/view.rs` hashes remain exactly the P1 supplied hashes. Controller/model/native files, shared nsh.rs, dependencies, Git/index and other scopes were not written.
- Scoped `rustfmt --edition 2024 --config skip_children=true --check` passed for all six CUA Rust files using rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18). Final newline/trailing-whitespace, production no-comment/no-unwrap/expect, unchanged native host hashes/export hooks and no reveal focus calls checks passed. The first policy scan falsely classified an unchanged multiplication continuation as a comment; narrowed comment-token matching corrected the scanner, not product source.
- Added two meaningful development regressions (thirteen CUA tests total): measured page geometry with nonzero offset, already visible/above/below/oversized controls, clamping and invalid bounds; and live keyboard revision/focus/selection/route/disabled/removed-control authority. These tests are **unrun in this allocation**. No builds, Clippy, tests, CI or app operations were run. Main's supplied strict workspace Clippy/fmt and Nextest 70/70 cover P1 only, not P2.
- Main must compile/Clippy and run the thirteen CUA tests, obtain independent review, and handtest overflow Overview/Guide/recovery Tab/Shift-Tab reveal, visible name/ring before activation, manual scrolling without snapshot snapback, focus loss, rapid stage/pointer/selection changes, and Guide drag hit-region clipping/export. No concrete API blocker is known; compile and native evidence remain pending.
- Before merge archive/remove `card/nsh_view/tests.rs` and its exclusive cfg/module declaration. `keyboard.rs` and `reveal.rs` are production and must remain. Preserve the P1 review conclusions; this is only the P2 scroll-reveal delta.

## P2 code content SHA-256 at handoff

```text
4f3d8ad079011075d8c967b896aa37b0f0cd07eaa4210330d7a3930f46a974fb  crates/vega_ui/src/cua_onboarding/card/nsh_view.rs
9b2c9f94940346cfd2620d1ebbd4b7ee18a57fdf114b6ceb323f957de6dc1314  crates/vega_ui/src/cua_onboarding/card/nsh_view/keyboard.rs
3b40e8de8ee2733a1c779bfbbb65d1bb247c296580be07c13c4157ddc4422820  crates/vega_ui/src/cua_onboarding/card/nsh_view/reveal.rs
697b705d60b03744234d08393e6c14dcfdc2d859eb0550d4f6657f73612364af  crates/vega_ui/src/cua_onboarding/card/nsh_view/tests.rs
```

## P1 code content SHA-256 at handoff (historical)

```text
ac48f8562d2a6c160bfdea9a9fff3d588b3788f4c6e497025a8b7e25fb82be62  crates/vega_ui/src/cua_onboarding/card.rs
51a26231281dc78a867bfa99c72a706e2122a2b8f154acaabacd633fe3e198f3  crates/vega_ui/src/cua_onboarding/card/view.rs
33903914762780d1603030b11f336a10eaa9bd1ac17aa27058551921988cb2bd  crates/vega_ui/src/cua_onboarding/card/nsh_view.rs
5202ccc3377294839d941b02aa5164b84bf886c07f32b82f5f047fa75791bd01  crates/vega_ui/src/cua_onboarding/card/nsh_view/keyboard.rs
92e081d993843cbb37836e636c91cf0e13b12fb5f5edcac24c9a6ae62b2dd291  crates/vega_ui/src/cua_onboarding/card/nsh_view/tests.rs
```

The document's own hash is reported externally to avoid a self-referential checksum. No shared NSH/icon/controller/model/native source, unrelated scope, manifests/lock, or Git/index was written. Writes stop after the final document/hash verification.

# Nested-wheel independent regression author lane

## Scope and acceptance

This is regression authorship, not independent review. User-approved scope is nested-wheel composition only; source-anchor regressions belong to the body integration lane. Baseline HEAD is `0b5ea426abfd8d4358489e916273d89474fb190f`; parent supplied dirty-content identity is `46316929a85a498cf97d2089476c8d09c0fe4dce3ac5ce248c7947893beb443b` and reports the remote refreshed. This lane does not refresh or mutate Git.

The new `crates/vega_ui/src/conversation_stream/scroll_composition_integration.rs` must exercise real main-thread GPUI window dispatch, two actual production reading-body Surfaces and one original scrollable GPUI ListState. No test helper implements consumption, remainder routing, gesture ownership or glide physics. Expected movement is measured from actual native inner offsets and the original ListState pixel offset, not from mirrored algorithms, text estimates or source-string assertions.

Acceptance includes: no-target forwarding; full inner consumption; partial forward/backward exact-once remainder; independent mixed-axis consumption; top/bottom transfer; Started/Moved/Ended/Cancelled; gesture retention when the pointer moves onto the neighboring Surface; release after cancellation; and no second glide after terminal edge transfer. Exact movement comparisons use a fixed half-logical-pixel rounding tolerance, much smaller than every asserted wheel/remainder displacement.

## Parent wiring contract, not an existing production API claim

The existing body fixture is private and is not an exported fixture constructor. At inspection, AppView exposes reading snapshots/geometry and integration scrollbar alpha, but not a public exact native scroll offset/max-offset observer or general native scroll-to actuator. The new shared fixture therefore accepts `BodyFactory` and defines `BodyMount` as a development-only adapter contract. These are newly authored fixture types, not claimed existing backend symbols.

The parent must include/re-export this module only under the existing `nsh-reading-integration` development feature and register each public case in the existing main-thread, real-font runner. Every case takes `(&mut TestAppContext, BodyFactory)`; runner wrappers supply the real factory. Do not add a second runtime, dependencies, concurrency overrides, ignored cases, or longer budgets.

Implement the factory where the real body internals are accessible. It receives the body kind, distinct slot index, the exact original ListState, Window and App. Each call must create a separate production LinkedBody/BodyState/Surface binding with real current reading owner/root/source authority. Connect the production remainder route to the supplied ListState; never replay the original event or implement routing in the adapter. Retain production request tasks and ownership for the full mount lifetime.

- `BodyKind::NoTarget`: short real production body, no inner scroll extent on either axis. Wheel input still hits that Surface.
- `BodyKind::Vertical`: actual vertically overflowing production Thinking body; native vertical extent at least 512 logical pixels, no horizontal extent.
- `BodyKind::Horizontal`: actual production body containing a horizontally overflowing code/details scroll target; horizontal extent at least 256 logical pixels, no vertical extent at the tested target.
- `BodyMount.render`: render the production body into the provided fixed-height row, preserving child-prepaint/currentness fences. No fake Surface or event interception in the fixture adapter.
- `BodyMount.observe`: return None until the actual mounted target is current, live, visible, active and has current finite geometry. Then return its actual AppView, clipped scroll-target viewport, positive native offset, native maximum offset, and current Surface error value. The viewport center must hit that scroll target (or the no-target Surface), not a scrollbar/native overlay. For horizontal content, observe that actual horizontal scroll node, not a different ancestor. Never synthesize offsets from requested wheel input, source length, or cached desired state.
- `BodyMount.scroll_to`: position only the actual observed native scroll target, without animation, for setup. Observe application before dispatch; preserve the other axis. Use real published production APIs, or provide an authorized minimal development adapter in the owning lane; do not invent backend symbols.
- `BodyMount.stop`: stop the actual Surface and release retained production bindings/tasks. The fixture invokes this on normal completion and unwinding.

If these seams are unavailable, the cases remain authored but blocked from execution. A constant/fake observer, factory that implements scroll decisions, or a passing empty test does not close the gap. No wiring, runner, Cargo, vendor, production body or existing test changes are authorized for this author lane.

## Validation and delivery boundary

Retain the existing 2-second/512-tick scheduler drain and 10-second wait deadlines. Timed observation advances the existing executor clock and draws real frames without changing default concurrency. The final parent cycle must compile/typecheck the adapter and new file and execute every registered case; zero executed cases is not a pass. Earlier 339 library / 36 host / 12 geometry / 5 mounted / 4 shell and 370 nextest results are historical parent evidence, not validation of this source.

This lane may run only scoped rustfmt and read-only source-hygiene/hash checks. No Cargo, rustc, tests, Clippy, network, Git mutations, app execution, dependencies or delegation. Independent consolidated review occurs after implementation lanes finish, not in this author lane. All Rust cases, this exclusive fixture, adapter/export/runner wiring and other exclusive development support must be archived with exact hashes/results and removed before master under repository policy.

## Production-body adapter implementation contract

The adapter lane owns only the new `selection_menu/body_scroll_integration.rs`, necessary composition-fixture wiring, and this document. It does not edit production body/source-anchor files, backend/vendor, runner, exports, Cargo, or Git. The parent reports fetch/update complete; this lane does not repeat network/Git mutations. Four implementation lanes remain separate; no independent review starts in this lane.

The fixed `BodyFactory` signature is retained. A mandatory development-only `NativeScrollAccess` global supplies three parent-owned operations: observe a current native target, synchronously position that exact observed target, and wrap a real body with the production original-ListState route. There is no default, simulated observer, adapter wheel handler, or fallback consumption. The observer receives the real AppView, reading area/geometry, body kind, optional production vertical node, and current clipped host viewport. Its returned target identity must identify the actual hit-tested scroll node (None only for no-target), with exact positive native offset/max and window-logical clipped viewport. Setup receives that complete current observation and desired positive offset; it must reject stale identity/geometry and preserve the unmodified axis. The parent must implement these seams in its authorized backend lane before the adapter can run.

Each mount creates a real ConversationStream and ThinkingBlock through production events, reveals/rebinds its production group, and renders `ThinkingBlock::render_stage_source`, retaining the actual source, reading owner, body Presence, LinkedBody, Surface, lease and production request lifecycle. Both streams use the same thread route but distinct source/card/Surface identities and share the original ListState. Root authority checks actual membership in the composition root rather than merely window activity. Rendering clips the real production body to its fixed fixture row; no production height is falsified. Observation first validates production owner/content/lease/reading geometry, then calls the required native seam; requested setup never becomes observed state. Stop is idempotent, stops the actual Surface, invalidates its binding and drops retained mount ownership.

Adapter success requires parent inclusion as a feature-gated child of `body.rs`, installing real `NativeScrollAccess` functions, runner wrappers supplying `body_scroll_integration::factory`, final consolidated compilation, and all eleven executed cases with unchanged assertions/budgets/tolerance. Static formatting alone does not satisfy these gates.

## Original regression-author stopped handoff (historical)

Eleven regression entry points authored, **0 compiled / 0 executed**:

1. `no_target_surface_forwards_wheel_once_to_original_list`
2. `inner_surface_consumes_wheel_until_actual_edge`
3. `partial_forward_remainder_reaches_original_list_exactly_once`
4. `partial_reverse_remainder_reaches_original_list_exactly_once`
5. `mixed_axes_consume_horizontal_inner_and_forward_vertical_remainder`
6. `mixed_axes_partial_inner_remainder_does_not_replay_vertical_delta`
7. `actual_top_and_bottom_edges_transfer_only_outward_delta`
8. `gesture_owner_survives_pointer_move_until_ended`
9. `terminal_phase_delta_reaches_original_list_once` — checks nonzero Started, Moved and terminal deltas for both Ended and Cancelled.
10. `cancelled_gesture_releases_owner_without_residual_glide`
11. `ended_edge_transfer_cannot_start_inner_and_outer_glides`

The glide observations advance the full requested executor duration (normally 1100 ms), checking actual offsets after every frame, with the unchanged 10-second wall deadline. They do not exit early merely because drawing consumed wall time. The fixture defines no Rust test attribute: these are main-thread runner cases, not nextest-discovered tests until parent wiring. Main-thread actual fonts and active-window authority are mandatory.

Executed checks: explicit `/Users/puzige/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/rustfmt --edition 2024 --config skip_children=true` and `--check` on the new Rust file only, both successful; formatter reports `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`. Read-only Python checks passed UTF-8/final-newline/trailing-whitespace/conflict-marker hygiene, absence of comments/lint allows/ignored or should-panic cases, eleven distinct public case names, and unchanged 2s/512-tick/10s budgets. Cached pinned gpui-fast 0.1.9 source and existing production callsites were inspected for ListState offset observation/positioning, Entity equality and window dispatch. These are static checks, not typechecking, behavioral results or review.

Rust file SHA-256: `29b5f81b5284015c8734cc047660145f4c49be482dfa206daf989ba3b3653ea9`.

Only this document and the new Rust file were written. No existing modules, exports, runner, manifest, lockfile, vendor, body, budgets, concurrency or security/currentness fences were modified. Parent action: implement the real adapter and production remainder-route binding, feature-gate/include/re-export the module, register all eleven wrappers in the existing runner, then compile and run the final consolidated version. Native acceptance and independent consolidated review remain outstanding. Original regression-author writes stopped after its final read-only hash verification. The adapter lane below subsequently changed the composition fixture and added the factory; the preceding hash and two-file scope describe that earlier stage only.

## Production-body adapter stopped handoff

Authored on the supplied worktree/branch with baseline HEAD `0b5ea426abfd8d4358489e916273d89474fb190f`. This is implementation authorship, not independent review. This lane changed exactly the adapter, the composition fixture, and this document; it did not modify production body/source-anchor modules, vendor/backend, exports, runner, manifests, lockfile, Git, or machine configuration. Three other implementation lanes were reported active by the parent; this execution has no spawning/status interface and did not independently verify their completion or start review.

The factory now constructs short prose, 80 short paragraphs, or a single long fenced code line through real production ThinkingDelta events. Extents are not inferred from those inputs: the eleven cases still require actual measured >=512 vertical or >=256 horizontal native maxima. Distinct mount ownership retains the stream/card/BodyState and the original ListState, and normal/unwind fixture cleanup calls the idempotent stop. Fixture changes are limited to weak stream membership for root authority and clipping each existing fixed-height body row. Existing case bodies, movement assertions, 0.5-pixel tolerance and 2s/512-tick/10s budgets are unchanged.

Current Rust SHA-256 values:

- `crates/vega_ui/src/conversation_stream/selection_menu/body_scroll_integration.rs`: `6c485f955e54f42368fc662824b12200f8db062e88769606f4981f1ff0f7cebe`
- `crates/vega_ui/src/conversation_stream/scroll_composition_integration.rs`: `4275278d5871569dd647c3e7bcbe83b2cbb58cb56df7aa8faa15518a9757d0c8`

Executed in this adapter cycle: explicit `/Users/puzige/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/rustfmt --edition 2024 --config skip_children=true` followed by `--check` on those two Rust files, successful, with `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`. Read-only Python UTF-8/final-newline/trailing-whitespace/conflict-marker/comment/lint-allow/ignore/should-panic hygiene, eleven distinct public case entry points, and fixed-budget/tolerance checks succeeded. These are static checks only: **0 compiled / 0 executed in this lane**, no Cargo/rustc/Clippy/app/network execution, no independent review. Earlier green parent suites do not validate this adapter.

### Exact remaining parent wiring

1. In the parent-owned `body.rs`, under `nsh-reading-integration`, declare a child module using `#[path = "body_scroll_integration.rs"] pub(crate) mod body_scroll_integration;`. Include/export the existing composition module as needed and make its eleven cases available to the existing main-thread runner. All types remain development-only.
2. Install `NativeScrollAccess` in the App before either factory call. Required signatures, as authored in the adapter:
   - `observe: fn(&NativeScrollRequest, &Window, &App) -> Option<NativeScrollObservation>`
   - `position: fn(&NativeScrollRequest, &NativeScrollObservation, Point<Pixels>, &mut Window, &mut App) -> bool`
   - `route: fn(ListState, Entity<AppView>, AnyElement, &mut Window, &mut App) -> AnyElement`
   No implementation or default is supplied for these mandatory operations. Missing registration fails explicitly at factory creation, not as a passing empty case. A sibling adapter in the same crate can fill these function pointers; do not make private production body state public merely for this wiring.
3. `NativeScrollRequest` carries the actual Surface AppView, current reading area/geometry, kind, production vertical node and clipped host viewport in window logical pixels. `NativeScrollObservation` carries the real target node (None only for no-target), clipped hit-test viewport in the same coordinates, and exact positive native offset/max. Native setup must revalidate the current geometry, attachment and target identity, position that same node without animation, preserve an unchanged axis, and return true when the target already equals the desired offset as well as when setup succeeds. The cases independently observe application after drawing; setup success cannot stand in for observation.
4. The backend seam is specifically absent at the inspected public boundary: `AppView::integration_scroll_alpha` exposes only alpha; `ReadingGeometry` has private scroll stamps, not offset/max access; `AppView::scroll_reading_anchor` is vertical-only incremental and rejects zero/no-op, so cannot implement this two-axis setup contract. The authorized backend owner can obtain real `ScrollState.x/y` and `ScrollState::max(frame.size)` from the attached `NshView`, use the native hit-test/transform/clip chain to select the actual horizontal child (not the Thinking ancestor) and verify its viewport center, then use the existing core scroll/report path (`scroll.rs::scroll_by` / shared scroll host) with appropriate currentness and idle guards. Read-only observation must not create a gesture or alter scrolling. No offset may be reconstructed from text length, requested position, source estimates, wheel deltas, or scrollbar alpha.
5. The route wrapper must bind both AppViews to the same production route for the exact original ListState and preserve it across rerenders. The wheel lane currently authors `ScrollRoute::install(&self, &mut Window, callback(ScrollRemainder, &mut Window, &mut App))` and `AppView::set_scroll_route`; the inspected root export was not yet wired. An acceptable parent helper is `crate::nsh::scroll::with_list_route(list, child, window, cx) -> AnyElement` plus AppView binding inside the wrapper, or the supplied equivalent `route` function pointer. The adapter must not consume/replay wheel input, implement remainder arithmetic, own gestures or add physics.
6. Runner wrappers pass this `factory` to all eleven existing cases. The parent owns final compilation/typechecking and execution of every case after all implementation lanes finish. In particular validate actual horizontal-node selection, production body clipping with the real height cap, owner/source/list authority, task cleanup, exact-once remainder and terminal/no-second-fling behavior. Any failure must be fixed in its responsible layer without relaxing assertions, tolerances or deadlines. Only after remaining implementation is complete may the requested consolidated independent review start.

Adapter writes stop after this handoff and final read-only verification. Native observation/setup, production route implementation, module/runner wiring, compilation, eleven-case execution, and subsequent independent review remain outstanding; this lane does not claim the overall issue complete.

## Concrete eleven-case wiring implementation contract (current cycle)

This section supersedes the earlier absent-backend/injected-route and stopped-scope descriptions for this cycle. The user transferred exclusive authorship of the adapter and composition fixture and authorized `crates/vega_ui/src/nsh/scroll.rs`, the new `crates/vega_ui/tests/nsh_scroll_integration.rs`, and this acceptance document. Parent owns all other files, including body/source-anchor module declarations, public re-exports, Cargo targets and backend/vendor implementation. No Git/network, Cargo/rustc/Clippy/test/app execution, dependency/config changes, ordinary code comments, lint allows or independent review are authorized in this lane. The parent supplied the refreshed branch and original HEAD; unrelated dirty work remains untouched.

The delivered backend now exposes `AppView::integration_reading_scroll(expected, area, node, horizontal)` and `integration_position_reading_scroll(expected, area, node, desired)`. The adapter's concrete `install(&mut App)` must register these real native operations, with native target observation intersected with the current production host clip. There is no remaining parent-supplied observer, positioner or per-body route injection. Keep current source/owner/root/lease validity checks and observe actual post-setup state; never derive offsets or consumption from requested wheel/setup values.

Both production bodies must remain inside one persistent composition-root `ScrollScope`, wrapping the entire original GPUI List tree during prepaint and paint, not one scope per body. Remainders use the same `crate::nsh::scroll::consume_list` as production TurnList; the factory itself does not route/consume wheel input. Cancel the root scope when retiring fixture mounts. The real public factory must match the unchanged `BodyFactory` and remain accessible through parent-owned feature-gated conversation_stream exports.

The new harness-free runner uses the current real platform text system on its main thread and registers all eleven named entry points sequentially, passing the concrete production factory. It retains one TestAppContext for the entire run, with actual-font assertion, GPUI Kit initialization and concrete backend installation before the first case. This is required by the existing backend `rt::JOBS` OnceLock queue: only its first App owns the receiver, while later `rt::init` calls refresh the async handle but do not recreate the queue. A fresh App per case would therefore break real later-case commits. Stop on the first failure instead of reusing a failed context; report actual passed/failed/not-executed totals, fail the process for any failure, and never report an unexecuted case as passed. A successful parent acceptance run requires eleven passed and zero failed/not-executed/ignored. No new runtime, thread/concurrency override, fake backend or event handler is introduced.

All eleven existing case bodies and assertions, 0.5 logical-pixel tolerance, 2-second/512-tick scheduler drain and 10-second wait deadlines remain unchanged. Pre-edit whitespace-insensitive SHA-256 of the case-body suffix beginning `pub fn no_target_surface`: `7b30ba36a21511414150ef9fa745873d8555a5f2228ed4d38893fbbe91980ade`. Scoped rustfmt and read-only source/hygiene/hash checks are the only validation executed here; parent performs consolidated compilation and eleven-case execution after all implementation authors finish, before independent review.

## Concrete wiring stopped handoff

The adapter's concrete installer delegates directly to the delivered real backend and clips its real observation; its public factory is now compatible with the already-present parent-owned public re-export. No route callback remains in NativeScrollAccess or Mount. The composition fixture retains its one whole-tree scope and cancels it during cleanup. `nsh/scroll.rs` received scoped rustfmt only; its existing production remainder consumption logic is unchanged. The new runner invokes all eleven original cases through that concrete factory and retains the one real-platform-text TestAppContext required by the process queue.

Final Rust SHA-256 values:

| Path | SHA-256 |
| --- | --- |
| `crates/vega_ui/src/conversation_stream/selection_menu/body_scroll_integration.rs` | `98f908498a08cad48917371e5c936ee4c3017a23c1315d08f134ac4cda15ce30` |
| `crates/vega_ui/src/conversation_stream/scroll_composition_integration.rs` | `b7262e429711dfc9ab5bd4f5b578ea16ae57b6e6c1d88d0b73e848a72c2f6438` |
| `crates/vega_ui/src/nsh/scroll.rs` | `90b27e700a8f577fc1535fbfc2d30bbb92a5797c073ca57a96b87360cefee380` |
| `crates/vega_ui/tests/nsh_scroll_integration.rs` | `906d8b145d82eb444821944900bea158035c7e143220a3e821fa0ecfc6e303a0` |

Executed here: `/Users/puzige/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/rustfmt --edition 2024 --config skip_children=true` on the four authorized Rust files, followed by final four-file `--check`, successful. Formatter version is `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`. Read-only Python checks confirmed UTF-8/final newline/trailing whitespace/conflict-marker/comment/lint-allow/ignore/should-panic hygiene, no unimplemented/todo placeholders, eleven unique runner registrations in original order, one App context, concrete install/factory calls and accurate result counters. The full original case-body token hash and all four fixed budgets/tolerance literals are unchanged. These checks are source/format checks only, not behavioral tests or review. **This lane compiled 0 / executed 0 cases.** No Cargo/rustc/Clippy/Git/network/app execution or independent review occurred.

### Exact parent-owned wiring and next validation

Read at handoff: parent already declared the following module in `selection_menu/body.rs` (do not move the file directly under conversation_stream because it needs the private body parent):

```rust
#[cfg(feature = "nsh-reading-integration")]
#[path = "body_scroll_integration.rs"]
pub(crate) mod scroll_integration;
```

Parent already added these feature-gated public conversation_stream exports; this lane made the underlying factory public to match them:

```rust
#[cfg(feature = "nsh-reading-integration")]
pub mod scroll_composition_integration;
#[cfg(feature = "nsh-reading-integration")]
pub use selection_menu::body::scroll_integration::{
    factory as scroll_body_factory, install as install_scroll_body_access,
};
```

The runner imports from the existing public `vega_ui::conversation_stream`; no lib.rs root re-export is needed. The inspected Cargo manifest still needs this parent-owned target, without new dependencies or feature changes:

```toml
[[test]]
name = "nsh_scroll_integration"
harness = false
required-features = ["nsh-reading-integration"]
```

Parent must format its own export/manifest changes and compile/run the consolidated final files, e.g. `cargo test -p vega_ui --features nsh-reading-integration --test nsh_scroll_integration`. This command is not executed in this lane. Require the actual final footer `11 passed; 0 failed; 0 not executed; 0 ignored`; on failure retain the failing name and not-executed count, diagnose the responsible production/backend/fixture layer and rerun the complete fixed suite after correction. Keep original assertions/budgets and all required regressions. Native target hit testing, genuine three-kind measured extents, live body/root authority, route ownership and terminal/no-second-glide behavior remain runtime acceptance gates, not claims established by formatting. Four-lane implementation completion precedes any independent review. This lane stops writing after final read/hash verification; no outstanding observer/positioner/route injection remains.

## First executed failure: List viewport borrow reentry

### Current repair scope and acceptance

This is implementation failure diagnosis and a minimal fixture-composition repair, not review. Parent has added the Cargo runner and successfully compiled the consolidated source. The raw first execution is **0 passed / 1 failed / 10 not executed / 0 ignored**, not the earlier unexecuted state. User keeps this lane's four Rust files and this document exclusive; `selection_menu/mounted.rs`, production body/Thinking, backend/vendor, Cargo and other parent-owned files remain outside write authorization. No Cargo/rustc/Git/network/app execution is permitted here. The source-anchor scheduler failure belongs to the other active author and is not investigated or modified here.

Acceptance is unchanged: preserve the concrete production factory, both real stream/source/body/Surface identities, the original ListState, one whole-tree ScrollScope, all eleven case bodies/assertions and the fixed deadlines/tolerance. Restore the existing production ReadingList frame boundary around the entire shared List, with both real owners attached to the same actual prepaint geometry and the same frame token. Do not replace native observations, catch the panic as success, bypass currentness, duplicate wheel handlers or alter Fast. The fixture repair requires the additive parent-owned multi-owner ReadingList seam below; it is not considered compiled or executable until that exact dependency is supplied.

### Original evidence and responsible layer

Read directly from `target/nsh-nested-scroll-11.stderr`: lines 21–34 show the first named case, actual Surface repaint activity and `[nsh-height] ... old=0 new=28`; lines 36–38 report `gpui-fast-0.1.9/src/elements/list.rs:822:16: RefCell already mutably borrowed`, with no backtrace enabled. The stdout footer is `nested scroll integration: 0 passed; 1 failed; 10 not executed; 0 ignored`. Raw stderr SHA-256 is `e5a7e2e0e043f0604aab35408bb420426f98c4763ffd67d785e247bf5ba54f86`; stdout SHA-256 is `27d10be70151c21beaaf4344a974c69de2dfa1406375d66579da3e712dae6b07`.

The responsible layer is the new fixture's missing production composition boundary, not native scroll consumption or a demonstrated Fast defect. The concrete static reentry path in the compiled source is:

1. Runner `main` invokes `no_target_surface_forwards_wheel_once_to_original_list`, which enters `Fixture::new` → `wait` → `frame` → real `window.draw` before its first wheel dispatch.
2. Fast `List::prepaint` at lines 1619–1658 holds `self.state.0.borrow_mut()` across `state.prepaint_items`; its item layout/prepaint runs the fixture row callback and the real body subtree while that guard is held.
3. `FixtureRoot::render` builds the actual List row, calls `BodyMount.render` → `Mount::render` → `ThinkingBlock::render_stage_source` → `render_source` → `render_body` → real body `LinkedBody`. The factory assigns the identical original ListState to each real ConversationStream at adapter line 295.
4. The logged height update is in `thinking_body.rs::Presence::prepaint` lines 200–219, immediately before its child prepaint. `body.rs::LinkedBody::prepaint` lines 1931–1948 obtains its real owner and calls `mounted::list_bounds(...).unwrap_or_else(|| stream.list.viewport_bounds())` to fill the current lease geometry.
5. The old fixture wraps only ScrollScope, not ReadingList. Therefore `mounted::ListFrames` has no entry for either real owner and the fallback executes. Fast `ListState::viewport_bounds` at lines 821–822 calls `self.0.borrow()`, reentering the exact ListState still mutably borrowed by prepaint.
6. Even avoiding only this first fallback would be insufficient: `BodyState::valid` at body lines 1277–1309 and `ReadingOwner::current_for_frame` at selection_menu lines 113–122 depend on the same real ReadingList frame boundary. Product `render.rs` lines 699 onward already wraps the whole TurnList in `selection_menu::mounted::ReadingList`; the existing body integration fixture also uses this production wrapper.

This path is established from the raw panic location, immediately preceding real height log and source call/borrow ordering; the original log does not contain a runtime backtrace, so it is not claimed as stack-trace-confirmed. No new execution or speculative runtime results are claimed. If parent needs dynamic confirmation before integrating, one backtrace-enabled run of the unchanged failing runner should show List prepaint → item subtree prepaint → Presence/LinkedBody prepaint → viewport_bounds, with the original first-case failure still counted. Do not repeat blind runs.

### Exact required parent patch: `selection_menu/mounted.rs`

Simply nesting two existing ReadingList wrappers is incorrect. Each currently creates a distinct `Rc<()>`; `Mounted::prepaint` captures `ListFrames.last()` for every lease (lines 167–170), while `lease_current` compares against the frame looked up by that lease's actual stream (lines 39–50). The first body's lease would capture the second owner's token and fail real currentness. Both owners must instead register their common List bounds with one shared frame token, scoped over the whole List's prepaint. No body per-row viewport proxy or weakened lease assertion is acceptable.

Parent should apply this exact additive patch after coordinating ownership; this lane does not write that file:

```diff
--- a/crates/vega_ui/src/conversation_stream/selection_menu/mounted.rs
+++ b/crates/vega_ui/src/conversation_stream/selection_menu/mounted.rs
@@
 pub(crate) struct ReadingList {
     child: AnyElement,
-    stream: WeakEntity<ConversationStream>,
+    streams: Vec<WeakEntity<ConversationStream>>,
 }
@@
     pub(crate) fn new(stream: WeakEntity<ConversationStream>, child: impl IntoElement) -> Self {
+        Self::with_owners(vec![stream], child)
+    }
+
+    pub(crate) fn with_owners(
+        streams: Vec<WeakEntity<ConversationStream>>,
+        child: impl IntoElement,
+    ) -> Self {
         Self {
             child: child.into_any_element(),
-            stream,
+            streams,
         }
@@
-        cx.default_global::<ListFrames>().0.push((
-            window.window_handle().window_id(),
-            self.stream.clone(),
-            bounds,
-            Rc::new(()),
-        ));
+        let frames = &mut cx.default_global::<ListFrames>().0;
+        let previous = frames.len();
+        let frame = Rc::new(());
+        for stream in &self.streams {
+            frames.push((
+                window.window_handle().window_id(),
+                stream.clone(),
+                bounds,
+                frame.clone(),
+            ));
+        }
         self.child.prepaint(window, cx);
-        cx.global_mut::<ListFrames>().0.pop();
+        cx.global_mut::<ListFrames>().0.truncate(previous);
```

Existing single-owner production callers retain `new` and one owner/token. The existing lookup, geometry/currentness logic and Mounted capture remain unchanged; both fixture owner lookups now resolve to their real shared List frame. The existing wrapper already does not unwind-clean its push/pop; this patch does not broaden into unrelated panic-lifecycle work.

The lane-owned fixture change gathers both actual `BodyMount.stream` handles before moving mounts into the row callback, wraps the entire List in `ReadingList::with_owners`, then wraps that result once in the existing persistent ScrollScope. Both wrappers preserve child layout bounds and real prepaint traversal. No factory, runner, backend observer, List consumer, case body, assertion, timing or budget change is needed.

### Verification and next concrete step

Only scoped rustfmt and read-only hashes/source hygiene are authorized here. Parent must first supply the above owned-file seam, then format/compile/run the same full eleven-case runner and preserve raw stdout/stderr. Required result remains `11 passed; 0 failed; 0 not executed; 0 ignored`; removal of this panic alone does not satisfy the eleven behavioral regressions. Run relevant mounted/geometry regressions for the shared frame seam without touching the source-anchor author's concurrent scheduler work. This lane compiled 0 / executed 0 in this repair cycle; the only actual execution result available remains the original 0/1/10 failure. No independent review was started.

### Stopped repair hashes and actual static checks

The only Rust content change is seven inserted lines in `scroll_composition_integration.rs`; removing exactly the owner collection and ReadingList wrapper restores its complete pre-repair SHA `b7262e429711dfc9ab5bd4f5b578ea16ae57b6e6c1d88d0b73e848a72c2f6438`. Current full-file SHA-256 is `a759033ea53ab45f61eebe7d8ee5018d81eb6425200955f98be9c5501efff024`. The canonical Python unified diff (`fromfile=pre-repair`, `tofile=current`, default three-line context) SHA-256 is `2f9757ef3afe83621161035f7db27e91ddc745cebff2ee6bef6a45bcd29e1c83`. The other three authorized Rust file hashes remain exactly as in the previous handoff table; no parent/source-anchor/body/backend file was written.

Executed `/Users/puzige/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/rustfmt --edition 2024 --config skip_children=true` on the changed fixture and the same formatter with `--check` on all four authorized Rust files: successful. Read-only Python confirmed only the two intended insertions, exact preimage reconstruction, eleven cases, the unchanged case-suffix hash `7b30ba36a21511414150ef9fa745873d8555a5f2228ed4d38893fbbe91980ade`, original four budget/tolerance literals, final newlines, trailing-whitespace/conflict-marker/comment/lint-allow/ignore/should-panic/placeholder hygiene. An initial static diff reconstruction assertion expected the wrapper's rustfmt line break in the wrong position; inspecting the actual formatted source and fixing only the checker resolved it, with no additional product mutation. These are source checks, not runtime verification. The missing parent-owned `ReadingList::with_owners` implementation is an explicit compilation dependency, not an injected observer or omitted native behavior. Stop writing after final read/hash.

## Five real host regressions appended to the mounted runner

### Authorized acceptance and implementation boundary

This cycle is implementation authorship, not independent review. The user authorizes only `disclosure/scroll_group/host_regressions.rs`, `tests/nsh_scroll_integration.rs` and this document. Keep the first eleven entries in their original order, append the five existing real host regressions, and retain accurate passed/failed/not-executed accounting. Keep every existing behavior function, expected value, fixed 0.5-pixel tolerance, 10-second mount deadline and 2-second/512-tick drain budget unchanged. The fifth host regression must use the installed real production body factory and a whole-List ReadingList boundary, not injected observations or duplicated wheel logic. Parent owns compilation, execution, Cargo and cross-module exports; this lane does not touch source body, disclosure production routing, shared scroll scope or scheduler work.

### Actual wiring

The runner now registers sixteen unique named entries. Entries 1–11 are byte-identical registrations. Entries 12–15 call the four functions with signature `fn(&mut TestAppContext)` through noncapturing adapters; entry 16 directly registers `real_body_then_group_then_original_list_preserve_partial_remainder`, whose signature is `fn(&mut TestAppContext, BodyFactory)`. All mounted cases receive the existing `scroll_body_factory`; the actual platform text system, nonempty platform-font assertion, GPUI initialization and native backend installation remain unchanged. The existing catch-unwind/fail-fast policy remains: only a completed call becomes PASS, a caught panic becomes FAIL, and the untouched suffix is reported as not executed. Thus an early failure does not pretend the five appended cases ran. Required eventual complete result is `16 passed; 0 failed; 0 not executed; 0 ignored`.

The host Root collects its actual `BodyMount.stream` into one owner vector before moving the body into the List renderer, wraps the complete original List in `ReadingList::with_owners`, and keeps that wrapper inside the existing whole-tree persistent ScrollScope. With one real body this vector contains exactly that body's stream. The shared frame token and actual List bounds therefore remain available during real body prepaint without reentering ListState's RefCell. No per-body proxy or separate scroll model is introduced.

The factory originally installs an authority that only recognizes the eleven-case FixtureRoot. The host fixture therefore uses the existing public `set_reading_navigation_authority` setter before mount to install its own actual Root predicate. It requires a live stream, active window, actual host Root, `mounted == true`, and the identical stream stored in that Root's body. Detached roots cannot retain authority; native geometry, source/lease and backend currentness checks remain unchanged. This avoids a false ready/currentness timeout solely because the genuine body is mounted under the new host Root type.

### Exact parent exports

Already present when inspected in `crates/vega_ui/src/lib.rs`; retain:

```rust
#[cfg(feature = "nsh-reading-integration")]
pub use disclosure::scroll_group::host_regressions as nsh_scroll_group_regressions;
```

Required additive crate-private export in `crates/vega_ui/src/conversation_stream/mod.rs`, adjacent to its private `mod selection_menu;` declaration:

```rust
#[cfg(feature = "nsh-reading-integration")]
pub(crate) use selection_menu::mounted::ReadingList;
```

`selection_menu` is private and `mounted` is `pub(super)`, so the host sibling cannot name their direct path. The crate-private re-export exposes only the already crate-private ReadingList type to the host fixture; do not make these production modules public. Parent has already implemented feature-gated `ReadingList::with_owners(Vec<WeakEntity<ConversationStream>>, impl IntoElement) -> Self` and the shared frame token. No new body module, factory injection or Cargo target is requested. This lane deliberately leaves the additive export to parent as authorized; until it is added, the host import is an explicit compile dependency.

### Static validation and stopped handoff

Executed scoped rustfmt, edition 2024 and `skip_children=true`, then `--check` on exactly the two authorized Rust files, using the existing `1.98.0-aarch64-apple-darwin/bin/rustfmt`: both commands exited successfully. Read-only Python confirmed sixteen unique registrations, four ordinary host adapters, the direct real-body fifth registration, unchanged counting/fail-fast code, original budget literals, and no comments, lint allowances, ignore/should-panic attributes, placeholders, conflict markers or trailing whitespace.

The entire five-host-behavior-function suffix remains byte-identical with SHA-256 `4525cc4484b13de961e42ed4a0f26b81457beedd049006ca7702a088be172e06`. The complete original eleven runner registrations remain byte-identical with SHA-256 `75e48b2e0b7d9bf4f71fb57f0b7c151dc39087c2d0adb4c55761fadc1fecc983`. This cycle did not write the original eleven fixture/assertion source at all. No assertion, expected value, timing or budget was weakened.

| Authorized Rust artifact | Before SHA-256 | Stopped SHA-256 |
|---|---|---|
| `crates/vega_ui/src/disclosure/scroll_group/host_regressions.rs` | `50e53878d7b1a5b1d7253fe06cf73fa260e283925aeee9b7e6b112a5912a228c` | `f14e7462e732fe7db030753444fc678469c49b545b885f5a23349ecab7d527c1` |
| `crates/vega_ui/tests/nsh_scroll_integration.rs` | `906d8b145d82eb444821944900bea158035c7e143220a3e821fa0ecfc6e303a0` | `4d1a878d160100179c7a94bce1d3f074f95adc10e882c7d499093da5462a2131` |

This cycle compiled zero targets and executed zero runtime cases, as explicitly required. The parent-reported first NoTarget pass and second vertical-body drain timeout are context, not current sixteen-case validation. No Cargo/rustc, Git, network, app launch, dependency or other authors' file writes occurred. Parent must add the single crate-private export, compile the exact integrated bytes and execute the complete sixteen-case runner with original raw stdout/stderr retained. A preexisting early failure must still leave the later cases counted as not executed, never passed. The other authors retain the real body/backend scheduler diagnosis. No independent review has been started; this lane stops writing after final read/hash.

## Actual async run: phase panic and fixture unwind repair

### Acceptance before this repair

This implementation cycle preserves all sixteen registrations and their order, both runners' parent-installed `allow_parking`, actual native observation/setup, unchanged production owner/source/lease checks, every behavior assertion and the 0.5-pixel / 2-second / 512-tick / 10-second limits. Vendor, production body, shared List consumer and production tool-group implementation are read-only in this cycle. The minimal authorized fixture change must prevent a missing-window cleanup error from causing a second panic while an original case panic is already unwinding; normal cleanup failure must remain fatal and unwind cleanup failure must remain visible. This is not permission to suppress the primary panic, continue running after a failed case, fake offsets, move input dispatch into paint, or relax Window's phase assertion.

### Original evidence and classification

Raw `target/nsh-real-async-scroll.stdout` SHA-256 `91712643f777d56f1e223e40d13d8afcb6cd3d98545f5154035d057f2d48156f` contains exactly two PASS lines. Raw stderr SHA-256 `43072ad5506c29ec430766a3783da73a42d16093f192f2507257b171ccbd4ed8` starts the third case at line 9681. Line 10529 reports actual first-body native offset `Point { x: 0px, y: 2615px }`, zero neighbor/outer offsets and identical expected values after `Fixture::position`. Therefore the actual nonanimated native setup command has returned successfully and the fresh post-frame native observer has matched the requested position; this evidence does not support attributing the subsequent panic to ScrollTo.

Stderr 10567–10569 contains the primary `Window::current_view` phase panic at Fast `window.rs:5404`, without a backtrace. Lines 10571–10572 contain a second `window not found` panic at `VisualTestContext::update`'s unconditional unwrap, whose backtrace identifies composition `Fixture::drop` at line 437. Lines 10661–10667 report destructor panic and SIGABRT. Accurate result: two completed/passed cases, third started but did not complete/report a harness outcome, thirteen subsequent cases never started, process aborted. There is no completed `1 failed` summary in this run.

The backend positioning path is `NativeScrollAccess.position` → `AppView::integration_position_reading_scroll` → fresh `integration_reading_scroll` validation → `crate::scroll::command` → core `scroll::command` → `interrupt` / nonanimated `moved_to` / `report`. No Window is passed to that backend function. It validates live current native geometry and compares actual resulting native state; the fixture must retain these checks and reacquire geometry after invalidation.

The separately identified current-source production defect is `crate::nsh::scroll::consume_list`: after real `list.scroll_by`, it calls `cx.notify(window.current_view())` at line 86 from a synchronous remainder/event callback. Fast's `Window::current_view` permits only Prepaint/Paint, while normal `dispatch_mouse_event` invokes capture/bubble listeners without entering either draw phase. The production tool-group `forward` has the same event-time call at `disclosure/scroll_group.rs:267`. Fast's own List captures `current_view` in its `Element::paint` at `elements/list.rs:1675`, before installing its event callback, and uses the captured ID later. Correct production repair must capture the owning rendered view during prepaint/paint or carry its known EntityId into the consumer, then notify that captured owner during input; retain real List mutation, follow interruption, synchronous remainder and refresh. Do not remove Window's phase assertion or defer/replay the scroll event.

The shared List consumer's observed modification time is newer than the failing binary and raw log, and the first no-target forwarding case passed. Thus the current-source defect is concrete, but the original first-panic call stack is not proven by this log. Parent must run the same exact sixteen-case runner once with `RUST_BACKTRACE=1`, retain both streams, and identify the primary stack before assigning that historical panic definitively. This lane must not rerun Cargo or edit those production files. Fixture cleanup repair below allows a future ordinary unwind to reach the unchanged fail-fast runner instead of aborting solely on missing-window cleanup.

### Delivered minimal fixture change

Both composition and host `Fixture::drop` now obtain the existing VisualContext window handle and call the declared fallible `TestAppContext::update_window` instead of `VisualTestContext::update`'s unconditional unwrap. Existing body stop, scope cancellation, mount removal and notifications are unchanged. A returned cleanup error while `std::thread::panicking()` is true is printed explicitly; it does not create a second panic. The identical error outside unwinding still panics. The primary case panic is neither caught nor suppressed by this change; the unchanged runner remains responsible for reporting one failure and stopping subsequent execution. This handles the observed missing-window error, not arbitrary panics inside cleanup.

Fast `App::update_window_erased` at `app.rs:3135` takes the window out of its map, invokes the update, then reinserts the window in its normal `trail` function. An update panic bypasses that normal trailing reinsertion. This explains why immediately calling VisualTestContext's unwrap-based update from Drop produces the observed `window not found`, independently of the original phase-panic caller. No Fast changes are proposed.

### Exact parent follow-up

First acquire the primary backtrace with `RUST_BACKTRACE=1` on the same sixteen-case target, with this cleanup patch, unchanged runner `allow_parking` and budgets. Do not treat the old destructor stack as the original call chain. If the first stack confirms the shared List consumer, change its signature to `consume_list(list: &ListState, remaining: ScrollRemainder, owner: EntityId, window: &mut Window, cx: &mut App) -> bool` and replace only its event-time `cx.notify(window.current_view())` with `cx.notify(owner)`. Obtain that owner in the actual ancestor's prepaint/paint or from its known entity, not by calling `current_view` at window construction/render/input. Wire the four current call sites: production `turn_placement.rs:152`, composition fixture scope at line 155, host initial scope at line 105 and host replaced scope at its current line 344. If using retained prepaint state, update/revoke it with the same actual scope lifetime. Production group `forward` similarly needs an owner captured by its element during prepaint/paint and stored with current generation; its event callback must not query `current_view`. Parent owns these production edits; fixture callers can be updated only after that concrete API is coordinated. Backend ScrollTo does not currently need an invented Window/phase parameter on this evidence.

### Current-cycle checks and stopped artifacts

Executed only existing rustfmt at `/Users/puzige/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/rustfmt`, edition 2024 with `skip_children=true`, on the two modified Rust files, followed by the same scoped `--check`: exit 0. Read-only Python reversed exactly the new Drop edits in memory and reproduced both full pre-edit SHA-256 hashes, proving all other bytes (including all sixteen behavior functions and their assertions) unchanged. Both files retain exact timeout/tick/tolerance constants and contain no comments, lint allowances, ignore or should-panic markers. Runner and factory full hashes match this cycle's pre-edit values; no runner/body/setup/vendor/production mutation occurred. Zero targets compiled and zero cases executed by this author. These are static checks, not proof the new cleanup compiles or the phase panic is fixed.

| Artifact | Before SHA-256 | Stopped SHA-256 |
|---|---|---|
| `crates/vega_ui/src/conversation_stream/scroll_composition_integration.rs` | `a759033ea53ab45f61eebe7d8ee5018d81eb6425200955f98be9c5501efff024` | `6cf4dd13bec0e5acf0bad79ff1b23f1247f194c5486ef4f29b1aa0058f78d357` |
| `crates/vega_ui/src/disclosure/scroll_group/host_regressions.rs` | `f14e7462e732fe7db030753444fc678469c49b545b885f5a23349ecab7d527c1` | `d31351d96d38e14a6c0be392ab01d9a162834e7d417a86fb830b3127745d6912` |
| `crates/vega_ui/tests/nsh_scroll_integration.rs` | `0e5f0240e30648e25d6c499f9e0e7f18911ece4ff174407400db69eccdbbb736` | unchanged |
| `crates/vega_ui/src/conversation_stream/selection_menu/body_scroll_integration.rs` | `98f908498a08cad48917371e5c936ee4c3017a23c1315d08f134ac4cda15ce30` | unchanged |

The complete original eleven behavior-function suffix SHA-256 is `3fb19e53248c0d7a10475e06b68a549ec14ebdd2615986ba1f427b544717eb15`; five host behaviors remain `4525cc4484b13de961e42ed4a0f26b81457beedd049006ca7702a088be172e06`. Preserve all. Required current-cycle compile and primary-backtrace runtime verification remain parent-owned and pending. No independent review started. This author stops after final read/hash; original run remains two PASS, one interrupted third case, thirteen never started and SIGABRT, not a completed failure-count report.

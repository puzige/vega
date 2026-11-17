# Mounted Thinking body / link preview development integration

## Acceptance and design (written before test implementation)

Scope: close the previously explicit mounted-body integration gap, not repeat the
static F1/F2 review or certify native acceptance. These are temporary development
Rust tests and their exclusive wiring must be removed before master delivery.

### Required observable behavior

1. On the process main thread, obtain the platform text system through
   `gpui_kit::platform::current_platform(true).text_system()`. Inject it into the
   fixed Fast 0.1.9 `TestAppContext::build_with_text_system`. Fail if there are no
   real fonts; ordinary zero-font mock text is not acceptable evidence.
2. Create a real ConversationStream, feed MessageStarted and ThinkingDelta, retain
   its actual RunActivityGroup and ThinkingBlock, and expand the actual Thinking
   card. Mount the real list and real Thinking Render under a window root that
   checks exact stream ownership. Use production rebind_thinking_presentation,
   MessageCopy, render_body, render(Content, thinking=true), Surface, and Engine.
3. Wait with a bounded deadline for the actual body acknowledgement, actual
   snapshot, valid ReadingOwner/root/list/lease, and actual ReadingGeometry.
   Require the preview target to be published by LinkedBody prepaint and to have
   multiple real shaped fragments. Do not register measured targets manually.
4. Dispatch a real wheel event through the mounted window. Capture geometry only
   after actual movement settles. Hover an actual visible fragment; keep the
   pointer stationary while real wall time and real draw frames advance beyond
   the shared scrollbar fade. Require observed alpha to start positive and end
   at zero, geometry to remain current, and the preview card to remain actionable.
   GPUI's deterministic timer alone cannot advance Surface's wall clock.
5. Exercise the card's real keyboard open route and require the original URL to
   reach TestAppContext's open-url recorder. Isolate only the external preview
   request boundary; fail then retry with the real coordinator load/retry path.
   Require the original target, retry flag, cancellation token and real completion
   state, not manufactured ActivePreview/visible/current values.
6. Revoke the actual root ownership while a retry completion is pending. The
   measured target must become noncurrent and open/retry/completion must not
   resurrect a card, launch a URL, or create another request.

### Test layout and smallest requested wiring

Authorized new files only:

- `crates/vega_ui/src/conversation_stream/selection_menu/body_integration_tests.rs`:
  a child of body.rs, so it can inspect actual BodyState through ReadingBinding;
  public runner functions for the custom executable, no ordinary #[test] route.
- `crates/vega_ui/tests/nsh_reading_integration.rs`: harness=false main-thread
  runner, real platform font injection, case counting, panic-to-failure reporting.
- This document: acceptance, wiring, limits and handoff.

Parent/owners must apply the following temporary wiring in their owned files;
this writer must not modify existing files:

1. Add feature `nsh-reading-integration = ["gpui_kit/test-support"]` to vega_ui
   and a [[test]] named `nsh_reading_integration`, `harness = false`,
   `required-features = ["nsh-reading-integration"]`. No new dependency/version.
2. In body.rs add `#[cfg(feature = "nsh-reading-integration")]` and
   `#[path = "body_integration_tests.rs"] pub(crate) mod integration_tests;`.
   Under the same feature, conversation_stream/mod.rs re-exports
   `selection_menu::body::integration_tests::{stationary_thinking_hover_survives_fade_open_and_retry,
   revoked_thinking_root_rejects_open_retry_and_delayed_completion}`; lib.rs
   re-exports those two names from conversation_stream. Do not move the test
   module to selection_menu: that loses BodyState private access.
3. Thinking owner adds a feature-gated `integration_source_copy(&self) ->
   MessageCopy` returning `self.source_copy.clone()`. No visibility, painted,
   acknowledged, lease, owner, generation or geometry state may be forced.
4. link_preview owner adds the feature-gated observation/request seam described
   below. It observes the real coordinator; tests dispatch actual keystrokes,
   never fabricate visible/active state or validity. The
   request hook replaces only the awaited external service.load call after the
   existing load-start gate and before the unchanged completion gate. The hook
   returns a controlled future and receives original target, retry, cancellation.
5. Backend owner supplies an observation-only
   `AppView::integration_scroll_alpha() -> Option<f32>` (same feature can be
   forwarded to a temporary backend feature, or use existing
   vega-host-regressions). Read the actual mounted reading scroll node's live
   alpha. Do not create a scroll node, drive alternate wheel code, mutate time,
   rebind geometry, or change repaint/invalidation behavior. Reject ambiguity if
   more than one reading scroll is mounted.

### Exact preview seam requested from its owner

Feature-gated `pub(crate) mod integration_support` inside link_preview.rs:

- `type PreviewResult = Result<LinkPreviewData, LinkPreviewFailure>`.
- `type Request = Arc<dyn Fn(String, bool, CancellationToken) ->
  Pin<Box<dyn Future<Output = PreviewResult> + Send + 'static>> + Send + Sync>`.
- `install_request(cx: &mut App, request: Request)`: store in the app's test
  registry before a coordinator is acquired. In load(), clone the hook after
  the existing active_current gate, carry it into the same background task,
  and await hook(target, retry, cancellation) instead of service.load only
  when installed. Preserve the existing generation/key/cancellation/current
  completion checks verbatim. No production fallback network request in this
  executable, including after root revocation. No hook calls from observation.
- `observe(window: &Window, cx: &App) -> Option<Observation>`: upgrade the real
  coordinator without acquiring/creating one, clone actual fields, and evaluate
  existing `VisibleLink::current` and `Coordinator::active_current` with those
  exact window/app arguments. Must be read-only; no ticking/rebinding/dismissal.
- `Observation { pub visible: Vec<ObservedLink>, pub active: Option<ObservedActive> }`.
- `ObservedLink { pub target: String, pub current: bool,
  pub text_bounds: Vec<Bounds<Pixels>> }`.
- `ObservedActive { pub target: String, pub current: bool,
  pub data: Option<PreviewResult>, pub open: FocusHandle, pub retry: FocusHandle,
  pub card_bounds: Option<Bounds<Pixels>> }`; card_bounds comes from the actual
  coordinator card_bounds, not recomputed or estimated rectangles.

Tests obtain actual focus handles from observation, use window.focus(), then
send real Tab/Enter through VisualTestContext. No direct keyboard()/arm()/load()
helper is needed; no ActivePreview or VisibleLink is constructed by the tests.
The controlled request future deliberately can finish after cancellation, so
that the unchanged production completion gate must reject it. Its completion
AtomicBool records only consumption of that external future, never core validity.

### Confirmed fixed API / asynchronous prerequisites

- Fixed dependency identities: gpui-kit 0.7.3, gpui-fast 0.1.9. Fast already
  exposes build_with_text_system(TestDispatcher, Option<&'static str>,
  Arc<dyn PlatformTextSystem>); no registry or Fast edit is required.
- Body valid() includes ReadingOwner.current(), matching source/revision and
  acknowledgement, Surface error status, live lease, viewport/scale, actual list
  viewport and foreground height. Tests must assert this method directly.
- ReadingOwner.current() checks the actual RunActivityGroup child and Thinking
  reading_menu_current(), plus list item visibility. Mounting a detached Body is
  insufficient.
- The app-owned navigation authority is a function pointer supplied by the
  outer window. The test root must implement the same exact-root/exact-stream
  ownership contract, not `|...| true`; this is an in-process window fixture,
  not the separate vega crate's production VegaWindow controller.
- AppView.load() starts the real Engine; mounting is asynchronous. A bounded
  pump must drive GPUI tasks and draw frames while yielding to the Engine's real
  worker, and report errors/timeouts rather than silently skip assertions.
- rt.rs installs a process-wide OnceLock JOBS queue with a foreground receiver.
  Both cases must share the same TestAppContext for the process lifetime; making
  a fresh context per case could leave subsequent Engine callbacks attached to
  the first scheduler. The runner intentionally shares one context and stops
  each mounted body's real Surface before proceeding.
- Surface.clock() is epoch.elapsed(); real prepaint invokes surface.tick() and
  paint requests animation frames. Sleeping without drawing or advancing only
  the test dispatcher does not prove scrollbar fade.
- LinkPreviewService::with_ports and its ports module are private to
  vega_conversation. Do not broaden that API or bypass coordinator validity;
  isolate only the external awaited request at link_preview's existing boundary.

## Implemented cases and remaining integration work

- `stationary_thinking_hover_survives_fade_open_and_retry`: real wheel must revoke
  the pre-scroll geometry; observed alpha must become positive and then zero;
  every fade frame must retain exact post-scroll geometry currentness and live
  production link identity; request failure, real keyboard retry, successful
  completion, and real card keyboard open must preserve the original URL.
- `revoked_thinking_root_rejects_open_retry_and_delayed_completion`: repeat the
  real mounted/fade path, then remove the actual stream from its root while
  retry is pending; real predicates must reject target/card, real keystrokes
  must neither open nor retry, token must cancel, and deliberately late request
  completion must actually be consumed without resurrecting the card.
- Both use a real ListState populated by production events and production
  RunActivityGroup::render/render_segment. Expansion uses reveal_segment(),
  never sets painted/lease/currentness flags or fabricates list bounds. The
  tiny real mouse-wheel Lines delta uses the actual window event route and
  avoids introducing touchpad momentum into the stationary fade assertion.
  The fixture retains the actual mounted BodyState entity after root removal
  solely for observing rejection and stopping its Surface, not for re-mounting.
- The outer test window replaces only the application controller's root-owned
  navigation callback with an exact FixtureRoot/stream/route ownership check.
  It does not mount the separate vega crate's VegaWindow or exercise that full
  controller, compositor raster output, native AX, or external browser behavior.
- Frame draws are explicit refresh/draw/arena-clear cycles. The test exercises
  real render tick/fade behavior but is not an assertion that an OS frame pump
  or presentation system delivered frames autonomously.
- Parent must wire the seams, type-check and execute. Correct harness/type/
  layout/Engine synchronization errors at their responsible layer; retain the
  currentness, real font, positive-to-zero alpha, delayed consumption and
  original URL assertions. A failure is not permission to substitute mock
  geometry or mark the case ignored.

## Production List/prepaint borrow repair (current authorized stage)

Root cause: Fast 0.1.9 `List::prepaint` holds its `ListState` mutable borrow
across row render/layout/prepaint. The diagnostic in
`target/nsh-body-borrow-diagnostic.stderr` reaches body canvas viewport sampling;
LinkedBody validity (`ReadingOwner::current`) and selection-menu canvas/validity
also read that same list. Deferring only the first canvas cannot repair this.

Acceptance/design before implementation:

- A production list-parent element records its actual layout bounds only for
  the synchronous child prepaint call, keyed to the exact stream. The sample is
  not a cached previous-frame viewport and is unavailable outside that call.
- Mounted body/menu geometry uses this same-frame parent sample and the real
  current content mask, bounds, viewport, scale and foreground. Presentation
  validity checks exact root/source ownership plus the live mount/geometry lease
  without touching ListState while its mutable borrow is held.
- Existing full list item visibility and viewport checks remain at interaction,
  request and asynchronous completion boundaries, when List prepaint has ended.
  No panic catching, guessed text geometry, permanent-false fallback or validity
  bypass. LinkedBody still shapes/publishes links, focus and AX in the original
  scope and frame; no deferred draw that loses the scope stack.
- The real integration fixture wraps its real list in that same production
  parent element. Existing assertions, real fonts/Engine, fade, URL, retry,
  cancellation and root-revocation requirements are unchanged.
- This writer does not build/run tests; parent centrally type-checks and executes
  both actual mounted-body cases and the affected menu/link regressions.

Implemented in the authorized production files:

- `mounted::ReadingList` shares its child's layout node, surrounds only synchronous
  prepaint, and keys the actual bounds by window ID and exact weak stream entity.
  Production render wraps TurnList/fade (both forward the same bounds); TurnList
  creates its inner List with those exact bounds. The integration fixture wraps
  its actual List in the same element, with no assertion changes.
- Each scope gets a fresh Rc identity. `Mounted::prepaint` clears geometry and
  records that identity before child prepaint. Body/menu presentation requires
  its lease to match the current scope; a previous-frame lease cannot authorize
  publication. The identity is deliberately not part of Geometry equality, so
  pure repaint/fade does not invalidate an otherwise identical link target.
- `ReadingOwner::source_matches` preserves all matching entries (including
  multiple Thinking segments). `current_for_frame` checks the unchanged root
  authority and source identity without List reads only inside the parent scope;
  outside it, the original full `current` item-bounds/viewport check runs.
- Both body/menu canvas viewport reads and both validity viewport comparisons
  use the scope bounds lazily, falling back to ListState only outside that scope.
  This covers LinkedBody's initial `state.valid`, the measured-registration
  validity callback invoked immediately by link_preview, and menu container-query
  / MenuState render validity, not only the first crashing canvas.
- Native requests, selection/copy, pointer/key/AX actions and async processing
  still use their existing validity entry points. The scope has already been
  popped there, so they run live List checks. Projection/source/root/mount,
  snapshot/geometry identity, original URL/binding, async generation and
  cancellation gates were not removed. No vendor/core/dependency change.

Current-stage verification: read the actual fixed Fast List, TurnList,
ContainerQuery, Mounted and preview registration lifecycle, and the captured
backtrace; executed scoped `rustfmt --edition 2024 --config skip_children=true`
and its `--check` on the five authorized Rust files, successfully. No Cargo,
type-check, test, Git, network or native launch in this stage. Parent-reported
279 app / 32 host / 12 real-font passes preceded this repair and do not validate
it; actual mounted-body case 1 had the recorded borrow panic. Parent must run
the unchanged two mounted-body cases (including fade/open/retry and revoked-root
late completion), plus affected selection-menu/link regressions, on these final
identities. Actual native acceptance and independent review are not claimed.

## Mounted-runner timeout diagnostic design

The parent rerun recorded in `target/nsh-integrated-fixture-test.stdout` and
`.stderr` finished 284 application, 33 host and 12 real-font geometry/opacity
cases, then started `nsh_reading_integration` without a case result before the
120-second command timeout. Parent reports no process remained afterward. This
is not a mounted-body pass or evidence yet of a particular product loop.

Acceptance/design before this diagnostic-only change:

- Modify only the body integration fixture and this document. Keep both cases,
  all behavior assertions, production currentness and real Engine/font paths.
- Emit stderr BEGIN/END records around fixture initialization, open_window and
  its construction closure, visual.update, refresh, draw, arena clear, wait
  predicates, clock advancement, real input dispatch and teardown. Include
  elapsed wall time and a case label. A BEGIN without END identifies a stuck
  synchronous call; logging cannot preempt that call.
- Replace unbounded task drains, including those hidden in GPUI simulation
  helpers, with the fixed API's single-task `BackgroundExecutor::tick`. Require
  quiescence within 512 ticks and two wall-clock seconds per drain; exhaustion
  is a diagnostic failure with queued foreground/background task counts, not a
  successful partial pump. Log individual tick boundaries so a nonreturning
  task can be distinguished from continually runnable tasks.
- Dispatch pointer input through `Window::dispatch_event` and keys through
  `Window::dispatch_keystroke`, the real routes used by the fixed helpers.
  Only remove their implicit unbounded drain; never invoke preview actions
  directly or replace owner/geometry/currentness decisions with test values.
- Preserve the existing ten-second wait deadline, real sleeps, explicit
  deterministic clock advancement and actual refresh/draw/clear lifecycle.
  Bounded drain does not advance time or claim autonomous frame delivery.

Actual fixed source inspected: Fast 0.1.9 `app/test_context.rs` (SHA-256
`73b6a83396db51e73cc7e0dcd9ecfe60b0a79940bb49084ba54d5c1e71d2e476`),
`executor.rs` (`02e68407eb89156de31e05320a4d3565ea6f1c639dd378650b1703e8729b5640`),
`platform/test/dispatcher.rs`
(`dea03a78a0c64d486f2c5b7bc669ba281638a0f8ee53316a52993a0e5b5cabd0`),
and locked gpui-pre-scheduler 0.3.9 `test_scheduler.rs`
(`2c07373f6651ed486b28bdcd4f1bce8e6eb653d78a7588bc37c838b2da7585c2`).
The actual scheduler `run()` loops `step()` without advancing time, despite the
broader executor doc comment mentioning timer advancement; `tick()` handles
expired timers or at most one runnable. `TestAppContext::run_until_parked` loops
`tick(false)`. Mouse simulation and key simulation use the background executor's
unbounded drain. `open_window` and VisualTestContext::update are synchronous,
without their own explicit scheduler drain. A product paint/notify loop is not
established by this static evidence; the next runner output must identify its
actual owner before any production change is proposed.

Diagnostic implementation now logs case entry, fixture setup, window construction,
activation, every explicit draw/clear and scheduler tick, wait predicates, real
input dispatch, fade entry and stop. Tick records include queued task counts;
END records report wall-clock elapsed time and whether unwinding is underway.
A budget failure is deliberately not a passing frame. No assertions were removed
or relaxed, no case was removed, no timing interval was extended, and no
production files were changed. The existing wait deadline remains ten seconds.
A single nonreturning task, draw, update, or log write cannot be preempted by
this same-thread diagnostic; retain the parent's outer process timeout and raw
stderr. If there is no case BEGIN at all, the stall precedes these fixture
functions in the unchanged runner initialization, which is outside this write
scope. If ticks return repeatedly but the drain exhausts its budget, identify
the actual task/notify owner before proposing any production repair; do not
raise the budget merely to obtain a pass.

Parent-only type-check route (not executed by this writer):

```sh
CARGO_FORK_AUTO_UPDATE=0 rustup run 1.98.0 cargo check -p vega_ui --features nsh-reading-integration --test nsh_reading_integration --offline --locked -j 2 --target-dir target/official-nsh-validation
```

Then run only the diagnostic target, retaining the existing outer timeout:

```sh
CARGO_FORK_AUTO_UPDATE=0 RUST_BACKTRACE=1 rustup run 1.98.0 cargo test -p vega_ui --features nsh-reading-integration --test nsh_reading_integration --offline --locked -j 2 --target-dir target/official-nsh-validation > target/nsh-body-stage-diagnostic.stdout 2> target/nsh-body-stage-diagnostic.stderr
```

Inspect the final `[nsh-body]` BEGIN/END records and any budget failure alongside
the backtrace. The unchanged custom runner reports each PASS/FAIL and fails
without silently skipping a case. Previously passing app/host/font suites are
not rerun by this focused command and are not evidence for these new bytes.
Executed in this diagnostic stage: scoped `rustfmt --edition 2024 --config
skip_children=true` and `--check` on body_integration_tests.rs, both successful.
No Cargo, type-check, test, Git, network or native launch was run. Two cases
remain defined; zero executed here. The timeout's exact runtime phase and any
product-loop ownership remain unverified pending the parent's diagnostic run.

## Earlier test-writer verification status

Design/source work only in this stage. No Cargo/build/test/native launch, Git,
network, dependency/registry edit, or existing source edit is authorized. New
cases are not runnable until parent-owned wiring and observation/request seams
are present. Rustfmt parsing is not type checking or runtime evidence. Earlier
258 app + 30 host + 6 geometry passes do not cover these new cases.

Executed here: scoped `rustfmt --edition 2024` and `rustfmt --edition 2024
--check` on exactly the two new Rust files. Both completed successfully before
final handoff. This is syntax/format evidence only; two cases defined, zero
executed here. No independent review is claimed: this agent authored the tests.
Exact final file SHA-256 identities are supplied in the handoff, without a
self-referential document hash inside this document.

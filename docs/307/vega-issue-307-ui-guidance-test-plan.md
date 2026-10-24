# #307 UI guidance/status TEST-FIRST contract

## Authority and checkpoint

This is the new UI-only test plan for the main spec's **最新范围裁决**, not the superseded automatic-ready plan. Baseline `a5c5a13ffed663ae7252e2390fbf144cca65c2fc`; origin fetched and branch fast-forward checked (already current) before writes. Runtime implementation is independently owned; no runtime/conversation type changes belong to this patch.

Only `cua_onboarding/guidance_tests.rs`, its `#[cfg(test)]` module declaration, and this document are authored here. No controller, lifecycle behavior, native window, renderer, new dependency, test framework, commit, push, build, test or app execution. Existing stage1 bundle/placement/retained_entry APIs remain production regression inputs, not restored historical tests.

**Intentional missing-API RED candidate, not observed RED:** tests import absent production modules `cua_onboarding::controller` and `cua_onboarding::lifecycle`. No stubs or independent test-side controller exist. Behavior assertions have NOT executed. Independent review and Actions must confirm target-interface failure before UI behavior implementation. Runtime-only parallel changes do not count as UI implementation or UI RED evidence.

## Production contract to implement only after RED

These are UI-internal pure APIs, not cross-crate shared types. Use existing `vega_conversation::types::{Preparation, ComputerUseError, ComponentState, PermissionState, DirectCapture}`. Do not introduce a probe wrapper or depend on a concrete runtime session constructor.

### `controller` signatures

```text
CardController::new(task: &str, executable: &Path) -> CardController
CardController::view(&self) -> CardView
CardController::act(&mut self, action: CardAction) -> Vec<CardEffect>
CardController::complete(&mut self, ticket: CheckTicket,
                        result: Result<Preparation, ComputerUseError>) -> Vec<CardEffect>
CardController::begin_drag(&mut self) -> Option<PathBuf>
CardController::drag_ended(&mut self)
```

`CardView` is an owned comparable renderer projection (`Debug + PartialEq`), with fields `component: Option<ComponentState>`, `accessibility: PermissionState`, `screen_recording: PermissionState`, `direct_capture: DirectCapture`, `check_state: CheckState`, and bools `open`, `following`, `can_drag`, `can_recheck`, `can_return_to_task`, `desktop_ready`. This is the actual future renderer input, not a second model solely for tests. `CheckState` includes `Idle`, `Checking`, `Observed`, `Busy`, `Unknown`, `Failed(ComputerUseError)`; observed denied is not busy/unknown. No result means no invented component observation. This guidance-only flow never establishes live-capture readiness.

`CardAction`: `Recheck`, `AccessibilitySettings`, `ScreenRecordingSettings`, `RevealInFinder`, `ReturnToTask`, `Close`.

`CardEffect` (`Debug + PartialEq`) is the exhaustive adapter boundary:

```text
Inspect { ticket: CheckTicket, prompt: bool, probe_direct_capture: bool }
OpenUrl(String)
RevealInFinder(PathBuf)
CancelPending
CloseWindow
ReturnToTask(String)
```

No execute/resume/authorize effect. `Inspect` always carries false/false. Adapter reuses the runtime owner's shared embedded preparation session. `ReturnToTask` is navigation only; user must submit a new task through normal authorization and fresh observation. Effect order cancels/invalidate work before closing, then navigates when requested. Close disables all actions. `CheckTicket: Clone + Debug + PartialEq` is opaque and scoped by task, unique card lifetime, and check generation; generation alone is insufficient when a card is reopened. Newer manual recheck supersedes the earlier callback. `begin_drag` returns the exact bundle path and freezes following synchronously, before any native handoff; end/cancel/failure never unfreezes this lifetime. No drag for the actual development executable; Finder reveals that executable, not a guessed installed app.

### `lifecycle` signatures

```text
EntryLifecycle: Default
EntryLifecycle::begin_task(&mut self, task: &str) -> TaskContext
EntryLifecycle::observe(&mut self, context: &TaskContext, error: ComputerUseError)
EntryLifecycle::detach(&mut self, context: &TaskContext,
                       outgoing_error: Option<ComputerUseError>)
EntryLifecycle::dismiss(&mut self, context: &TaskContext)
EntryLifecycle::visible(&self) -> Option<retention::PrepareEntryKind>
```

`TaskContext: Debug + PartialEq` includes run generation, not just task text; restarting the same task invalidates old callbacks. All callbacks including dismiss/detach must carry their original context. `detach` captures the outgoing status before discarding control; repeated detach with no error preserves the retained entry. Explicit dismissal persists for that task generation despite repeated status delivery/detach; a new task generation resets it. Classify using existing `retained_entry` / `ComputerUseError::needs_permissions`, never error text or a blanket failed phase.

## Fifteen frozen test targets

Source: `crates/vega_ui/src/cua_onboarding/guidance_tests.rs`. Helpers only create `Preparation` data and unpack/validate actual controller effects; they implement no decisions.

| Test | Observable failure guarded |
| --- | --- |
| `observed_component_and_permissions_remain_separate` | Missing component, granted AX, denied screen remain independently represented; capture unverified |
| `granted_static_checks_never_complete_or_close_the_card` | Both TCC grants do not imply desktop ready or automatic completion/close |
| `busy_unknown_and_denied_are_distinct_and_retryable` | Busy, unknown and observed denied have distinct states; recheck recovers |
| `settings_actions_only_open_the_requested_privacy_route` | Exact AX and ScreenCapture URL, no inspection/prompt/other effects |
| `drag_constructor_freezes_following_for_lifetime_but_not_recheck` | Constructor freeze survives drag end and recheck; fresh lifetime follows |
| `development_executable_has_no_drag_and_finder_reveals_real_executable` | No invented app payload; fallback uses actual executable |
| `packaged_finder_action_reveals_the_app_not_a_guessed_installation` | Nonstandard installed bundle preserved |
| `close_invalidates_callbacks_and_all_later_actions` | Cancel before close; immutable closed projection; no action/drag after close |
| `older_check_generation_cannot_overwrite_newer_observation` | Late callback cannot overwrite current state |
| `callbacks_from_another_task_or_closed_card_lifetime_are_rejected` | Old card ticket rejected for both reopened same task and another task |
| `return_only_navigates_to_original_task_and_never_resumes_authorized_actions` | Only cancel/close/navigation effects, never replay old authorization |
| `detach_captures_outgoing_permission_error_and_repeated_detach_preserves_entry` | Entry survives control removal even without prior observe |
| `explicit_dismiss_survives_repeated_status_and_detach_for_same_task` | Polling/detach cannot resurrect dismissed entry |
| `nonpermission_failures_do_not_create_a_permission_entry` | Protocol/unavailable/busy/rejected do not masquerade as permission failure |
| `new_task_generation_rejects_old_observe_detach_and_dismiss_callbacks` | Old task callbacks cannot create or dismiss current entry; same-ID restart safe |

Transport boundary: `ExecutorFailure` has private fields and no public transport fixture constructor; UI must not modify runtime/types or add a test-only runtime constructor. The nonpermission test does **not** claim `Unavailable` is transport. Static review must verify `ExecutorFailure::needs_permissions` excludes `Class::Transport` (currently only the three permission classes match), and the lifecycle delegates classification to that production API. Actual transport injection is not covered by these pure UI tests. If independent review requires executable transport coverage, coordinate a separate runtime-owner test before that behavior is implemented; do not pretend this suite supplies it.

## Mandatory real production wiring anchors

Pure tests are useful only if these APIs are actually consumed. Review after implementation must verify each anchor, not source-string assertions or a parallel model:

1. `conversation_stream/core.rs`: store one `EntryLifecycle` plus active task context on `ConversationStream`, initialize in `new_with_request_queues` using `thread` identity. Do not rebuild it on every render.
2. `conversation_stream/computer_use.rs::set_computer_use_control`: before replacing/removing the old `ComputerUseControl`, read its current `status().error`, deliver it with the current context to `detach`; do this even if no earlier status render occurred. Do not let repeated `None` clear retained state. Observe incoming status through the same lifecycle. Existing diagnostic generation and takeover invalidation remain intact.
3. `conversation_stream/content.rs`, `ConversationEvent::MessageStarted`: the current code directly clears control/diagnostic state. Route this boundary through lifecycle generation transition once the event is accepted (after duplicate-active-message rejection); ensure old callbacks carry the previous context. Thread switch/replacement must also invalidate old context.
4. `conversation_stream/computer_use.rs::render_computer_use_notice` currently returns immediately for absent control. Render the retained entry independently of that early return; use `visible()` for entry and task-scoped dismiss. `conversation_stream/render.rs` existing `.children(self.render_computer_use_notice(cx))` remains the production placement anchor; preserve unrelated pause/resume UI behavior.
5. `vega/src/window/artifact.rs` stream control refresh is guarded by `if let Some(active)` and passes `active.computer_use.clone()`; it does NOT itself send `None` after the active entry disappears. Review the finish/detach integration explicitly rather than assuming that delivery exists. `AppAgentController::finish` removes active run. Arrange capture while the outgoing control is still available, and route any later explicit `None` through the same setter; do not expect a later runtime error delivery.
6. Future card renderer/window owner under `cua_onboarding`: owns **this** `CardController`; every visible component/AX/screen/capture state and enabled state comes from `view()`. Click and keyboard callbacks both dispatch the same `CardAction`; no second reducer or renderer-local readiness policy.
7. Effect adapter: `Inspect` delegates to the runtime owner's existing session with false/false; completion retains exact `CheckTicket`. URL/Finder failure feedback must be truthful; dispatch is not proof the external application opened. Return handler selects the original thread only, never invokes control resume, task submission, pending action, or old observation replay.
8. Native `.on_drag` constructor calls `begin_drag()` synchronously before returning the GPUI payload; external payload uses that exact `.app` path with `FileDragPaths`. Native handoff/end is not accepted-drop/permission evidence. Follow loop consults `view().following`; it never resets the freeze on `has_active_drag == false`.
9. Manual close, window destruction and return paths invalidate controller tickets and stop follow/probe loops before dropping window ownership. A delayed task must not target a newly opened card for the same task. Cancellation of preparation must not stop an unrelated active computer-use run.

Anchors are planned integration requirements, **not verified wiring**. This patch deliberately does not edit any of them beyond the test module declaration.

## Evidence, review and remaining verification

GPUI harness is unavailable and new dependencies are not authorized. These are pure controller/lifecycle tests, not component-tree/AX-tree/screenshots/native UI tests. They do not prove focus/keyboard wiring, real dragging, follow geometry, Settings window discovery, nonactivation/Spaces behavior, clipping, dialog visibility or real TCC. Follow the native review's coordinate/ownership/drag constraints and perform user manual validation later. Static integration review must separately inspect all nine anchors above, including transport classification.

Local permitted checks: isolated `rustfmt --check` on this test file, `git diff --check`, test-definition count, content hashes and scoped diff review. No local Rust execution. Coordinator-only Actions diagnostic command after independent review: `cargo test -p vega_ui --lib cua_onboarding::guidance_tests` using existing PR workflow/toolchain; retain exact invoked workflow command and raw failure, not merely this proposed filter. No workflow changes authorized here. Expected missing API imports are the intended RED target; unrelated compiler/dependency/runtime-owner failures are not UI RED. No Actions result is available in this handoff.

Before UI implementation: independent review freezes exact source hashes, coordinator obtains Actions target-interface RED and records run/job/raw reason. After implementation: same unchanged tests must reach Actions GREEN; review real production integration and native/manual gaps separately. Preserve source and RED/GREEN evidence before the required independent test-removal commit and final empty-set gates. Do not remove failing tests to obtain GREEN. This writer stops writes at READY for independent review.

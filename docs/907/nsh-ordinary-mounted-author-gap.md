# Ordinary mounted controls: author implementation gap investigation

## Scope and evidence boundary

Author implementation investigation for #907, **not independent review, acceptance or implementation completion**. Only this new diagnosis file is writable. No source, Cargo, vendor, dependency, comment or suppression changes were made. No build, test, application, AX activation or network operation was run. Parent owns central validation; another agent owns the footer repair. The footer source was not inspected or changed.

Worktree: `/Users/puzige/Workspace/worktrees/vega-907-nsh-full`. User-supplied HEAD: `0b5ea426abfd8d4358489e916273d89474fb190f`, with dirty migration bytes; HEAD alone is not source identity. Parent reports remote fetched with no new baseline delta. Applicable root AGENTS, `docs/vega-exec-guide.md`, design guidance, [migration contract](nsh-full.md) and [C2 acceptance](nsh-timestamps-ordinary-controls.md#acceptance-fixed-before-code) were consulted; no nested AGENTS was found. All implementation lanes finish before consolidated independent review.

Execution deviation: the initial shell was in `loom` and ran read-only Git identity/status commands there despite the no-Git instruction. No Git mutation occurred; no later Git command was used. Source identities below use SHA-256 reads, not Git.

### Existing parent evidence, not a new validation run

- `target/nsh-closure-final-callback-execution.json`: ordinary original-callback/node-reuse test exit 0; timestamp centering test exit 0; UI suite exit 0; mounted runner exit 1, no watchdog timeout.
- `target/nsh-closure-final-callback-ui.stdout:361`: **357 passed, 0 failed, 0 ignored** for that historical executable, not a guarantee about concurrently changing source.
- `target/nsh-closure-final-callback-ordinary-mounted.stdout`: **1 passed, 1 failed, 2 not executed, 0 ignored**. Timestamp mounted case passed. First ordinary case failed during fixture initialization, before controls; the other two ordinary cases were not executed.
- `target/nsh-closure-final-callback-mounted-backtrace.stderr:128–146` locates the already-owned footer failure at `ListState::viewport_bounds` during list child prepaint. Only the existing failure frames and enclosing list contract were used here; this is not a second footer investigation.

## Finding 1 — concrete ordinary production prepaint reborrow

**Classification: statically established production defect, currently masked by the earlier footer panic; not an observed ordinary runtime panic yet.** It violates the existing requirement that actual mounted controls converge and remain usable, without requiring any acceptance change.

All crate paths in this section are relative to the worktree:

1. `crates/vega_ui/src/conversation_stream/render.rs:699–706` constructs `TurnList` with `self.list.clone()`; `788–790` appends `ordinary_controls(index, cx)` inside that list row, after its message body/footer.
2. `crates/vega_ui/src/conversation_stream/turn_placement.rs:323–342` creates the actual GPUI list with the same `ListState` and prepaints it. There is no separate geometry-only list for controls.
3. Resolved `gpui-fast-0.1.9/src/elements/list.rs:1619` acquires `self.state.0.borrow_mut()`. That guard remains live through `state.prepaint_items` at `1649`; `prepaint_items:1356–1357` synchronously prepaints each row child.
4. Ordinary `crates/vega_ui/src/conversation_stream/ordinary_submissions/presentation.rs:605–637`, specifically **619–620**, calls `stream.list.viewport_bounds()` and `stream.list.bounds_for_item(self.index)` inside that child prepaint, with a mounted global element id and live stream owner.
5. Fast `list.rs:821–822` implements viewport lookup with `self.0.borrow()`; row lookup at `728–729` also calls `borrow()`. These are immutable reborrows of the exact RefCell still mutably borrowed by the enclosing list. Fixing only the first getter leaves the second hazard. `last_layout_bounds` is only assigned after child prepaint at `1660`, so merely reading an earlier cached viewport is not an equivalent current-frame geometry contract.

The footer precedes controls in this row, explaining why the supplied run cannot expose this second site. The fault is in Vega's ordinary geometry sampling phase, not a need to modify Fast or relax the mounted fixture.

### Exact minimal repair contract to parent — not applied here

Change the ordinary `Mounted::prepaint` geometry sampling at `presentation.rs:614–637`, not Fast, controller authority, native press callbacks or test expectations:

- During child prepaint capture only the actual control bounds, current content mask and weak identities. Invalidate old geometry immediately; do not preserve action authority while new geometry is pending.
- Obtain viewport and row bounds only **after the enclosing list prepaint has returned**. An existing safe outer boundary is after `body.prepaint_at` in `TurnList` (`turn_placement.rs:340–343`); alternatively `Window::defer` (`Fast window.rs:2631–2637`) schedules a window callback at the end of the effect cycle. Parent chooses the smallest integration with the other geometry repair; this lane adds neither.
- A deferred callback must retain only a weak mount/host and reject released/replaced mounts, changed source/presentation, superseded geometry captures, missing row and non-finite/zero width. It must not keep an unmounted lease alive or install older geometry over a newer prepaint.
- Recompute clipped visibility using the captured control/mask bounds plus the current viewport and foreground exclusion, then publish/notify only for a real change. Require the native displayed acknowledgement for the resulting geometry/presentation before accepting commands. Do not manufacture a fresh authority snapshot inside activation.
- Preserve `Host::valid:365–388`: exact presentation/source/submission/attempt, live lease, current route/settings/preview/suspension, nonempty visibility, current viewport/row/foreground, active window and healthy Surface. Its list reads occur on the asynchronous request/input path, not from this child prepaint; this investigation establishes no second conflict there.

This source/contract was reported to parent before writing this note. Source repair remains parent-owned because this lane is diagnosis-only.

## Remaining three mounted cases and necessary verification

Runner order is fixed in `crates/vega_ui/tests/nsh_ordinary_controls_integration.rs:8–25`. Preserve **all assertions and the 2-second/512-tick drain and 10-second readiness budgets** (`presentation/mounted_regressions.rs:171–211`); no ignore, suppression, deadline extension or fail-fast accounting change.

| Ordinary case | Required observable result, already encoded | Current evidence |
| --- | --- | --- |
| `production_buttons_pass_pointer_through_full_inset_wrappers_and_keep_focus_dispatch` (`mounted_regressions.rs:432`) | Actual NSH-button-coordinate pointer input through transparent wrappers; exactly one Retry event with original thread/submission/displayed attempt; original action-before-diagnostics focus order and Enter/Space; exact displayed-attempt diagnostic clipboard with no action event; 360px reflow converges | Fixture initialization failed before these assertions |
| `retry_and_same_attempt_fact_changes_reject_old_displayed_controls` (`486`) | Real `try_retry` invalidates captured old Retry/diagnostics immediately; new attempt reprojection copies new diagnostics; same-attempt `invalidate_recovery` again rejects old controls and preserves clipboard sentinel | Not executed |
| `route_settings_geometry_and_unmount_reject_captured_controls` (`542`) | Settings, absent route, changed foreground, actual width/reflow and actual unmount reject old captures; actual unmount releases lease; no event or clipboard mutation | Not executed |

`Fixture::rejects:388–415` deliberately calls `Host::activate` with the captured old snapshot. This is useful negative production-fence coverage, **not pointer, keyboard or AX dispatch coverage**. Likewise the passing original NSH press callback test is not AX evidence. No further current production defect was established in these two unexecuted cases; possible input/convergence failures must be classified from the next actual run, not preemptively repaired.

Parent next verification: finish the separately owned footer repair and ordinary geometry repair, then rebuild final bytes and rerun the complete existing mounted runner under the original budgets. Record all four case results, executable/source identities and actual failures. Rerun the exact original-callback/node-reuse regression and relevant UI suite on those final bytes; earlier 357/357 does not substitute. Do not skip the first failure to present the remaining cases as a full pass.

## Production request/controller route: no additional established gap

- Native `pixels.rs:169–193` keeps immutable observed snapshot, live command/presentation owners and revision across measurement; sends Displayed then Activate. Host consumes them through `presentation.rs:439–476` and validates before activation.
- `Host::activate:398–425` checks displayed action eligibility and current geometry/source; emitted `OrdinaryActionRequested` retains **observed** thread/submission/attempt. Diagnostics calls `preparation_diagnostics(observed.source.attempt)` and emits no ordinary request.
- `crates/vega/src/window/native_subagents.rs:624–629` subscribes the real stream event to `VegaWindow::ordinary_action`. `ordinary_send.rs:647–688` checks owned stream/route, database-local submission and exact latest attempt before `try_retry(request.attempt)`; `agent.rs:231–247` requires both opened thread and cached stream entity. No latest-attempt substitution or concrete bypass was found in this scoped path.
- The mounted fixture only subscribes into `Root.requests` (`mounted_regressions.rs:129–145`); it does **not** instantiate `VegaWindow`, run preparation or prove backend retry completion. Preserve that honest boundary; do not broaden this lane into unrelated controller audit or real-provider execution.

## Actual AX activation route without modifying Fast

**Production route exists; current TestWindow cannot exercise it. This is a test/observation gap, not evidence that production AX is absent.**

Ordinary host `presentation.rs:528–555` renders actual transparent GPUI Button wrappers with labels, original focus handles and `on_a11y_action(AccessibleAction::Click, ...)`. The immutable accessible snapshot is closed over at render time. Full-inset geometry is not a reason to replace the listener with a synthetic pointer click.

Resolved public platform seam:

1. Fast `platform.rs:1000–1006` exposes `A11yCallbacks { activation, action, deactivation }`; `PlatformWindow::a11y_init`/`a11y_tree_update` at `1404–1408` are adapter hooks.
2. Fast `window.rs:1705–1731` installs the real callbacks. Activation sets the active flag and schedules a refreshed tree; action enqueues the actual `accesskit::ActionRequest`. `1761–1767` delivers it to the original `Window::handle_a11y_action`.
3. `handle_a11y_action:7098–7115` finds listeners by actual target node id and action, calls the registered ordinary wrapper listener and returns when matched. Its later coordinate-click fallback is **not** the required proof of this listener route.
4. Real `gpui-fast-macos-0.1.9/src/window.rs:2393–2420` installs `accesskit_macos::SubclassingAdapter`; `2433–2444` forwards native activation/action to those same callbacks. Therefore an authorized native host run can use macOS AX tree discovery followed by `AXUIElementPerformAction(..., kAXPressAction)` on each actual Retry/diagnostics Button, driving the unchanged dispatcher. No Fast patch is needed. This route has not been run here; OS trust, selected process/window and tree availability must be verified, not assumed from historical GPUI AX limitations.

Why current mounted runner cannot do it: `TestAppContext::build_with_text_system` (Fast `app/test_context.rs:142–171`) always builds `TestPlatform`; `platform/test/platform.rs:558–575` always opens `TestWindow`. That window does not override the default no-op AX adapter hooks, so callbacks are discarded. Real fonts do not make this a native window. `handle_a11y_action` is crate-private; tree inspection/listener registration is not dispatch. Neither `simulate_keystrokes`, calling `Host::activate`, nor invoking a saved NSH press closure closes this gap.

If parent requires deterministic in-process AX dispatch rather than native AX: the public `Application::with_platform` (Fast `app.rs:178–185`) permits an application-level Platform/PlatformWindow adapter that retains the **real supplied** `A11yCallbacks`, publishes/observes actual TreeUpdates, activates them and forwards a real node-targeted ActionRequest through `callbacks.action`. This is feasible API-wise without changing Fast, but is a separate harness integration, not a drop-in `TestAppContext` option; its platform builder is private. Do not start adapter implementation or add dependencies under this diagnosis authorization.

Required positive AX evidence: active actual tree, current node selected by role/label within the correct mounted group, one native AXPress or retained platform ActionRequest per control, one exact original Retry payload, exact displayed-attempt diagnostic clipboard with no ordinary event, and proof the registered listener path was used. Keep stale-capture rejection evidence separately; numeric node reuse does not itself replay an original closure. Parent must arrange the authorized native route or separately scope the app-level adapter; neither is claimed complete.

## Exact inspected byte identities

SHA-256 taken during this investigation; concurrent parent changes after sampling require a new identity check. Paths starting `src/` in the first table are relative to `crates/vega_ui/`.

| Path | SHA-256 |
| --- | --- |
| `src/conversation_stream/ordinary_submissions/presentation.rs` | `7e18e375d8c6df57191df4809f560e28e6c581805394bddcbfc09e0e51553279` |
| `src/conversation_stream/ordinary_submissions/presentation/mounted_regressions.rs` | `3494fda898aae81d537c119c1089438992754c0dcec7cd96e182480f561db86c` |
| `src/conversation_stream/ordinary_submissions/presentation/pixels.rs` | `3f1e1ef6ece0e0c0d39c00eccc0d50d7ccb1cc2d56cf7bf9fbd3ebf398b13e1d` |
| `src/conversation_stream/ordinary_submissions.rs` | `2aa5e125dd4eee2faed302bafc04d904c0c425fcb9b441382d31ee781c41e130` |
| `src/conversation_stream/render.rs` | `64174d622992fd53901ffa16787a42b15914067b331b648ec40a900b96163712` |
| `src/conversation_stream/turn_placement.rs` | `7c179d54071feb4143e4ab2cd72896931f85a2f0408141f9cb5911b1083dd93e` |
| `tests/nsh_ordinary_controls_integration.rs` | `071bbc2c452b277e64f6b1e634bb62df44f9d7a1fd5210db1caf9712d0b012d6` |
| `crates/vega/src/window/ordinary_send.rs` | `a9ff8dae22190442ea2df816d9867bf5846c32aed5454212d0a07d427a949d74` |
| `crates/vega/src/window/agent.rs` | `1a688363a4e7cb574af0bc798d736f8d8fd0ce73c6b89960456ce7ecb94414c9` |
| `crates/vega/src/window/native_subagents.rs` | `7710a8698d3f7631da387a977d81f969c57b6523da32994cd82a0dfdeb5f80cd` |

Resolved registry root: `/Users/puzige/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`. Read-only dependency evidence, not proposed edits:

| Registry-relative path | SHA-256 |
| --- | --- |
| `gpui-fast-0.1.9/src/elements/list.rs` | `5f13980f79d9c7ae2e330418fc42bcd2cd047682a26af4e1269a06d354ffb7f7` |
| `gpui-fast-0.1.9/src/window.rs` | `651325f0c518596012292bbb5e635445a367705edfab27f001bb8d41eee2d893` |
| `gpui-fast-0.1.9/src/platform.rs` | `cb761f9a34e1c21edca910ae7d5dcf9d59f59c16ffe864ded7a2a4b005e5321f` |
| `gpui-fast-0.1.9/src/platform/test/window.rs` | `3b359c9e7943ccb065096c718756b32400dac3394bfd9ab6f361dda8163e97bd` |
| `gpui-fast-0.1.9/src/platform/test/platform.rs` | `4692cb57df966235f6ba8d702c9dba044bff6a4dce6ea7bad61de0c212729011` |
| `gpui-fast-0.1.9/src/app/test_context.rs` | `73b6a83396db51e73cc7e0dcd9ecfe60b0a79940bb49084ba54d5c1e71d2e476` |
| `gpui-fast-macos-0.1.9/src/window.rs` | `7f873d2dd2a79458b277c638760166e3e90f4eb41318a6ffcdab0c1343e3df0e` |

Parent artifact SHA-256: execution JSON `37d1ae616bada63357b3d498d513ecd3d8a08bb421acd2e1087f7a4133724b12`; backtrace `5fa6d5a7beb83a85114dc1ef453ba3867a638dd0922fe19a718d9199ba6dfe12`. No claim that historical executables contain later repairs.

**Prior-stage READY — diagnosis document only at that handoff.** The followup below supersedes its write ownership, production-repair status and next verification. No independent-review or acceptance result was supplied.

## Followup — bounded readiness observation and primary-failure-preserving teardown

### Current evidence and authorized contract, before implementation

The parent repaired the ordinary prepaint borrow site. Current `ordinary_submissions/presentation.rs` SHA-256 is `e04f6b636d10ac40e4bfbe60dddd23323f9e02c3e3aa4c1f99f080b126803bac`: prepaint revokes the mount geometry and records previous geometry/bounds/mask; paint samples exact current viewport/row and notifies only on actual geometry delta. Finding 1 above is historical, not a currently unresolved prepaint call site.

Parent-provided `target/nsh-closure-footer-notify-execution.json` reports the footer owner regression GREEN and the UI suite GREEN. Its ordinary runner exited 1 after 13.922 seconds, without watchdog timeout: timestamp passed, first ordinary case failed at the original `mounted_regressions.rs:202` readiness deadline, and two ordinary cases were not executed. The retained stderr confirms first-frame execution now returns and reports `controls timeout: production NSH displayed/layout/host convergence`; it contains no readiness predicate values. Parent reports the build completed exit 0 in 20.49 seconds. None of those results validates this followup's future bytes.

This followup authorizes writes only to ordinary `presentation.rs`, `presentation/mounted_regressions.rs` and this note; all other source is read-only. Parent owns Cargo/build/test/clippy/evidence collection. No Git, network, vendor, dependency, shared-source or budget changes are authorized. No other writer owns these three paths during this stage.

Classification now: **observed readiness failure, responsible production-versus-observation layer not yet established**. Static source alone does not identify which conjunct remains false. Do not relax `Host::valid`, substitute current authority into captured callbacks, mark displayed from a helper, or change the native text/layout requirements to obtain a pass.

Implementation contract:

- Add one bounded diagnostic snapshot only when the existing 10-second wait assertion fails, not per frame. Sample current host ownership/live lease, actual debug bounds, snapshot/live geometry, displayed/valid/source/current row/viewport/foreground/active-window predicates, native root dimensions and height agreement, each expected text/button observation and Surface error state. Missing state must be reported as missing, not manufactured or treated as success.
- Leave the readiness closure and all behavioral assertions intact, including old immutable snapshots, dispatch/focus/clipboard, clipping, raw image metadata, original 2-second/512-tick drains and 10-second waits. Diagnostics only observe; they are not authority or AX evidence.
- Apply the existing timestamp fixture's fallible `update_window` cleanup pattern to ordinary teardown. A cleanup error still fails normal execution; during an already-unwinding failure it is reported without a second panic. Stop mounted hosts and remove the window; retain normal bounded drain checks, but do not drain while unwinding and replace the primary failure with a scheduler panic.
- No production behavior repair until predicate evidence establishes the responsible layer, unless a concrete cause is statically demonstrated. Initial handoff is diagnostic-only plus the scoped teardown observation repair; parent then rebuilds/runs the unchanged four-case runner.

### Implemented delta and static verification

Only `ordinary_submissions/presentation/mounted_regressions.rs` and this note changed in the followup. Production `presentation.rs` remains byte-identical to the parent's repaired version.

- `wait:202–206` lazily appends `readiness_summary()` to the existing timeout assertion. No summary is collected or emitted on a successful frame. The original failure reason remains present.
- `readiness_summary:230–371` reports six bounded lines for an owned host: current-host/ownership and actual debug bounds; displayed/valid/enabled/source/submission/window/width/height; snapshot/live geometry and equality; live row/viewport/foreground/visibility and host-height agreement; current native root dimensions, per-expected-text/per-button observation booleans and current/total observation counts; explicit Surface existence/health plus at most 256 error characters. Snapshot/live geometry tuples are ordered `(bounds, visible, viewport, row, foreground)`. Text booleans follow the original message/detail/button-label iterator order; button entries include the actual action. If there is no live current host, the latest owned host is diagnostic-only fallback and `host_current=false`; no owned host produces explicit unavailable state. No authority, source, geometry or displayed state is modified.
- `Drop:563–582` uses the same fallible window-update and unwind handling as the current timestamp fixture. A normal cleanup failure still panics. An error returned during unwind is reported; successful normal cleanup still executes the original bounded drain. No blanket panic-catching or failed-assertion suppression was added.

Static verification used a read-only Python script to reverse exactly the three intended hunks in memory. Its reconstructed preimage SHA-256 equals the initial fixture hash `3494fda898aae81d537c119c1089438992754c0dcec7cd96e182480f561db86c`, proving the rest of the source, including the full original readiness closure, focus/click/payload/clipboard/stale assertions and 2-second/512-tick/10-second budgets, is unchanged. This is source comparison, **not Rust compilation or behavior validation**.

Exact fixture delta: **156 added lines, 3 removed lines**, unified hunks `@@ -201,7 +201,8 @@`, `@@ -223,6 +224,149 @@`, `@@ -419,13 +563,22 @@`. SHA-256 of Python `difflib.unified_diff` output using `splitlines(True)`, headers `before/mounted_regressions.rs` and `after/mounted_regressions.rs`: `83fe7cb941c9523ec0f8f281ded1ec408e7ba1ea9bfa08d6473bc2784cfa8ece`.

| Current path | SHA-256 |
| --- | --- |
| `crates/vega_ui/src/conversation_stream/ordinary_submissions/presentation.rs` (unchanged) | `e04f6b636d10ac40e4bfbe60dddd23323f9e02c3e3aa4c1f99f080b126803bac` |
| `crates/vega_ui/src/conversation_stream/ordinary_submissions/presentation/mounted_regressions.rs` | `6ddef036a37e5586ef74ae2c09e08800dd4e2ff050ebf97aee5910ef8e80dc68` |
| `crates/vega_ui/src/conversation_stream/timestamps/presentation/mounted_regressions.rs` (read-only pattern reference) | `7834cb392c864aeba8f865b97d008a0dfb8cad43872cc771593edf50cb83e666` |
| `target/nsh-closure-footer-notify-execution.json` (historical execution evidence) | `6b56b41e89bd9811f73e5605670113431810b2f19543b493759318f2e6fb7826` |

### Next feasible verification and stop

Parent: centrally compile/format-check these final bytes and execute the original `nsh_ordinary_controls_integration` target with all four cases, unchanged deadlines and fail-fast accounting. Preserve the first failure and its new summary. If the wait still fails, identify the false conjunct from actual values before choosing a production fix versus observation fix. In particular, missing native observation on a new presentation id is not by itself proof of Surface failure; `displayed=false` is not permission to bypass its real acknowledgement. Check exact snapshot/live geometry, source currency, Surface error and current native observations together.

Full four-case GREEN remains a goal, **not a result obtained here**; two ordinary cases remain unexecuted in the latest available parent run. Positive AX listener dispatch is still separate from direct stale-callback helper checks. No Cargo, rustc, formatter, build, test, application, AX, Git or network execution occurred in this followup. No independent review, acceptance, CI or merge result is claimed.

**READY — followup writes stopped.** Parent may rebuild/run this diagnostic-only source delta plus primary-failure-preserving teardown, then return the actual readiness summary for the minimal responsible-layer repair.

## Followup — concrete text observation identity mismatch

This section supersedes the preceding unknown-responsible-layer status. Current authorization permits changes only to ordinary `presentation.rs`, its `mounted_regressions.rs` and this diagnosis; vendor, `presentation/pixels.rs`, shared modules, dependencies, Git/network and all builds/runs remain outside this writer's authorization. Only this document changed in this followup. Parent's prior fetch remains the supplied baseline; no fetch was repeated.

### Actual parent execution, not a new run

Inspected `target/nsh-closure-ordinary-predicates-execution.json`, the bounded failure tail of `.stderr`, runner summary in `.stdout`, and the build JSON's final `build-finished` record. Build success is recorded; parent reports exit 0 / 27.77 seconds. Executable SHA-256 is `892b8b2b8701df09cf1eca9f27259b3ee98239fc4a0a40580462f88607d5fb4d`; execution exited 1 in 13.406 seconds, no watchdog timeout. Timestamp passed; first ordinary case failed; two ordinary cases were not executed.

At the unchanged readiness deadline, **only the three projection-keyed text checks were false**: `text_matches=[false,false,false]`, `current_texts=0`, `total_texts=3`. Host/source/enabled/displayed/valid/active checks were true. Width was 736 and measured height 31; current native root was `[0,0,736,31]`, with matching GPUI bounds/height, matching snapshot/live geometry, all live-layout checks true, both Retry/Diagnostics observations current and Surface healthy. There were three current layout observations and nine total. This is not a scheduler hang or evidence of missing button layout.

### Responsible contract and limits of the evidence

**Established observation defect:** the fixture requires a change-driven layout event to recur for every authority snapshot. That is not NSH's event contract. This explains why the existing observation cannot establish current text; it does **not**, without a backend observation, prove all three current text nodes are actually painted or rule out a second production defect.

Exact current source trace:

1. `ordinary_submissions/presentation.rs:499–506` creates a new `ProjectionId` whenever `Snapshot::same` differs, including current geometry/selection. `:509–512` publishes to the retained Surface. The new authority id is intentional and must not be stabilized merely to satisfy the fixture.
2. `presentation/pixels.rs:13–15` reconstructs the dynamic presentation's handlers from the published snapshot. `:102–121` binds `TextLayout` to that snapshot id and text string. It passes neither a native node id nor a native mount identity to the recorder. `mounted_regressions.rs:17–29` stores only `(ProjectionId, String)` after nonempty lines. Readiness `:392–404` consequently cannot associate retained native text with a replacement authority snapshot.
3. `vendor/nsh-core/src/core/mod.rs:5630–5635` preserves an equal attributed text/layout context. `:6520–6536` skips equal content bounds, reuses layout for an origin-only move, and emits `TextLayout` only on the branch doing a new `engine.layout`. Metadata-only remount `:6478–6485` also does not emit it. There is no promise of one event per handler replacement, published snapshot or frame.
4. The actual backend applies `MountItem::Text` at `vendor/nsh-platform-gpui/src/mount.rs:223–235`, installing retained payload/layout through `crate::text::set`. That current native state, not an old event tagged with a new projection id, is the appropriate observation source. No actual node ids or identity continuity were obtained in this followup; do not present numeric-node reuse or cross-snapshot retained text as a runtime-established fact here.

Rejected fixes: copying old text records to the current id; matching strings anywhere in historical records; clearing observations after the only event; inventing node continuity from equal strings; changing `Host::valid`/`Snapshot::same`; forcing a production text/style/layout change or remount solely to trigger the fixture; reducing the three-text requirement. None proves all expected text is currently rendered. The prior primary-failure-preserving teardown remains unchanged.

### Concrete scope blocker and minimal next implementation

There is **no existing exported nonselectable current-text observation seam** in the inspected backend. `AppView::surface` is `pub(crate)` (`app.rs:663`); the view graph, `ViewState::text` and retained `DrawnText` are private to that crate. Its actual batch-fed `nsh_testing::agent::Model` exists behind `dev`, but `Surface::set_test_model` is crate-private (`surface.rs:718–724`), not a usable fixture API or an authorized dependency change. Public `AppView::reading_snapshot` is not a workaround: `text.rs:3175–3183` rejects text without `is_selectable()`, and ordinary labels do not opt into selection. Changing product selection for observation would alter behavior. Existing feature-gated scroll/edit/host-regression exports do not expose ordinary current text.

A correct fix therefore needs parent-owned authorization/integration outside these three writable paths; **do not rebuild the unchanged runner expecting this document-only delta to fix it**:

1. Add a development-only accessor under the existing `vega-host-regressions` feature in `vendor/nsh-platform-gpui/src/app/integration_support.rs` (parent or separately authorized writer). Read the actual attached current view graph for that AppView after mount application. Return actual Surface/generation/batch and native view identity, root-relative tree membership, retained text payload, native laid-out bounds and nonempty native layout evidence. Validate exact attached view identity, not just numeric node id; detached/stopped/recycled nodes must disappear or invalidate a retained observation. This is an observation-only API; do not mutate layout, selection, display receipts or authority.
2. Tie the current ordinary rendered tree to that observation using a real root/node identity captured by the current renderer, not only the existence of a Surface. The existing root `ElementRef` in `presentation/pixels.rs:17` offers a real `node()` API, but plumbing that identity requires authorization to that file too (or an accessor that establishes the unique actual current root directly). A current root-layout observation and the existing real Displayed receipt remain required. Do not infer a current subtree by scanning arbitrary numeric ids or by copying an old root/text record to the new id.
3. In the fixture, use each current expected message/detail/button label against the exact attached text descendants of that current native root, with actual nonempty layout and visibility. Keep all three current text checks, multiplicity, and all existing root/button/height/source/host/Surface predicates. Historical `TextLayout` records may stay diagnostic-only; they must not supply new authority. Parent should verify accessor retirement on unmount/replacement/node reuse and that changed or missing actual text still fails. This is a proposed contract, **not implemented or compile-checked**.
4. Parent centrally rebuilds and runs the original four-case runner with unchanged 2-second/512-tick/10-second budgets and all focus/click/exact original payload/clipboard/stale snapshot/raw image metadata assertions. Preserve first-failure accounting. A subsequent failure identifies the next responsible layer; no all-four GREEN is claimed now. AX listener activation remains separately unverified and must not be renamed from helper invocation.

### Exact current identities and stop

All identities below are SHA-256 of inspected disk bytes; no Git command was used. Production and fixture have **zero source delta in this followup**.

| Path | SHA-256 |
| --- | --- |
| `crates/vega_ui/src/conversation_stream/ordinary_submissions/presentation.rs` | `e04f6b636d10ac40e4bfbe60dddd23323f9e02c3e3aa4c1f99f080b126803bac` |
| `crates/vega_ui/src/conversation_stream/ordinary_submissions/presentation/mounted_regressions.rs` | `6ddef036a37e5586ef74ae2c09e08800dd4e2ff050ebf97aee5910ef8e80dc68` |
| `crates/vega_ui/src/conversation_stream/ordinary_submissions/presentation/pixels.rs` (read-only) | `3f1e1ef6ece0e0c0d39c00eccc0d50d7ccb1cc2d56cf7bf9fbd3ebf398b13e1d` |
| `vendor/nsh-core/src/core/mod.rs` (read-only) | `b28c31c11f803ea27c1e4355100f10efcb248c7881ac46536f704449097f28a0` |
| `vendor/nsh-platform-gpui/src/app.rs` (read-only) | `7d73eda709a7ab2cafa23384c35fedb33935374ad9972b3b8ba25be1e9eead6f` |
| `vendor/nsh-platform-gpui/src/surface.rs` (read-only) | `2f182c026fe4a4b98380fa2f323c9378ba121d5a2e57ded0f1c311c251a5b712` |
| `vendor/nsh-platform-gpui/src/mount.rs` (read-only) | `4b558951ee8c5d5729df71b2712ca89b62b41806dd227bc0dcb58a0187095f2f` |
| `vendor/nsh-platform-gpui/src/text.rs` (read-only) | `91d6ea7c2737175abb4eefcc8d4c75641479b47e897f9f3dc6898b68bceea784` |
| `target/nsh-closure-ordinary-predicates-execution.json` | `6bfe00459e18b1fb4111c014441e10baa64f0750b2d2d7aa2dbd366cf457664e` |
| `target/nsh-closure-ordinary-predicates.stderr` | `375b96f2b06f24016ed08972733031d762d17e6a9ed89af5bcb6afa65a89707a` |

**READY — writes stopped with an observation-API scope blocker.** No Cargo, rustc, formatter, build, test, application, AX, Git or network operation was run in this followup. This is author diagnosis and a concrete next implementation contract, not independent review, acceptance, production failure exoneration or completion. All four cases remain pending a corrected current-native-text observation and central execution.

## Current-surface observation wiring followup

The user subsequently authorized edits only to the ordinary mounted fixture and this diagnosis (pixels only if necessary). The parent-provided backend observation API is now available under the existing development feature; its contract is [current native text observation](nsh-current-text-observation.md). The earlier API scope blocker is superseded, not evidence of a successful mounted run.

Implementation contract before fixture edits: obtain a fresh `integration_text_observation()` from the exact current host's `Surface.entity().read(cx)` on each readiness decision. Require exact expected message/detail/button text multiplicities, the same total text count, nonempty actual backend lines, finite positive frame/content dimensions, finite nonnegative line widths and positive line heights, with at least one positive-width line for each expected nonempty label. Do not require `TextLine.text`: the backend intentionally leaves it empty and reports full text separately. Preserve displayed/valid/source/current geometry/root size/button/error checks and all original budgets and behavior assertions. Historical TextLayout events remain diagnostic-only, with no reassignment to a new projection.

Regression additions stay inside the existing three ordinary mounted cases (plus the unchanged timestamp case): check real current observations, reject unavailable/empty/missing observations, wrong text and duplicate multiplicity; extend the existing unmount scenario through explicit stop and real remount, retaining the retired AppView only to verify it remains unavailable while the replacement surface has current expected text. No detached historical record supplies authority. The API is instantaneous and its node ids are diagnostic; these checks do not certify painting, AX activation, cross-snapshot identity or ABA. Existing application-error handling remains independently required. No production renderer change is made, and no acceptance change is made.

### Implemented delta and verification handoff

Only `presentation/mounted_regressions.rs` and this document were written. Production `presentation.rs`, `pixels.rs`, vendor and the four-case runner remain unchanged. `TEXTS`/`record_text` and its gated pixels hook stay as historical diagnostic side evidence; readiness never consults them. The current host Surface supplies all current text authority. Deadline-only diagnostics now include availability, exact multiplicity, expected strings and bounded actual native observations (first 16 nodes, first 256 characters per text, line count, first 4 width/height metrics, frame/content and layout-valid predicate). There is no per-frame logging.

The first ordinary case adds rejection checks against copied actual observations: None/empty/missing entries, equal-count duplicate text replacing a distinct label, wrong full text, absent lines and nonfinite/zero frame width. These deliberately invalid local copies never enter the live tree or supply readiness authority. The final ordinary case preserves its original stale unmount rejection before explicitly stopping the host, checks both missing host Surface and stopped AppView return None, then remounts through the actual root renderer, requires a different Surface entity and reacquires real current text while the retired Surface remains unavailable. This exercises stop/replacement, not hidden-ancestor, busy-borrow, numeric-node ABA or painted-output coverage. The existing retry case still requires real new-attempt reprojection and exact diagnostic clipboard evidence. The fallible Drop already preserves the primary failure; it was left unchanged.

Executed static checks only:

- Direct `rustfmt --edition 2024 --check --config skip_children=true` on the fixture, exit 0; `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`. No Cargo invocation.
- Python static policy/budget/readiness checks, exit 0: no comments/suppressions/ignore, clean whitespace/newline, original 2-second/512-tick/10-second literals and non-text readiness gates, exactly three ordinary exported cases, and no historical TEXTS access in readiness. These are not behavioral test results.
- SHA-256 comparison against the exact parent-provided production/pixels/backend API bytes, matching all five inspected identities. Readback of the unchanged runner confirms the original four names and first-failure accounting.
- Read-only discovery had incorrect guessed example/bin paths and a shell glob miss; corrected by reading the explicit `[[test]]` target and actual `crates/vega_ui/tests/nsh_ordinary_controls_integration.rs`. No build or test was attempted as part of that discovery.

Final source SHA-256:

| Path | SHA-256 |
| --- | --- |
| `crates/vega_ui/src/conversation_stream/ordinary_submissions/presentation/mounted_regressions.rs` | `c197eaaaccbd3127377de40e4b7079519b043b2e381e13e47eb6639a7691ca7b` |
| `crates/vega_ui/src/conversation_stream/ordinary_submissions/presentation.rs` (unchanged) | `e04f6b636d10ac40e4bfbe60dddd23323f9e02c3e3aa4c1f99f080b126803bac` |
| `crates/vega_ui/src/conversation_stream/ordinary_submissions/presentation/pixels.rs` (unchanged) | `3f1e1ef6ece0e0c0d39c00eccc0d50d7ccb1cc2d56cf7bf9fbd3ebf398b13e1d` |
| `crates/vega_ui/tests/nsh_ordinary_controls_integration.rs` (read-only runner) | `071bbc2c452b277e64f6b1e634bb62df44f9d7a1fd5210db1caf9712d0b012d6` |

Backend identities are the unchanged `d9a90d8d…` text support, `ad96f234…` integration support and `a509bd21…` lib export from the linked backend document. The final diagnosis hash is supplied externally to avoid recursive self-hashing.

**READY — implementation writes stopped after final readback/hashing.** Parent must centrally compile/lint the combined new backend and fixture, then run `nsh_ordinary_controls_integration` with its existing feature/runner and all four original cases, preserving original payload, focus, click, clipboard, stale snapshot, clipping and raw image metadata regressions elsewhere. The new rejection assertions and stop/remount path are defined but unexecuted. No Cargo/rustc/build/test/application/AX/Git/network was run by this lane; no all-four GREEN, independent review, acceptance or completion claim. The latest actual mounted result remains the earlier 1 passed / 1 failed / 2 not executed until parent supplies current-cycle evidence.

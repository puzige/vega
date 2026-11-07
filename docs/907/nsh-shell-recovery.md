# Shell recovery NSH migration

## Scope and authority

Implementation baseline: `b301b64e00690365087362ba21ec45a96f35400f`, with upstream merge supplied by the parent. Owned writes are `conversation_stream/shell_recovery.rs`, its new child directory, and this document only. No Git mutation, build, application execution, shared focus protocol changes, dependencies, or shell operation execution is authorized. The full migration contract is `nsh-full.md`.

The existing `ShellRecoveryAction`, `ShellRecoveryRequested`, `ShellRecoveryOutcome`, request/finish/refresh controller, sequence checks, source checks, operation identity, submitted decision and background refresh lifetime remain authoritative. NSH receives nonsecret presentation data and opaque observation identities, never `ShellRecoveryConfirmation`, its decision token, an Entity or an Rc lease. The host resolves acceptance against the exact live confirmation only after fresh validation.

## Acceptance matrix (before implementation)

| Scenario | Required observable behavior / rejection |
| --- | --- |
| Active commands | Preserve each original status label; Cancel enabled only for that displayed running active operation and while not pending. |
| Paused collapsed | Preserve danger/paused status and Review. Review opens the original controller and calls its original refresh path. |
| Open projection | Preserve workspace, every command or numbered empty-command fallback, exact exit/unknown result, translated deduplicated reason mapping, incomplete-output and unsettled warnings. |
| Check / retry / prepare | Dispatch the original action with the current controller projection; preserve original per-row availability; pending rejects all business actions. |
| Confirmation | Preserve title/body/output warning and accept/keep-paused; safe Keep Paused is the initial confirmation selection; no invisible default Resume; acceptance resolves only the exact live host-owned confirmation, once. |
| Dismiss / Escape | Original dismissal and decision-submitted behavior retained; focus returns to Review; original parent Escape route stays valid. |
| Empty / error | Original empty panel suppression, error mapping and Read-again behavior; failure remains visible and does not imply success. |
| Stale authority | Reject old surface/session, source thread/project/backend, sequence, projection identity, confirmation identity, opened/pending/error/submitted state, removed/disabled operation and stale keyboard selection. |
| Refresh lifetime | Hiding or rebuilding the Surface does not cancel controller refresh or automatically replay a business request. |
| Native keyboard | A real GPUI focus handle owns the leaf host; Tab/Shift-Tab traverse enabled displayed controls in order; held Enter/Space never submit; pointer selection, visible NSH ring and AX active descendant refer to the same observed action; Escape remains with parent. |
| Scroll reveal | Keyboard-selected controls are revealed with supported NSH measure/scroll APIs and current-selection fences, not on every snapshot; scrolling never moves native focus asynchronously. |
| UI failure / reload | Independent NSH recovery presentation retains explicit recovery/dismiss controls as applicable; reload replaces display identity only, never retries or resumes business work. |
| Styling / i18n | Existing theme and Typography/Layout tokens and all original translated recovery copy; NSH draws every product pixel, no legacy view inside native(). |

## Implementation plan

1. Preserve controller APIs and business branches, add opaque projection/confirmation observation identities and an optional presentation owned by `ShellRecoveryUi`.
2. Add a child NSH module with Send + Sync snapshots, ordered controls and a single production acceptance policy. All mouse, keyboard and AX activation uses that policy before the original controller methods.
3. Render existing content in a measured NSH scroll viewport with an invisible GPUI native focus/AX host only. Safe stage selection and observed scroll reveal must not authorize or replay requests.
4. Retain original refresh tasks independently of presentation. Describe the exact parent render/hide hooks without writing parent files.
5. Add focused development regressions for actual policy, stale authority and safe traversal. Run scoped rustfmt/static checks only; parent owns compilation, Clippy, tests, CI and native verification. Record tests for pre-merge archival/removal.

## Implemented controller and presentation contract

- `shell_recovery.rs` retains all three public event/action types and original dismissal, refresh, request, finish and translated reason functions. The only additions inside those controller functions rotate opaque UI observation identities. A static function-body comparison against `b301b64e00690365087362ba21ec45a96f35400f` confirms equality after removing those identity assignments and whitespace. Original thread/project/sequence guards, command-card updates, error decisions and one-second refresh scheduling are unchanged.
- `nsh_view.rs` contains the Surface, nonsecret projection, displayed ordered action policy, original action dispatch and NSH product renderer. Controller confirmation and decision/authority tokens never enter snapshots or commands. The host clones the exact current confirmation only after the current observation/confirmation identity, source, stage, availability and submitted checks pass.
- `keyboard.rs` binds the original `review_focus` / `safe_focus` to the invisible native host. Focus subscriptions publish actual ownership. No mount, reload, measurement or error callback dispatches a business request. Stage focus transfers synchronously only if the other recovery handle already owns focus; it never takes focus from another product region. Pointer activation selects that same action. Tab/Shift-Tab skip disabled actions in displayed order and return to native focus traversal at boundaries; no held Enter/Space activation. Confirmation defaults to Keep Paused, not Accept. AX active descendant uses the selected name and the same guarded action, with an additional current-selection check.
- `reveal.rs` uses pinned NSH 0.4 `watch_scroll`, `ElementRef::measure` and `scroll_to`. Measurement.page is viewport-relative page geometry; scroll.offset is the positive content offset. The destination adds only the missing top/bottom distance and clamps to content minus viewport size. A visible control produces no scroll. It runs once for a keyboard selection identity, revalidates through the host, and checks unchanged scroll observation before issuing the command. After a selected target is mounted/measured (and any requested scroll applied), it acknowledges that exact observed selection. Native Enter/Space and AX activation require that acknowledgment, not merely a newly published host snapshot. No asynchronous native focus calls exist.
- Surface error switches to a new independent NSH recovery Surface, invalidating the old Surface identity. It retains original active statuses and eligible cancellation, pending/error/paused feedback, safe dismiss or review/read as applicable, and an explicit UI Retry. UI Retry destroys only presentation; it does not retry Check/Prepare/Resume or cancel the controller refresh task. Recovery never offers Accept/Prepare. All original detailed content returns on explicit UI reload.
- Every status/warning/command/result/control pixel is NSH. GPUI only sizes/embeds the Surface and owns native focus, keyboard capture and an invisible AX active descendant. Content height is NSH-measured, capped by `Layout::TOOL_GROUP_MAX_HEIGHT` with a local NSH viewport. Theme, language and typography use existing APIs/tokens. No new IO, dependencies or arbitrary URI handling.

## Exact parent integration (required before compilation)

No edits to parent-owned files were made. Parent must apply:

1. In `conversation_stream/render.rs`, change `render_conversation_foreground(&self, ...)` (baseline line 128) and `render_composer(&self, ...)` (line 275) to `&mut self`. Both callers already run in mutable `Render::render`; no public event API change is needed.
2. Change baseline line 456 from `.child(self.render_shell_recovery(cx))` to `.child(self.render_shell_recovery(window, cx))`. The new method is `pub(in crate::conversation_stream)` with `&mut self, &mut Window, &mut Context<Self>`.
3. At the start of `ConversationStream::render`, before constructing the tree, call `self.hide_shell_recovery_surface()` when `self.reading_preview` is true. This method only drops the presentation, not projection/decision/pending/refresh. Action reception independently rejects reading-preview state even before this hook runs.
4. Call `hide_shell_recovery_surface()` when the containing cached stream is hidden/unmounted, and before source route rebind/backend changes in `core.rs` (`select_task_backend`, `rebind_draft_project`, and `apply_thread` when backend differs). This invalidates Surface sessions even for leave-and-return routes; direct thread/project/backend comparison already rejects cross-source callbacks while changed. Do not reset or cancel `shell_recovery.refresh` as a UI hide operation.
5. Keep the existing Escape dispatch at baseline `render.rs:2000` calling `dismiss_shell_recovery`; no new Escape handler or keybinding is required. No new `core.rs` fields or initialization are needed; `ShellRecoveryUi::new` initializes its own presentation and opaque identities.

The source-rebind hook names describe the current baseline. Existing source projection behavior remains parent-owned; this migration does not change how the controller refreshes a new source.

## Validation evidence and limits

Executed on the final owned Rust source:

```text
rustfmt --edition 2024 --check <shell_recovery.rs and four child Rust files>
exit 0

git diff --check -- <owned paths>
exit 0

static controller function comparison against b301b64e00690365087362ba21ec45a96f35400f
  dismiss_shell_recovery: unchanged except opaque UI identity rotations
  refresh_shell_recovery: unchanged except opaque UI identity rotations
  request_shell_recovery: unchanged except opaque UI identity rotations
  finish_shell_recovery: unchanged except opaque UI identity rotations
  recovery_reason: unchanged except opaque UI identity rotations
controller preservation comparison: PASS
```

Pinned registry API inspection confirms ordinary View is not a keyboard stop (`nsh-platform-gpui-0.4.0/src/input.rs:1114`, only Input/Native); View focus resolves through input state (`text_input.rs:1757`). The bridge therefore uses actual native host FocusHandle ownership rather than `view().focusable(true)` as a substitute. Core `element_ref.rs:183/289` supplies scroll/measurement; `ambient.rs:1143` documents initial and changed scroll observations. These are API/static facts, not native runtime evidence.

Twelve focused development tests in `shell_recovery/tests.rs` exercise the production action/selection/reveal policy: review/check/retry/prepare/confirmation eligibility; pending/submitted single-submit; stale session/source/sequence/projection/confirmation; row availability; exact running cancel identity; safe ordered disabled-skip traversal; UI recovery without implicit resume; empty/error behavior; current-selection reveal; scroll geometry; measured-selection rejection; native traversal boundaries. **All are unrun here.** Before merge, archive their exact source/hash and actual parent validation results, then remove `tests.rs` and the exclusive `#[cfg(test)] #[path = "tests.rs"] mod tests;` declaration in `nsh_view.rs`. All other modules are production code.

No build, compile, Clippy, Rust tests, CI polling, app execution, installation or native handtest was performed. Parent-supplied baseline passes are not evidence for these changes. Required next checks are parent integration, coordinated compilation/strict Clippy and the 12 new regressions, independent review, then native keyboard/AX/scroll/focus and recovery handtests on supported platforms. The unintegrated parent call currently prevents compilation; do not claim this leaf has compiled. A second independent recovery-engine failure is not native-tested; engine recovery cannot guarantee rendering under a persistent backend failure.

## Pinned signal API compile correction

Acceptance: replace only the three non-PartialEq signal initializers in `nsh_view.rs` with pinned `signal_with(value, SignalEquals::Never)`. Do not add equality or hashing to Snapshot/ProjectionId, change action assertions, authority comparisons, or controller behavior. Pinned `nsh-core-0.4.0/src/element/reactive.rs:501/506` requires PartialEq only for `signal`, not `signal_with`; Never notifies on every write. Existing explicit content-authority/appearance/language filtering remains intact. No builds/tests are authorized for this correction; parent owns compile validation. No Hash use was found in the owned ShellRecovery Rust files.

Correction evidence: scoped rustfmt check passed after the three-line change. The parent compile report identified these three E0277 failures; no compile or tests were rerun here. Cached outgoing stream entries are `crates/vega/src/window/render.rs` in `VegaWindow::render`, both `self.stream_view.take()` branches (observed lines 513 and 584); parent has now inserted `previous.update(... suspend_shell_recovery_presentation())` in both. This is the concrete outgoing cached-stream retention boundary, not an additional leaf change.

## Parent integrated validation (supersedes leaf-only unrun status)

Parent integrated mutable foreground/composer rendering and window-aware recovery mount; reading-preview, project/backend/source rebind invalidation; public presentation-only suspend hook; app settings/opening-thread hiding and both cached outgoing-stream transitions. No controller timer or business operation is canceled by the UI hide hook. Existing Escape dismissal is unchanged.

Stopped integrated source on baseline b301b64e: UI `cargo check -p vega_ui --locked --offline -j 2` exit0/6.03s; workspace all-target `cargo clippy --workspace --all-targets --locked --offline -j 2 -- -D warnings` exit0/7.86s; workspace fmt and diff checks exit0. `cargo nextest run --workspace --locked --offline -j 2`:87 discovered/executed/passed,0 skipped; all twelve Shell regressions executed, run c40ad357-f27d-45c3-89af-65b36f562866, build15.13s/execution0.403s. Toolchain rustc1.98.0, Cargo1.101.0/fork0.102.0-9, Nextest0.9.151. Prior leaf unrun statements describe its isolated handoff, not this integrated result.

Integrated parent SHA-256: core.rs37d9e0cd4da974e497629a3c05848b0725c04cbacfff14a06c2cb0e8a0cb817a; UI render.rsd73bb870a3636d5b77eaf14d2c44a70b7d9eee49523b8137d23bd50576dff89c; app window/render.rsfebd1a8946a63fa1cf54044927baa24add6bf2546fa7df14b100443ab37b2364. Independent Shell/lifecycle review remains pending; exact new committed-head CI and native keyboard/AT/scroll/recovery acceptance remain unrun. No app execution/packaging/install/merge/release.

## Independent review corrections: acceptance before implementation

Scope for this correction is only `shell_recovery/nsh_view.rs`, `reveal.rs`, `tests.rs` and this document. Parent owns the composer early-return hide hook. No build, Git mutation, app execution or delegation is authorized.

| Finding / scenario | Required correction and regression |
| --- | --- |
| Permission UI suppresses recovery before the next render | Fresh action authority rejects `automation_permission.busy()` even if the old Surface still exists. Snapshot presentation eligibility includes reading-preview and permission suppression; queued Accept and every other action fail while suppressed. |
| Leave / return to a new presentation | Hiding drops presentation only, retaining projection, decision, pending and refresh. Old callbacks remain invalid after return because the new Surface identity differs; new displayed confirmation retains safe Keep Paused selection. No confirmation token enters the engine. |
| Missing control / viewport measurement | An attempt is not a Displayed acknowledgement. A bounded fresh asynchronous measurement may succeed without changing the selected control; no business retry or native focus call occurs. |
| Scroll / layout changes during measurement or host validation | Reject the stale observation, then measure fresh geometry within a finite budget. Acknowledgement requires successfully measured current selection and exact current scroll observation; a post-scroll measurement must succeed too. |
| Selection or Surface changes while an attempt is pending | Old completion cannot acknowledge or consume the new selection's measurement. At most one measurement attempt is in flight for the current selection. |
| Repeated invalidation, duplicate effect notifications, host rejection | Distinct in-flight/completed/failure state prevents duplicate measurement and reactive self-trigger spin; finite per-observation and per-selection budgets bound automatic measurement. Host rejection terminates that selection's measurement, never automatically retries an action. |
| Regression preservation | Keep all twelve existing tests/assertions, add focused tests against the production eligibility/attempt policy for hidden queued activation, new-presentation replay, transient measurement and scroll recovery, stale completion, duplicate scheduling and safe caps. Development tests remain pending parent execution and must be archived/removed before merge. |

Implementation: add a nonsecret presentation-eligibility projection plus a direct fresh busy fence; replace the consumed-at-start marker with a production bounded attempt state machine and exact observation checks around asynchronous measure/host validation/scroll. Preserve existing controller bodies, geometry math, native focus bridge and action dispatch.

## Review correction implementation and evidence

P1: `Snapshot.presentation_eligible` is derived from `!reading_preview && !automation_permission.busy()`, participates in visibility and exact authority, and therefore also fences selection/Displayed/Reveal acknowledgement. `accepts_shell` independently checks fresh permission busy state before original action dispatch. Parent's `render_composer` early-return hide call is present on readback (line 277 at inspection). No additional parent hooks are required for this correction. The hide body remains presentation-only; `shell_recovery.rs` and `keyboard.rs` retain their supplied hashes. No controller token or new business operation enters the engine.

P2: production `Attempts` distinguishes in-flight, measured-completed and terminal host-rejected outcomes. It permits two attempts for an unchanged observation and six total for a Surface/selection identity. A changed scroll/layout observation resets the two-attempt allowance, never the six-attempt total. Scroll changes increment a monotonic observation revision (including away-and-back changes); control and viewport layout bounds changes increment that same revision. Each effect reads actual state/scroll/layout plus an asynchronous completion wake, but scheduling mutations use nonreactive `Stored`, so scheduling cannot synchronously retrigger itself. At most one attempt is active for the current selection; old completions cannot release a newer attempt. Tasks belong to the Surface renderer owner rather than a rerunning control effect, preventing an effect rerun from canceling a task and stranding in-flight state.

Measurement checks current Surface/selection/authority and exact scroll+revision after awaits and after host reveal approval. A scroll command is never a Displayed ack: it waits for a layout round, then a fresh bounded measurement must establish visible, finite positive control/viewport geometry with unchanged observation. Only that current measured completion sends Displayed; the host retains its original fresh focus/selection/authority validation. Host rejection is terminal even if focus changes before its response arrives. No delayed native focus, automatic business request, reload/retry/resume or controller timer change is introduced. Completed selections do not unconditionally remeasure/scroll on every snapshot. Exhausted budgets fail closed (no Enter/Space/AX activation); a new user selection/presentation gets a new budget. Persistent measurement failure and actual native scroll/AX behavior still require runtime validation.

Static validation on the final correction: scoped `rustfmt --edition 2024 --check` passed for the three owned Rust files; owned-file trailing-whitespace/conflict-marker/newline checks passed. Original twelve test functions/assertions are byte-for-byte preserved: removing only the new fixture eligibility field and appended tests reproduces original tests SHA-256 `6b5b96676f02225d6a3050d915a717d619d90510f2f7a60a71f1ba8212a041ab`. The first preservation-check script accidentally appended an extra newline; inspecting the prefix identified that observation-script error, and the corrected hash check passed without source/assertion changes. Production files have no unwrap/expect/comments; reveal has no native focus call. No Git command was run for this correction; therefore no current `git diff --check` result is claimed. No build/Clippy/test/app/CI execution was performed. The prior integrated 87-test result does not validate this delta.

Development inventory is now **19 tests: 12 retained + 7 new**, all unrun on this correction. New tests call production `Snapshot` eligibility and `Attempts` transitions, not a duplicate retry algorithm: hidden queued Accept and new-Surface replay; missing measure then fresh success; changed-scroll rejection then fresh success; old selection/Surface completion rejection; duplicate scheduling/unchanged cap plus changed-layout restart; finite total cap; terminal host rejection and hidden completion. Parent must compile/run these regressions, review the asynchronous platform wiring, and archive/remove `tests.rs` plus the exclusive test module declaration before final merge as already required above.

## Final correction validation and independent review

Main executed on the corrected stopped source: workspace fmt/diff checks exit0; `cargo clippy --workspace --all-targets --locked --offline -j 2 -- -D warnings` exit0,8.21s; `cargo nextest run --workspace --locked --offline -j 2` executed/passed94/94,skipped0,execution0.411s. This includes all nineteen Shell policy regressions and all three shared keyboard regressions. The earlier87-test run is historical, not the final correction result. Final UI render.rs SHA-256 is `60ec454302052d001117f126cfe9f6b50a154a1ed15c3ac46bd142d9f2f45645`; unchanged parent core/app hashes remain above.

Independent incremental source review closed P1 and P2 on the final leaf hashes below: the permission early-return retires presentation and fresh suppression gates block callbacks; bounded observation/selection budgets, explicit effect wake, renderer-owned tasks, exact scroll/layout rechecks and fresh post-scroll measurement preserve acknowledgment safety. No unresolved concrete finding in the corrections. Persistent measurement failure or budget exhaustion intentionally fails closed; native interaction and assistive-technology behavior remain unverified. No app/packaging/install/merge/release. Exact new-head cloud CI is still required after committing this development stage; tests still must be archived/removed before final merge.

## Owned Rust content hashes (SHA-256)

```text
d51aa2c69048f60995a0f7a865dd9fd0a507a71f504c017fdf755d3b659df901  shell_recovery.rs
d2a22d85c38421d61f0ec4fe6b1030b1297bd31644c1b39343f36f161f373f3b  shell_recovery/nsh_view.rs
c48f7811bc55c1d017cd4ce05d1fadb181ef7a6f14edde3b3e801661b6fcebaa  shell_recovery/keyboard.rs
5761f0304ec55960a9c89653319e9918fcc417ddc7dac601197eb6ef15d46e87  shell_recovery/reveal.rs
c9c74dcb753d2399dbc6029a274303cf2a892c9c10f81866878f81fd3ec50a04  shell_recovery/tests.rs
```

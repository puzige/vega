# CUA runtime notice — NSH leaf migration

## Scope and acceptance (before implementation)

Baseline: `bd89db7d182e6d9a5c60b1388e79a4623e8cacc4` plus other writers' uncommitted integration changes. This writer owns only `conversation_stream/computer_use.rs`, its new `computer_use/` children, and this document. No parent, shared bridge, dependency, Git/index, onboarding, composer, thinking or preparation-error changes.

| State | Required visible NSH content | Existing trusted operation |
| --- | --- | --- |
| Running with active agent message | Take over computer control | `pause_at(observed_epoch)` |
| AwaitingApproval with browser target | Existing browser confirmation, exact target label, Allow this tab | `approve_browser(observed_epoch, observed_target)` |
| Paused | Existing paused guidance and Continue | `resume(observed_epoch)` |
| Pausing | Returning control, without an action | None |
| Other phases | No runtime notice | None |

All visible text, action surfaces, hover/pressed/focus pixels and layout belong to NSH. GPUI retains only the Surface host, actual focus/Tab/Shift-Tab/Enter/Space dispatch, paint-free accessibility nodes and mount/geometry tracking. Keep existing translations and theme/layout/typography tokens. Buttons do not grant assistant tool authorization; approval/resume do not replay unknown outcomes, initiate assistant actions or relax permissions.

## Dispatch invariants

- Capture the source only in actual native rendering, not when constructing a deferred element. Retain the exact `ComputerUseControl` instance, `TaskContext`, thread, phase, runtime epoch, browser target/capability/target label, control generation, active-message identity and permission mode.
- Read the parent's `run_notice_epoch` as the stream presentation epoch, without renaming or mutating it. The parent rotates it synchronously at the start of `suspend_ui_presentations`; a callback captured before suspension must remain invalid even after showing the same thread again.
- Every pointer, keyboard and accessibility activation uses one main-thread dispatcher. It rechecks the source against the current owner and rejects replaced controls/tasks, changed thread/phase/epoch/target/generation/active message, Settings, reading preview, revoked/pending permissions, inactive routes/windows, missing/recycled mounts, stale viewport/footer geometry and unavailable Surface state.
- Retain only a weak mount lease in callbacks/controller state. A retained native element-state lease may represent the current rendered mount; removed/recycled mounts cannot authorize old commands. Presentation changes rotate the local projection identity and stop the prior Surface, not the runtime control.
- Keyboard selection belongs to a presentation identity. A phase/control change while focused must not silently turn Enter into approval or resume. Explicit focus entry or navigation selects the current action. Repeated/modified/composition-preferred activation is ignored using the existing shared keyboard policy; no keymap or IME bypass.
- Keep original control attachment/detachment/task ownership and takeover invalidation. Runtime/onboarding/native CUA hosting remain their existing owners.

## Verification and integration boundaries

This assignment forbids builds, test execution, network, Git mutation, app launch and nested delegation. The parent owns compilation, regression execution, independent review, final-head CI and native acceptance. Existing 166 passing regressions predate this delta and are not evidence for it. Dependency pins remain NSH 0.4.0, Kit 0.7.3 and Fast 0.1.9; the known pinned-backend IME limitation is not solved here.

Parent verification must cover all four states, exact browser target display, pointer and AX dispatch, native Tab/Shift-Tab/Enter/Space and visible focus, modifier/repeat/composition-preferred rejection, phase changes while focused, control/task/message/generation replacement, hide/show before queued callbacks, route/Settings/preview/permission changes, unmount/recycle and resize/footer geometry changes, narrow widths, long browser labels, and light/dark plus English/Chinese. No native acceptance is claimed by static inspection.

## Implementation and stopped-version evidence

Implemented the leaf-local native `RenderOnce` adapter without changing `render_computer_use_notice(&self, cx)`. `Source::capture` runs in native rendering; trusted dispatch obtains a fresh source for equality checks. `presentation.rs` owns projection identity, native focus/selection, the bounded request consumer and dispatch. `presentation/pixels.rs` draws the entire notice in NSH; `presentation/mounted.rs` delegates host drawing and records clipped bounds, window/list viewport and foreground height. Runtime methods and original attachment/task owners remain unchanged.

Successful dispatch consumes that presentation; duplicate queued commands cannot run it again. Source/enabled/mount changes stop the old Surface, rotate local identity and clear selection. Focus is not automatically reassigned to the new action after a phase change. Old Surface error callbacks are fenced by local identity. Browser confirmation and exact target remain in native accessibility group text; the paint-free AX button uses the same dispatcher. NSH disabled state exposes unavailable controls without misleading hover/pressed feedback.

### Actual checks

- Direct pinned-toolchain formatter: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`. Initial `rustfmt --edition 2024 --check crates/vega_ui/src/conversation_stream/computer_use.rs` parsed the module tree but reported formatting differences. Applied the formatter only to this owned tree; final identical check exited **0**. No Cargo/rustc compilation was run.
- Python read-only inventory of the four Rust files: no `unwrap(`, `expect(`, code comments, `#[test]` or `#[cfg(test)]`; no trailing whitespace; final newlines present. This is a static inventory, not a behavior test. No development tests were added.
- Read existing control implementations and cached NSH 0.4.0 / GPUI Fast 0.1.9 APIs for epoch/target methods, keyed/element state, native focus, layout and disabled semantics. Checked manifest pins without editing manifests or lockfiles. These API inspections do not establish type-check or native acceptance.
- No builds, test execution, Git commands/mutation, network requests, application launch or delegation performed. No claims about other writers' dirty files or whole-worktree validation.

### Stopped Rust source SHA-256

Paths are relative to `crates/vega_ui/src/conversation_stream/`.

| File | SHA-256 |
| --- | --- |
| `computer_use.rs` | `3d9ffd03f08a040cfbbe988c2df096935cc3602f7a4a09dd2b25ca7fe8e17b97` |
| `computer_use/presentation.rs` | `c0cace4acdbda5b0a5c2ad38dd01c6601035000470758c6f907d49279d938c2f` |
| `computer_use/presentation/mounted.rs` | `a735cc81d4d40cf8759e068cfda8ff240dc56630c72c8782b135d4d929a4fd87` |
| `computer_use/presentation/pixels.rs` | `60ad2e40164d0158f99e66103d931ca150b0e93f8104c5bb40dc424cf32d01d6` |

### Development regression addendum

User-provided integration baseline: HEAD `ee3947ac0f17b503e6efb635e0c383cea99576af` plus dirty parent Box fixes, with compilation, all-target strict checks, 166 current regressions and independent static review reported passing before this test delta. Those results are parent-reported, not executed or independently reproduced by this writer, and do not cover these new tests. Earlier source hashes and no-tests inventory above describe the previous stopped migration, not this development addendum.

Only `computer_use/presentation/tests.rs` and the appended `#[cfg(test)] mod tests;` declaration in `computer_use/presentation.rs` change Rust in this addendum. Production behavior and acceptance remain unchanged. Three focused development regressions are added:

- Source identity rejects stale presentation/runtime epochs, replacement control and same-thread task generation, thread/phase/capability changes, each browser identity component, target removal/label changes, control generation, active-message identity/index/removal, permission and language changes. Identical visible browser labels cannot substitute for connection/tab identity; restored content with a fresh presentation epoch cannot revive old authority.
- Actual `pixels::render` mounts inside NSH `Core` / `Component` / `Root`. Produced text and accessibility button semantics cover Running takeover, AwaitingApproval confirmation/exact browser target/allow, Paused guidance/resume, and action-free Pausing. Narrow/light and wide/dark projections cover enabled/disabled states; disabled production callbacks emit no activation.
- Each actionable phase invokes the actual mounted production press callback, queues a replacement projection through an engine-owned task, then inspects the real `Request::Activate` after replacement. Assertions retain the original action, projection identity, exact control/task/thread/phase/epoch/capability/browser target/label/generation/message/permission/language and displayed text/enabled/selection. The retained source does not match the replacement; a removed button cannot emit another activation. Controls are public `ComputerUseControl::new()` instances kept disconnected; sources are synthetic projections, not forged runtime state, controller calls or native acceptance.

All signals are created/read/written by the mounted component or engine task; test code outside the engine uses channels and returned mount batches only. Explicit imports avoid GPUI's `#[test]` macro shadowing. No extra validation gates or acceptance changes are introduced.

Actual local checks: direct `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)` formatting of the new test file and final `rustfmt --edition 2024 --check --config skip_children=true` on the two scoped Rust files exited **0**. Read-only inventory found three test cases, explicit imports, no ignored tests, final newlines and no trailing whitespace. An attempted pre-edit-body hash comparison used an unrecorded reference and was discarded; only the current-byte hashes below are asserted. No builds, test execution, Git commands/mutation, network, application launch or delegation occurred in this addendum.

| Development file | Current SHA-256 |
| --- | --- |
| `computer_use/presentation.rs` | `43cd7c39e5f4973d44a27b569ef93d6fdeb8859b26f1e8218ba8ed506fbd5372` |
| `computer_use/presentation/tests.rs` | `cec47a3230b08e7e7b0f2240261acc2828cae0a40b3bf66cc4da61ee8749494e` |

Compilation, Clippy and execution of these three tests are **NOT RUN** by this writer. Parent will run the CUA presentation test filter serially with the existing regressions after this writer stops; native focus/AX/geometry dispatch and platform acceptance remain separate and unverified here. Before merge, preserve recoverable development test source/patch and actual results, then remove `presentation/tests.rs` and the appended test-only module wiring. These Rust tests and their exclusive fixture must not enter master; final cleanup and existing final-head checks remain parent-owned.

### Concrete integration needs / unverified limits

- Parent must retain fresh stream epoch initialization and synchronous epoch rotation at the start of suspension for every relevant hide/Settings/route lifecycle. This leaf only reads that field; it does not install global lifecycle hooks or alter parent call sites.
- Dispatch intentionally fails closed until the route and Settings globals exist, the window is active, the Surface has no reported error, an actual clipped mount exists and its NSH-measured dimensions match the current native mount. Parent integration must verify first-layout/resize/footer settling and callbacks immediately before/after hide/show or unmount. Geometry acknowledgements are not compositor-visible-pixel proof.
- Native focus/Tab/Shift-Tab/Enter/Space and AX paths are implemented, not executed or accepted. Check focus entry from both directions, pointer focus transfer, exit to neighboring controls, phase transitions while focused, AX confirmation context and action activation. The pinned backend's known IME limitation remains; the existing keyboard event filter is not an IME fix.
- Compilation, Clippy, new-delta regressions, whole-workspace formatting, independent review, final-head required CI, all native pixel/input/AX checks and Windows/Linux acceptance are **NOT RUN** by this writer. Parent coordinates these checks; the previous 166-test result does not cover this leaf delta. Preserve existing assertions and regression scope when validating.
- No native acceptance, merge, commit, installation, release or full-migration completion is claimed.


## Parent integrated validation (current development batch)

Baseline `ee3947ac0f17b503e6efb635e0c383cea99576af` plus this stopped delta. Format, workspace all-target Clippy `-D warnings`, and Nextest run `4eb222ed-ad9e-4ff9-8cd6-1ccea4c84ec8` passed: **175 discovered /175 executed /175 passed /0 skipped** across16binaries; runtime0.294s. Existing Python release/preparation/general groups passed5/15/40. Actual tools: rustc1.98.0, Cargo1.101.0 (huacnlee0.102.0-9), Nextest0.9.151; CI versions/dependency pins unchanged. This is headless development regression evidence, not native focus/AX/IME/pixels or Windows/Linux acceptance. Final exact-head CI is pending. Rust tests and exclusive wiring must be preserved as recoverable development evidence, then removed before master.

The parent boxed the large immutable activation snapshot without changing its fields, then corrected action-task ownership to the stable component scope. Independent static reviews covered both authority and this owner-lifetime delta with no actionable finding. All three new CUA renderer/source regressions executed and passed in the175-test run. Current pixels SHA256 is `317bcafc2660cf6252e7e5ab56865da071db40a3591e1e1b74913c79df049158`; test SHA256 remains `cec47a3230b08e7e7b0f2240261acc2828cae0a40b3bf66cc4da61ee8749494e`. Earlier embedded hashes are historical handoff identities. Native control dispatch and failure presentation remain unaccepted.

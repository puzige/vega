# #307 Stage B card, preparation adapter and host integration

## Delivery state and boundaries

READY for independent review of the content manifest below. This is implementation plus local static evidence, **not** cloud compilation/GREEN, independent review, merge, or native acceptance.

- Worktree: `/Users/puzige/Workspace/worktrees/vega-307-cua-onboarding`, branch `feat/307-cua-onboarding`.
- Observed committed HEAD: `48cef07bce9fdfb8187b2fae08873725b2ac9abf`. Parent supplied the fetch/fast-forward check and merge of documentation-only master `2ba904f7074115de5ee89cc18c376c083f803357` before this writer's changes.
- Replacement writer resumed the preserved card/session/view files at the hashes in `vega-issue-307-stage-b-integration.md`; they were inspected and incrementally completed, not regenerated. No nested agents. No Git/index/commit mutation, fetch, push, CI query, or local application/native execution was performed.
- Written production scope: card/view/session and module wiring, narrow Settings event/save-success wiring, one scoped host module plus host subscription/render/owner fields, and three Layout tokens. This report is the only authored document. Parent's recovery checkpoint and the recovered native implementation note were not modified.
- Runtime, conversation types, UI-A controller/lifecycle/stream/finish integration, frozen tests, native adapter, dependencies and workflows remain unchanged. No acceptance change, test addition/edit/removal, comments, non-test unwrap/expect/panic, or lint suppression.

## Implemented integration

### Card and native boundary

`vega_ui::cua_onboarding` now reexports `open`, `GuidanceCard`, and `OpenError`; private card/native/session modules are compiled through production module wiring. `open(task, Box<dyn Fn(String, &mut App)>, cx)` returns a typed `WindowHandle<GuidanceCard>` result. Settings uses the empty task identity and has no return button or fabricated destination.

The recovered renderer owns the existing `CardController`. Component, accessibility, screen-recording, actual-capture, check and action-enabled projections all come from `CardView`; click and Enter/Space dispatch the same `CardAction`. Explicit button roles/labels, Tab/Shift-Tab, Escape, Finder fallback and a keyboard-focusable scroll body are wired. Component/AX/screen facts remain separate, browser extension is explicitly unassessed, actual capture is unverified, and there is no readiness or automatic-completion reducer in the renderer. Errors are placed before the status rows in the scroll body; heading/close and footer/recheck/return do not scroll out of the card.

`Layout::PREPARATION_CARD_WIDTH = 480`, `PREPARATION_CARD_HEIGHT = 420`, and `PREPARATION_CARD_SHADOW_PADDING = 24` replace renderer-local sizing. Outer dimensions are `480 + 2*24 = 528` and `420 + 2*24 = 468` logical points. The existing read-only placement policy still uses its frozen 24-point allowance and 18-point inset; the theme allowance matches it exactly.

The existing native adapter is consumed without modification: Floating, focus:false, nonmovable/nonresizable/nonminimizable, transparent background/titlebar and offscreen traffic lights; unique retained title marker; configure immediately after creation; 150ms follow; bounded 20-tick initial Settings acquisition; existing `perch`/`follow_target`. A missing Settings frame preserves the last position after acquisition. Configuration failures visibly name focus/Spaces risk, not only movement failure.

Placement uses CG-global screen rectangles paired to the adapter's real display ID. The initial GPUI origin is `(P.x-C.x, P.y-C.y)`; native moves receive CG-global `P`, leaving AppKit conversion and actual frame height to the adapter. No Retina scaling or compensating initial-height subtraction. Static inspection of pinned `gpui-pre-macos 0.3.4` confirms `MacDisplay::bounds()` discards global origin and `visible_bounds()` is display-local; the degraded path therefore uses the **primary display's local** visible bounds and matching primary display ID, disables following, and displays the geometry fault. It does not invent a CG-global secondary-screen origin or feed fallback coordinates to native movement.

Native drag construction calls `begin_drag()` synchronously before returning a preview. The exact returned path becomes `Files(FileDragPaths::new([(bundle, true)]))`. The controller's permanent drag freeze is never reset from GPUI active-drag state or native end/drop assumptions. Development executables get no drag source; Finder targets the actual executable. No drop acceptance, permission change or successful external launch is claimed.

URL/Finder use GPUI's void dispatch API. Recovery text appears only after the action, does not say the application opened, and names manual recovery; a nonexistent Finder path gets a concrete error. Actual Launch Services/Finder failure receipts are unavailable from this API and remain a manual observation boundary.

### Reusable preparation session and cleanup custody

`ProbeClient` keeps the recovered watch/coalescing channel and original `CheckTicket` for each response. One lazy dedicated current-thread Tokio reactor owns the worker JoinSet and cleanup tasks across card destruction. A successful `PreparationProbe` is reused for subsequent checks. The adapter rejects true flags, invokes only `PreparationProbe::open/check/close/cleanup_pending`, and the read-only runtime performs `check_permissions(false, false)` with the same embedded identity as normal desktop use.

Original inspection errors are not replaced by cleanup results. The service retains the last inspection error across reopening; a fresh controller lifetime projects only that error through its own ticket without making a preparation RPC or replaying any old observation. Explicit Recheck is required to recover from that prior failure. Closing while an inspection is pending cancels the per-client token; it does not stop another `ComputerUseControl`.

Cleanup records the direct `close()` error separately from the manager drain outcome. `Clear` is set only after `cleanup_pending()` returns `Ok`; if direct close failed but retained-manager recovery succeeded, the recovered state and original close error are both visible. A manager failure keeps `Failed(error)` and runtime-owned transport/lease custody. A worker with no remaining probe does not run a second incidental drain on exit that would overwrite the failed status. No UI code releases the runtime lease. The dedicated reactor survives shutdown acknowledgement so unresolved retained cleanup is not destroyed with the card. Explicit Recheck retries cleanup before a fresh session can be acquired; no failed worker automatically reacquires a daemon. Service/cleanup/inspection errors remain available on reopen, and automatic opening checks are suppressed while recovery is unresolved.

### Settings and host ownership

`DesktopPreparationRequested` is emitted by the separate manual “电脑操控准备” action and after a successful **requested** false-to-true desktop preference save. The request bit is captured from the original base/candidate, and emission additionally checks the actual saved configuration after `edit.save`, config assignment and `SettingsSaved`. Failed writes, clicks before save acknowledgement, browser toggles and unrelated saves do not start guidance or claim enablement. The existing component-only check, direct privacy routes and browser-extension behavior are unchanged.

`window/cua_onboarding.rs` owns the card handle, per-open Arc identity, origin and source-stream subscriptions. Each new open explicitly closes/invalidates the old card before replacing ownership. Root release closes its card. Source release, source task-generation change and authoritative stream replacement close stale task cards. Old source subscriptions and deferred navigation callbacks compare the original per-open identity before affecting current ownership; no delayed callback targets a reopened same-thread card.

`bind_conversation_stream` subscribes to the real retained-entry `PreparationRequested`. Its guard checks both the event's thread ID and `TaskContext` against the **emitting stream**, then checks its current host ownership. Because `ConversationStream::thread` is crate-private, the read-only `PreparationRequested::is_current(stream)` guard is implemented inside the authorized card module in `vega_ui`; no stream file or lifecycle contract was changed. The host uses the existing public `route_project_id()` for the original navigation project.

Return follows the controller's cancel → close → navigation effects. A deferred, lifetime-fenced host callback resolves exactly the original project/thread with the existing `NavigationService`, then calls the existing `accept_palette_thread` route path (OpenedThread/SelectedProject/SettingsOpen, draft reservation and visit handling). It does not call control resume, agent submission, authorization, grants, pending actions, capture, or observation APIs. An unresolvable/deleted thread or pending task mutation yields a persistent host error and retry/dismiss controls. Opening errors are likewise visible in both Settings and conversation shells with keyboard-accessible retry/dismiss; the retained preparation entry itself is not dismissed by opening, closing or return.

## Static nine-anchor integration audit

This is source/API inspection, not behavioral or accessibility-tree execution. Anchors 1–5 are unchanged UI-A production, re-inspected for this integration.

| Anchor | Inspected production boundary and conclusion |
|---|---|
| 1 | `conversation_stream/core.rs`: one stored EntryLifecycle/current task; initialized from original thread identity in `new_with_request_queues`; identity-changing draft rebind starts a new context. No render-time lifecycle construction. |
| 2 | `computer_use.rs::set_computer_use_control`: outgoing error is read before replacement/removal and delivered to detach; repeated None preserves retention; incoming status is observed. Existing takeover/diagnostic invalidation remains. |
| 3 | `content.rs::MessageStarted`: duplicate-active-message rejection precedes `begin_computer_use_task`; old control is detached before the new context. Host observes the source stream and rejects/closes old context owners. |
| 4 | `render_computer_use_notice`: retained entry is rendered independently of the absent-control early return in the active notice; mouse/key callbacks capture original context. Existing stream-render placement still consumes the notice. Host subscription now opens the card from that actual event. |
| 5 | `artifact.rs::detach_finished_computer_use`: matches generation/thread/stream, reads outgoing control error and passes context to detach before finish. Both normal batch finish and `agent.rs` worker-spawn failure call it before `agent_controller.finish`. No reliance on a later None/error delivery. |
| 6 | `card.rs::GuidanceCard`, `card/view.rs`: own the frozen CardController, render CardView and dispatch common CardAction for pointer/keyboard. No independent ready/completion policy. |
| 7 | `card.rs::apply_effects`, `session.rs::ProbeClient/inspect/reclaim`, host `return_from_preparation`: exact ticket, false/false-only existing probe session, truthful external-dispatch recovery, original-thread-only navigation, original inspection error and cleanup custody retained. |
| 8 | `card/view.rs::app_row`: synchronous begin_drag, exact bundle + directory=true native payload, view.following consulted by the follow loop; no accepted-drop or has_active_drag readiness/freeze inference. |
| 9 | Card Close/Escape/Return, on_window_should_close/on_release, host replacement/root/source release: invalidate/cancel before ownership removal, exact-lifetime fencing, detached service cleanup, no unrelated control stop. |

Transport classification was re-inspected: `ExecutorFailure::needs_permissions` in runtime `diagnostics.rs` matches only PermissionsMissing, AccessibilityPermissionMissing and ScreenRecordingPermissionMissing, not Transport. This is static evidence, not executable transport-injection coverage.

## Local checks actually performed

No local Vega/Rust/native execution, builds, package commands, cargo check, Clippy, Rust tests, Python tests or CI calls. No independent review result is claimed.

1. `find . -name AGENTS.md ...`: only root AGENTS; no nested instruction files in edited paths. Required recovery/spec/design/test/native/UI-A documents and pinned installed GPUI APIs were read.
2. `rustfmt --edition 2024 --config skip_children=true` followed by the same `--check` on all 13 authored Rust files: exit **0** on the final source revision. Explicit `skip_children=true` avoids touching frozen tests or native files. Formatting validates syntax/format, not types.
3. `git diff --check`: exit **0**. `git diff --no-index --check /dev/null <new source>` on the four new/recovered owned source files: exit **1**, no whitespace diagnostics; 1 is expected content difference from `/dev/null`, not a passing compilation or a whitespace failure.
4. Scoped added-line plus new-source grep: no added comments, unwrap/expect/panic, tests or lint suppression. No new dependencies/workflows. Source-only/static hygiene, not a test run.
5. `git diff --exit-code HEAD -- crates/vega_runtime crates/vega_conversation crates/vega_ui/src/conversation_stream crates/vega_ui/src/cua_onboarding/{controller,lifecycle,guidance_tests,bundle,placement,retention}.rs Cargo.toml Cargo.lock .github`: exit **0**.
6. `shasum -a 256` of frozen tests/native matches the checkpoint. Static declaration counts remain UI **15**, runtime **13**. No test content or test wiring was changed.
7. Actual installed GPUI inspection covered open_window callback/handle ownership, observe/observe_release signatures, nonactivating creation options, display-local bounds, keyboard focus without native activation, external file-drag resolver signature, accessibility roles/labels, and URL/Finder void APIs. `NavigationService` requires the original project as well as thread ID; the host retains both.

The existing independently reviewed UI-A Actions run `39817506348` (28 run, 28 pass, 0 skipped) is **inherited evidence only** and does not validate this now-wired Stage B patch. Earlier UI REDs `39811963094` and `39813558507` were interface-missing REDs; behavioral assertions had not executed at those REDs. Parent owns current-version independent review and the existing full cloud gate with the same frozen 28 tests; tests must remain until that full implementation is GREEN and evidence is fixed.

## Exact SHA-256 content manifest

All paths are relative to this worktree. The report's own hash is supplied separately in the READY message to avoid a self-referential hash.

| Authored production file | SHA-256 |
|---|---|
| `crates/vega_ui/src/cua_onboarding/card.rs` | `7419ba8d19430f0b2c7fe98d266e963e331603def43c02ab58f9552e8ae260fd` |
| `crates/vega_ui/src/cua_onboarding/card/view.rs` | `a36f52afad72b6d6b4ec3ecd1db3eecf3ac38dfed40c9a02975493fc5608ad4d` |
| `crates/vega_ui/src/cua_onboarding/session.rs` | `f25fdfe025249263feb35fcd968d752136d7fdeb6d65921d226e3f7f4dfa1899` |
| `crates/vega_ui/src/cua_onboarding/mod.rs` | `67df22b2bdd3c46d57ad3138548223efdec2266a6b23f1a8c101744df12db7fa` |
| `crates/vega_ui/src/settings/agents.rs` | `e98175e61f1df56b9e5f38ffb0384dcbb21b1d434eeecf8ecabfc4084839a074` |
| `crates/vega_ui/src/settings/preferences.rs` | `de6a7b9a61bbe747fd23f3b0ec8bc9724d1018358569e38f3374918cea426642` |
| `crates/vega_ui/src/settings/mod.rs` | `ed82635b7f00919a4a4c2930c49883d5da90359a29db7957e0a8357aab0a0aba` |
| `crates/vega_ui/src/settings/state.rs` | `335fef55e37e2c1eecbb97455b5e89df762004785d1c8fa4587695be9172e5f1` |
| `crates/vega/src/window/mod.rs` | `7ec6c70b8f155e26a4136be5e066c6c2f4ace8c15523cc8b6d12c234557398ec` |
| `crates/vega/src/window/render.rs` | `128359b30b4264940447868381f719072baa99ab8a312dd28e5d65858197e25e` |
| `crates/vega/src/window/native_subagents.rs` | `4936bac22e17d38b567a26ef2f9f5000162b2957a2d65c6a01a273dcfcf3ee9a` |
| `crates/vega/src/window/cua_onboarding.rs` | `962bd75a87f30ae27818498a157b90ec45b18ee68b933794a6ab8dbbc32d0a8b` |
| `crates/vega_theme/src/lib.rs` | `53ef94948bbc0e0a3324203d63f8780b4b9135c64f967b75c5cf3ffa9353b6a5` |

Protected preserved content:

| File | SHA-256 |
|---|---|
| UI `cua_onboarding/guidance_tests.rs` | `26971f63dbaa01dc31e68e76d6237b2ffcc1942234bdc4ade96055069e6e48f2` |
| Runtime `computer_use/preparation_session_tests.rs` | `48076fccdbe6546543d4246f45128adf5177b19172015f6b0d3c5ddd94328d2a` |
| `cua_onboarding/native.rs` | `122a7e9126862ce1beac7bb5c0fecd1a3430b643a5d77097cf1efdabc7d31fb4` |
| `cua_onboarding/native/macos.rs` | `e5c65124751445bb5edcec0db7bc7fd27133aef8cb19fcc7f994598b0faf03f0` |

## Unresolved validation and handoff

- No known authorization/scope blocker remains from this static pass. Compilation, warnings/lints and the same 28 behavior assertions on the fully wired final version remain **pending parent CI**. The new UI/native/session/host integration itself has no executed component/native harness coverage; do not infer it from pure-controller tests.
- Independent review must inspect this exact source manifest, especially retained error restoration without RPC, close/manager custody, same-thread replacement fencing, original-project navigation and native fallback coordinates. Review is not supplied by this writer's own audit.
- User manual checks: installed `.app` and development fallback, real TCC status/restart behavior, actual file drag into Settings and cancelled drag, focus/Tab/accessibility tree, nonactivation/Spaces/fullscreen, administrator-dialog layering, 150ms follow/freeze/loss/reappearance, primary and left/above/below/Retina display placement/topology, 480×420 scroll/clipping in light/dark themes, Launch Services/Finder failure recovery, actual close/reopen cleanup behavior, and both entry routes after a finished permission-failed run.
- No native capture bridge, prompts, browser onboarding, automatic close/ready/resume, grant mutation or observation/action replay was introduced. The issue is not claimed complete or manually accepted.
- Writer stops writes at this READY handoff. Parent owns independent review, Git/index/commit/push, CI, later required test-removal/final empty-set gate and user acceptance.

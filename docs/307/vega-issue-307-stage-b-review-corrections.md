# Issue #307 Stage B — original-review corrections

## Handoff and scope

Implementation-writer handoff: **READY for parent integration and independent delta review, not acceptance or full GREEN**. The writer stops source/report writes on return. The parent owns Git/index, commits, push, CI, review coordination and eventual integration; this pass performs none of those mutations and does not query CI.

- Worktree: `/Users/puzige/Workspace/worktrees/vega-307-cua-onboarding`.
- Branch: `feat/307-cua-onboarding`.
- Synchronized baseline supplied by the parent, and unchanged committed HEAD throughout this pass: `757c851e2a3c0daf881b1ba064dc4ec5236f02dc`.
- Original reviewed integration: `3e32543432d15952d986bda504ff703f63602073`.
- Original independent review: [BLOCK receipt](https://github.com/puzige/vega/pull/537#issuecomment-6348903371). This report does not supersede that receipt with an independent PASS.
- Applicable correction boundary: [integrated review correction checkpoint, 2026-10-22 19:00 UTC](vega-issue-307-stage-b-integration.md#integrated-review-correction-checkpoint-2026-10-22-1900-utc). These five findings correct the original acceptance within the still-open implementation cycle; there is no acceptance change or new coverage goal.
- The separately completed selector repair, `2e0089cf9cdc769ebbacccbc3831a4157ef41f28`, is already integrated. Both native files are read-only in this pass; see [selector declaration report](vega-issue-307-selector-declaration-fix.md).

This writer's authored delta is only the five permitted production files plus this new report. No runtime, shared types, Settings, placement, native adapter, theme/Layout, dependency, workflow, frozen test, historical report or parent-checkpoint edits were made by this writer. No optional submodules were added. No tests or harnesses were added, changed, removed, weakened, ignored or retried. Root `AGENTS.md` is the only applicable AGENTS file found below this worktree; the execution guide and the correction checkpoint were re-read.

During final verification, a concurrent documentation-only delta appeared in the parent-owned `docs/307/vega-issue-307-stage-b-integration.md`: eight added lines under “Selector repair verified; behavioral corrections pending, 2026-10-22 19:45 UTC”. It is not part of this writer's authored patch and was inspected read-only, not reverted or edited. Its observed SHA-256 is `502ed596b90542474e20718ec293c7251eed2a78f9d4961fc9b5ff54ddc1c0a1`. Consequently the final whole-worktree diff also contains that external checkpoint change; the five-source manifest and production patch identity below remain unchanged.

## 1. P1 — cancellation/reopen automatic-recovery race

### Correction

`session.rs` now carries service-owned lifecycle and request provenance without changing `CardAction`, `CardEffect`, `CheckTicket` or runtime ownership APIs:

- Each attached client has an exact `Arc<()>` identity and a service-owned retiring flag. `ClientLease::cancel` marks retirement and advances the revision under the shared state lock **before** cancelling the token. Explicit cancel, drop and worker teardown use the same idempotent identity; `finish` removes it only after that worker's reclamation attempt has finished.
- `ServiceState::snapshot` cannot report Clear while reclamation or any retiring client remains. An existing Failed state remains Failed rather than being replaced with a misleading Clear or hiding its typed error.
- Automatic startup and explicit Recheck use separate adapter-local `InspectionIntent` values. `card.rs::start` is the automatic caller; user dispatch is manual. Controller actions/effects and ticket matching are unchanged. A cached original inspection error is projected through the controller without dispatching an RPC.
- An automatic request captures an `Arc` revision at service enqueue. Under the asynchronous service gate, `validate` rechecks cancellation, fatal failure, revision and the actual service snapshot before opening or checking a probe. Automatic requests never invoke the empty-session recovery drain.
- A manual request with no probe may invoke `cleanup_pending`, but authorization is revalidated under the state lock at the same point that reclamation is published. Retirement blocks that request with the existing typed failure or NotReady; it does not become a delayed acquisition that silently proceeds after the retiree finishes. A later explicit Recheck can retry.
- After an authorized manual drain, admission is checked again immediately before acquisition. That validation and synchronous `PreparationProbe::open` use the same state lock as client cancellation, so retirement publication cannot slip between validation and acquisition. This does not replace the runtime's exclusive ownership authority.
- Inspection, terminal-error reclamation and retirement reclamation share one asynchronous gate. A failed check is reclaimed before releasing that gate or returning its result, rather than allowing another worker to consume the cleanup manager's recovery slot first.
- Original inspection results remain the request result. Inspection, direct-close, cleanup-manager and fatal-service failures remain distinct status fields. A successful unrelated probe retirement does not erase an earlier direct-close error; successful explicitly authorized empty-session recovery can clear it. Rejected stale automatic work does not overwrite cached inspection errors.

The short standard-mutex sections end before every await. Synchronous probe open is deliberately inside the admission/cancellation linearization section; no local timing, responsiveness, Send/type-check or concurrency execution claim is made.

### Source-level interleaving audit

| Ordering | Result in the corrected adapter |
| --- | --- |
| Old client cancels before the new card's snapshot | Retirement is already published. Snapshot is Reclaiming, or preserves Failed, and startup is suppressed. |
| Card cached Clear, then cancellation happens before automatic enqueue | Enqueue may capture the newer revision, but worker validation still sees retirement or cached failure and rejects it. No automatic drain occurs. |
| Automatic enqueue precedes cancellation | Cancellation changes the revision. Gate-time validation rejects the stale request even if later cleanup succeeds. |
| Queued automatic work waits behind old cleanup | Reclamation and completion advance the revision. Failure remains blocking; success does not authorize that stale queued request. Neither path runs automatic recovery. |
| A retirement has already fully succeeded before automatic admission | A genuinely Clear service with no cached failure may admit a new initial check. This is not a cleanup retry; the automatic path has no recovery drain. |
| Cancel races synchronous probe open | If cancellation obtains the state lock first, admission fails. If open obtains it first, acquisition precedes cancellation; subsequent cancellation retires that worker and its probe. Runtime exclusivity is unchanged. |
| Manual Recheck overlaps retirement | It either rejects before recovery, or, if authorized before a new retirement, must revalidate again before open. A blocked request cannot auto-acquire after retirement finishes. |
| Close, release and drop invalidate the same client repeatedly | Retirement and completion are identity-based and idempotent. One client cannot decrement another's custody or publish Clear while other retirees remain. |
| A transport/check failure needs reclamation | The original inspection error is cached and returned; reclamation stays in the same gate. Cleanup failure remains separately visible and retained by the unchanged runtime manager. |

These are production-source conclusions, not executed race tests.

### Read-only runtime evidence

`vega_runtime/src/computer_use/preparation_session.rs` still opens through the existing authority, checks cancellation, calls `check_permissions(false, false)`, returns the original check error after its close attempt, and releases ownership only after successful transport cleanup. Drop transfers remaining transport to `OwnershipAuthority::retain`.

`ownership.rs:93–114` serializes `cleanup_pending`: an unreported cached cleanup failure is reported once; a subsequent drain may retry it. That is why automatic startup must not drain the manager. This correction does not edit that policy, force-release a lease, cancel `ComputerUseControl`, or cancel an unrelated run. Failed cleanup remains in the existing runtime custody.

## 2. P2 — duplicate Enter/Space activation

The card's shared `action_button` and the host's open-failure retry/dismiss controls now use only their existing `on_click` activation path. The additional Enter/Space keydown dispatchers were removed. Enabled guards, tab stops, focus-visible style, roles, accessible labels and click actions remain. Explicit body action handles also apply `FocusHandle::tab_stop(enabled)`, so tracking focus does not bypass the enabled tab-stop state.

Pinned evidence is `gpui-pre 0.3.4`, `src/elements/div.rs:2954–3017`: a focused `on_click` control records an unmodified Enter/Space keydown focus generation and invokes its click listeners on the matching keyup as `ClickEvent::Keyboard`. Registering another activation keydown dispatch is redundant and can trigger the action twice. The correction relies on that actual pinned path, not an assumed mouse-only `on_click` contract.

The card's root capture handler retains Escape, Tab and body scrolling, but does not dispatch Enter or Space. Settings' separate mouse-up plus keydown implementation is untouched. Keyboard activation has not been exercised locally; exact-once interaction remains part of independent review and manual acceptance.

## 3. P2 — keyboard access to overflowing content

The existing fixed heading and footer remain outside the scroll body and retain `flex_shrink_0`. The body now has a persistent `ScrollHandle`, a tracked body focus handle and stable focus handles for Finder, Accessibility Settings and Screen Recording Settings.

- Up/Down scroll by the typography-token line step; PageUp/PageDown use the measured viewport height; Home/End use the measured offset limits. These keys act only when the body or one of its descendants has focus, without modifiers. Offset is clamped to `[-max_offset.y, 0]`, then focus moves to the body so a scrolled-away action is not left focused.
- Tab/Shift-Tab still use `focus_next`/`focus_prev`, followed by a reveal request for the actual focused body action. `Context::on_focus` subscriptions cover other focus transitions too.
- Reveal uses `scroll_to_item` with the actual immediate child containing that action, rather than estimated pixel coordinates. Action child indices are derived from the rendered feedback count: app row `N`, Accessibility row `N + 2`, Screen Recording row `N + 3`. Feedback insertion/removal updates those indices and re-reveals a currently focused action.
- Native GPUI wheel scrolling remains intact. Close, Recheck and Return remain fixed and keyboard reachable; no extra permanent explanatory text was added.

Pinned `gpui-pre 0.3.4` API evidence:

| Source | Contract used |
| --- | --- |
| `elements/div.rs:770–782,1506` | Explicit focus tracking/tab stops and scroll-handle tracking. |
| `elements/div.rs:1936–1967` | Real child layout bounds are recorded before `scroll_to_active_item` runs in prepaint. |
| `elements/div.rs:2378–2435` | Layout-derived nonnegative maximum offset and bounds are recorded; offsets are clamped. |
| `elements/div.rs:4129–4273` | `new`, `offset`, `max_offset`, `bounds`, `scroll_to_item` and `set_offset`; reveal uses the actual child bounds and minimum visible-region adjustment. |
| `window.rs:2243–2261` | `focus_next`/`focus_prev` only change focus; they do not reveal an offscreen child. |
| `window.rs:585–614` | Explicit handles can update tab-stop state and test focused/contained-focused state. |
| `app/context.rs:547–567` | `on_focus` supplies the entity/window/context callback and returns a retained subscription. |
| `gpui-pre-macos 0.3.4/src/events.rs:379–382` | Platform key identities are `pageup`, `pagedown`, `home`, `end`. |

The APIs were read from the installed registry source matching the unchanged lockfile versions. This is source inspection, not a GPUI harness run, screenshot, accessibility-tree check, or proof that every long-content layout is visually correct. Those checks remain manual.

## 4. P2 — truthful component fact on typed open errors

The only `controller.rs` change is the component projection inside `CardController::view`. When there is no preparation value, these typed failures map as follows:

| Preserved `CheckState::Failed` error | Component row state |
| --- | --- |
| `MissingComponents` | `ComponentState::Missing` |
| `IncompatibleComponents` | `ComponentState::Incompatible` |
| `UnsupportedPlatform` | `ComponentState::UnsupportedPlatform` |

The original error and reducer behavior are unchanged. Unrelated failures still project no component fact. Absent preparation still means Unknown for each permission, `DirectCapture::NotChecked`, and `desktop_ready: false`; no grant or capture success is inferred. Recheck/ticket/lifetime/action/effect logic is unchanged.

## 5. P2 — acquired follow lifecycle

`CardEffect::OpenUrl` now restarts bounded initial seeking only from `Tracking::Waiting`. It no longer resets an already acquired `Tracking::Following` state to a 20-attempt initial budget, and it does not extend an initial budget already in progress.

The existing follow loop decrements that budget only while Seeking. After acquisition, an absent `geometry.settings` frame retains Following and the last origin, so the existing 150 ms poll continues and can resume when a frame reappears. Privacy-route actions cannot turn that acquired state into permanent Waiting. Permanent `begin_drag` freeze is unchanged; OpenUrl cannot re-enable controller following or a Disabled state. Existing native geometry/movement error degradation is unchanged and is not claimed as recoverable missing-frame behavior.

## Preserved integration invariants

The correction leaves the original Stage B integration intact: the real embedded preparation probe; static independent permission facts; false/false RPCs; no permission prompts, capture bridge, browser onboarding, automatic ready/close/resume; no stale ticket acceptance; exact `.app` drag payload and permanent drag freeze; original-thread navigation after cancellation/close with no task replay; host lifetime/origin fences; Settings entry/persistence behavior; panel marker and nonactivation configuration; display-ID/point geometry; dimensions and theme tokens. Independent delta review must inspect affected invariants rather than accepting this writer audit as review evidence.

## Static validation actually performed

No Vega execution, compilation, build, package, cargo check, Clippy, Rust/Nextest/Python tests or UI harness was run. No cloud status was queried in this pass. Only fast source/diff/format/hash inspection was used.

The final production version below was checked with these commands and outcomes:

| Check | Actual outcome |
| --- | --- |
| `rustfmt --edition 2024 --config skip_children=true --check` on the five production files listed below | Exit 0. |
| `git diff --check` | Exit 0, no diagnostics. |
| Exact sorted `git diff --name-only` comparison with the five-file source allowlist, before adding this report | Exit 0; only those five production files differ. |
| `git diff --exit-code` over runtime, conversation/types, frozen tests/lifecycle/native, conversation stream, Settings, theme, Cargo files, workflows and the two protected historical Stage B reports | Exit 0 at the earlier scope check. Final strict whole-tree comparison subsequently detected the concurrent parent-checkpoint change described above; that evidence was inspected and preserved, not treated as this writer's source delta. |
| Added-production-line scan for comments, `.unwrap(`, `.expect(`, `panic!`, test/ignore declarations and lint suppressions | Exit 0 for the audit; no prohibited additions found. This is a source hygiene check, not a behavioral test. |
| `shasum -a 256` on final sources and frozen files | Exit 0; exact values below. |
| Static `#[test]` / `#[tokio::test]` declaration count | Exit 0; 15 UI declarations and 13 runtime declarations. These are not execution counts for this pass. |

The report's final read-back/whitespace and hash checks are supplied with the parent handoff. An initial read-only manifest verifier stopped because the shell variable `path` changed zsh command lookup; using `file_path` verified all source/frozen hashes. The subsequent strict scope mismatch was the external parent-checkpoint delta above, not a product or source-hash failure. Both observations were inspected before continuing; neither prompted a source change or weakened a behavioral assertion. No unrun validation is marked passed.

### Exact production SHA-256 manifest

| Path | SHA-256 |
| --- | --- |
| `crates/vega_ui/src/cua_onboarding/card.rs` | `d0955eabb987fa4a500081d6be1470814043bb3d8b19a4bdcea6ec40fb0ef033` |
| `crates/vega_ui/src/cua_onboarding/card/view.rs` | `7d5205cd86a7754a0c3d8bb99331bb3d086e73f300dbbefd280c131af66b5b53` |
| `crates/vega_ui/src/cua_onboarding/session.rs` | `1d6281d74a6becf6138df4ac6a480bfb8bb5d902c15da44166ab0fbcf4c253ed` |
| `crates/vega_ui/src/cua_onboarding/controller.rs` | `2100a7f60a4ae0559c9be2f28a8eb2090bda2a0c32809cd54bae3900e6acbfa4` |
| `crates/vega/src/window/cua_onboarding.rs` | `0806e8d2b04961f8b254edbca7731f193285557e2e6906c34ee0b3fddb04b187` |

Production diff identity, relative to baseline HEAD, excluding this new report:

`SHA-256 f62ffde466bed838a374cd28b42b9a474f30d210273edb4c5037d55003521989`

Reproduction command used (read-only):

```sh
git -c color.ui=false -c core.abbrev=8 -c diff.algorithm=myers \
  -c diff.mnemonicPrefix=false -c diff.noprefix=false diff \
  --no-ext-diff --no-textconv --binary --src-prefix=a/ --dst-prefix=b/ \
  --unified=3 HEAD -- \
  crates/vega_ui/src/cua_onboarding/card.rs \
  crates/vega_ui/src/cua_onboarding/card/view.rs \
  crates/vega_ui/src/cua_onboarding/session.rs \
  crates/vega_ui/src/cua_onboarding/controller.rs \
  crates/vega/src/window/cua_onboarding.rs | shasum -a 256
```

Diff size at this version: 5 production files, 329 insertions, 99 deletions. The full baseline SHA plus the source manifest and diff identity identify the uncommitted implementation; there is no new implementation commit yet. The report's own final SHA-256 is returned separately to avoid self-hash circularity.

### Frozen/read-only SHA-256 manifest

| Path | SHA-256 |
| --- | --- |
| `crates/vega_ui/src/cua_onboarding/guidance_tests.rs` | `26971f63dbaa01dc31e68e76d6237b2ffcc1942234bdc4ade96055069e6e48f2` |
| `crates/vega_runtime/src/computer_use/preparation_session_tests.rs` | `48076fccdbe6546543d4246f45128adf5177b19172015f6b0d3c5ddd94328d2a` |
| `crates/vega_ui/src/cua_onboarding/native.rs` | `122a7e9126862ce1beac7bb5c0fecd1a3430b643a5d77097cf1efdabc7d31fb4` |
| `crates/vega_ui/src/cua_onboarding/native/macos.rs` | `5c0d06a6f081960610860f1e176978031cb98218a78e7daa91c4309e23f3f251` |

## Historical CI versus current pending validation

The inherited fully wired [Actions run 39832532323](https://github.com/puzige/vega/actions/runs/39832532323), recorded in the [original CI receipt](https://github.com/puzige/vega/pull/537#issuecomment-6348465410), applies to `3e32543432d15952d986bda504ff703f63602073`, not this correction diff:

- Nextest job `120012146283`: 28 discovered, 28 executed, 28 passed, 0 skipped; release-script checks 5 + 15 + 40 success.
- Clippy job `120012145906`: failed on the cross-module `sel_registerName` declaration conflict under `-D clashing-extern-declarations` from `-D warnings`.
- This was not full GREEN. The separate selector repair addresses that declaration defect, but neither those passing assertions nor the selector source audit proves this corrected version compiles, lints, passes CI or satisfies native interaction acceptance.

The newly observed parent-checkpoint update additionally records an independent selector-only [PASS](https://github.com/puzige/vega/pull/537#issuecomment-6349020243) and [Actions run 39837897922](https://github.com/puzige/vega/actions/runs/39837897922) full GREEN at baseline `757c851e2a3c0daf881b1ba064dc4ec5236f02dc`: Nextest job `120030933592`, Clippy job `120030933870`, aggregate `120032957529`; 28 discovered/executed/passed, 0 skipped, Python checks 5 + 15 + 40. Its [durable receipt](https://github.com/puzige/vega/pull/537#issuecomment-6349218192) is supplied by that parent-owned documentation, not independently fetched or queried by this writer. That checkpoint explicitly excludes the five pending behavioral corrections from its GREEN, so it is not current-correction validation and does not erase the original BLOCK or failed-gate history.

Parent next steps: commit the stopped source/report version, reuse the original independent non-writer for the delta and affected invariants including the integrated selector repair, then obtain the unchanged full cloud gate at the reviewed exact head with the same 28 frozen tests. Retain those tests until reviewed full GREEN; any later test removal and final empty-set gate remain separate parent-owned steps under the existing lifecycle. This writer does not delete tests or assert issue completion.

Manual acceptance remains pending for close/reopen/cleanup failure timing; exact-once Enter/Space, Tab/Shift-Tab and scrolling of long failures; enabled/disabled/focus and accessibility behavior; privacy-route frame loss/reappearance and permanent drag freeze; installed `.app` versus development fallback; real TCC/restart/reauthorization; Finder/open dispatch failures; nonactivation, Spaces/fullscreen/admin-dialog layering; multi-display points/Retina topology; and long-content clipping in light/dark themes. The 28 frozen pure tests do not supply a native/keyboard/component interaction harness. No screenshot or manual result is claimed.

No known additional implementation change outside the authorized five findings is proposed. Remaining independent review, current-head cloud validation and user manual acceptance are explicitly unresolved, not waived.

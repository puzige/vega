# #307 compact permission guide · integration handoff

## Version and stage

Parent integration report, 2026-10-23 02:18 UTC. The finalizer reached its stage budget during final audit and did not return READY. Its execution is terminal/interrupted and all writers are stopped. Parent inspected and froze the preserved source instead of assigning another architecture rewrite. This is **an immutable review/CI candidate, not verified native effect or acceptance**.

- Base HEAD before this candidate: `20b4f59a5b7023fe566d0c2b867891ef5dd3f805`, including current master `f3eab9827dcd1ddb459088410d6f1eedcbce8864`.
- Contract: [compact-guide specification](vega-issue-307-compact-guide.md), C1–C12; [recovery trail](vega-issue-307-compact-guide-recovery.md) preserves both earlier interrupted stages and their exact artifacts.
- Production and test content is identified by the manifest below. SHA-256 of its sorted `shasum -a 256` output is `3267e24d80dc5d07b20e436748b7b6508d0fe9b1333eaacac754c49ffea0465a`.
- No local Vega execution, compilation, package, cargo check, Clippy, Rust/Python test or UI harness was run. No original user recording/frame is committed or uploaded.

## 2026-10-23 continuation: verified development evidence and new delivery baseline

The preceding candidate record is historical, not the current delivery policy. Candidate `ccba58f8b9fa47ead8e3e516d234548bbc8ef608` is pushed and reachable in [draft PR #574](https://github.com/puzige/vega/pull/574). [Actions run 39889718957](https://github.com/puzige/vega/actions/runs/39889718957), `headSha` exactly matching that candidate, succeeded: fmt/all-target Clippy job `120206311731`, Nextest `120206312181`, required `check (fmt, clippy, test)` job `120207119205`. Raw Nextest reports **12 discovered, 12 executed, 12 passed, 0 skipped**; both new controller regressions passed. Retained Python groups executed 5, 15 and 40 tests successfully. No local code or tests ran.

Latest master `9f9fa092a3ca391a65f08419c9206f32979119e6` (#573) corrects test retention independently of TDD ordering: development Rust tests must not enter master. It removes the upstream compaction suite and updates policy. Integrated cleanly at `77bfcecd591699fb0e1cd7da8faaa15a308de095`; all thirteen compact-guide production files and the new controller test remain byte-identical to `ccba58f8`. The test file [original source](https://github.com/puzige/vega/blob/ccba58f8b9fa47ead8e3e516d234548bbc8ef608/crates/vega_ui/src/cua_onboarding/controller_tests.rs), its [wiring](https://github.com/puzige/vega/blob/ccba58f8b9fa47ead8e3e516d234548bbc8ef608/crates/vega_ui/src/cua_onboarding/controller.rs#L222-L224), and [original commit patch](https://github.com/puzige/vega/commit/ccba58f8b9fa47ead8e3e516d234548bbc8ef608.patch) remain available. GitHub Contents API independently confirmed test blob `d5d07800214a065178e75cf196af01643e03a8dd`, 3387 bytes, SHA-256 `f5eee0c9730bfb185ea8c1cb3a8738e05e20bf1ea2ba11db1067324368252b4b`.

The prior reviewer execution has no accessible current result and no review was published. Do not count it as PASS. Next: delegated cleanup limited to the already-passing controller test and its exclusive three-line module wiring, non-writer independent production/cleanup review on the stopped version, and full latest-head required CI establishing Rust 0 discovered/0 executed. The old statement below about retaining new tests is superseded by this section and the updated spec, not an authorization to weaken assertions or delete a failing regression. No native/visual acceptance is claimed.

## Current repair checkpoint after final-candidate review failure

The earlier final candidate `fca732e91451c657811a7c7d1cf0e95f22065964` passed [run 39891774453](https://github.com/puzige/vega/actions/runs/39891774453): fmt/all-target Clippy, Python5+15+40 and required check, with `no Rust tests` (0 discovered/0 executed), recorded separately in [the final-candidate receipt](https://github.com/puzige/vega/pull/574#issuecomment-6356280653). That does not validate the new delta below. Its independent review failed before a verdict; no PASS is inferred. Parent diagnosed and recorded two actual UI defects in the recovery trail and clarified C9/C11 before repair.

Repair writer explicitly reported READY and stopped, changing only `model.rs` and `card/view.rs` (73 insertions/38 deletions):

- Separate local blocking path/panel/window faults from shared inspection/service/cleanup warnings. Shared warnings retain a valid real-app icon/drag target and add the existing accessible `权限与检测` route in the 72pt app row. The four short warning strings are15/16/16/16 full-width characters, estimated ≤208pt in334pt header allowance; icon/diagnostics/task-return estimate253pt fits366pt inner width. These are arithmetic, not pixel measurements. Default content/dimensions and real bundle/freeze behavior are unchanged.
- Last-owner release cancels the client and snapshots service facts, retaining the existing passive poll only while cleanup is Reclaiming. Each tick records new service facts then stops at Clear/Failed if still unowned. Reacquisition reuses the one poll. Tail observation issues no RPC, owns no probe, and does not retry cleanup; model disposal still stops unconditionally. Settings opening remains passive and can read the captured final cleanup facts.

Exact repair SHA-256: `model.rs` `85eb740f9e325827fc38c9000fbbe44aef8c4c6e7d0cdb50c49fe9a3adaa8977`; `card/view.rs` `437ce12f683b54f673252a0b9976c593419a86f8b61a4bbe735ab7e1f24de894`. Remaining production manifest entries below are unchanged except controller test removal/prefix hash `c7346d2892d7802fab8e71b21cbd4cacfd1928d2e58b4eacedbfcd5fc0808b01`. No executable Rust tests/support remain. No new behavior tests were added for this GPUI lifecycle wiring; static call-chain review and new full-head Actions do not prove native rendering or model timing.

During the repair, master advanced independently to `5a1ec8ad26fa53fd6210a8e58b6ad85e739dd8bb` (delegated-stage recovery). Parent fetched, inspected its scope and merged cleanly at `d3407a20a6bbba4d1aec36a4c273e2fb6b179055`; the two repair hashes remained unchanged. No feature changes to runtime/session/native/dependencies/assets/workflows/Python relative this latest base. Writer and parent ran actual scoped rustfmt and `git diff --check`, exit0. Current source is stopped for a new immutable commit, independent non-writer production/cleanup review and final required Actions; previous GREEN does not validate this delta. Native/user acceptance remains pending.

## Implemented delta (original candidate; current repair above supersedes error and poll details)

1. The helper is a **400×140 logical-point** surface, with 24pt shadow padding on each side (448×188 native bounds). Default content is one instruction, the official Vega icon/name drag region and named close. There is no diagnostic scroll body, permission table or footer. A task-origin guide may include a secondary navigation-only return action.
2. The icon and drag preview embed the existing `vega-icon-smile-light.svg` used by packaging, not a generic monitor glyph or third-party asset. The current process's parsed `.app` remains the external file payload; drag freezes following synchronously. A development executable remains non-draggable and offers Finder at the real executable. Missing paths and native-follow failures use compact actionable replacements.
3. A shared UI `PreparationModel` owns **one ProbeClient**, one pending exact ticket and one polling lifecycle. Settings and the guide hold identity-specific owners, not competing detection chains. The card no longer owns a separate probe; the old Settings component-only background worker is removed.
4. Settings → Agents → Native presents independent component/Accessibility/Screen Recording facts, actual capture `未验证`, manual recheck, restart note and retained inspection/adapter/close/cleanup failures. Mere Settings construction/opening does not acquire a probe. Explicit Settings actions acquire passive ownership; recheck uses Manual admission. Only the guide's first clean startup may request Automatic admission.
5. `DesktopPreparationRequested` carries an exact permission pane. Persisted desktop enable still emits only after successful false→true saving. Per-permission actions dispatch their specific Settings URL and reuse a valid open compact guide. Task-origin requests retain their source/context/project fences and default to the Accessibility pane.
6. Closing a guide invalidates its controller, cancels its following loop and releases its model owner with inspection cancellation before removal. Settings disposal/close releases only its owner; final release stops polling. Past facts remain clearly marked retained, not a replayed new check. Queued old polls recheck cancellation inside the UI update, and old tickets cannot complete newer work.
7. Actual shared preparation failures notify the guide and offer a compact route to `权限与检测`; details remain in Settings. The route uses existing draft-guarded Settings navigation rather than bypassing unsaved edits.

## Invariants and static audit

- Existing `session.rs`, native adapter, runtime preparation/ownership, driver, shared conversation types and retained task-entry implementation are unchanged.
- All preparation RPCs still originate in the existing runtime `false/false` transport. There is no prompt, native capture bridge, browser onboarding, auto-Ready/completion/close, run resume or grant/observation replay.
- Existing service retirement/revision/gate validation and confirmed cleanup/manager custody remain the authority. The UI model never releases runtime leases itself or stops another run. Explicit recovery remains Manual admission.
- Following retains 150ms polling, 1pt movement threshold, point/display-ID mapping, original placement clamp and acquired-window recovery. First drag permanently freezes this card lifetime. Native appearance, focus and actual layering are not proven by that source audit.
- Default vertical geometry is 2pt border + 16pt top + 24pt instruction/close row + 8pt gap + 72pt icon/name target + 16pt bottom = **138pt**, within 140pt. Icon/name are nonshrinking. This arithmetic is not a screenshot or font-layout measurement.
- Existing production SVG bytes and all protected paths remain unchanged. Upstream `storage_contention_tests.rs` from #570 is preserved; the suite is not assumed empty under the current validation policy.

## Actual checks

Parent repeated these after all implementation executions stopped:

| Check | Actual result |
| --- | --- |
| `git diff --check` | Exit 0, no whitespace diagnostics. Earlier trailing-space failure is preserved in the recovery report; source was corrected by the finalizer. |
| Focused `rustfmt --edition 2024 --config skip_children=true --check` over 13 modified/new production paths | Exit 0. |
| Same focused rustfmt on new `controller_tests.rs` | Exit 0. |
| Protected-path diff over runtime, conversation, `.github`, Cargo files and assets | Empty; no changes. Session/native files also absent from the complete diff allowlist. |
| Actual source call-site search | Only `model.rs` creates `ProbeClient`; card/Settings acquire and release the same model, and no prep prompt:true/capture:true or legacy per-click detect path was found. |
| Manifest hashes and source/final HEAD readback | Matched; fresh fetch found no newer master delta. |

These are quick static checks, not type/borrow/concurrency/renderer validation. Full current-version Actions and independent non-writer review are pending.

## Risk-proportionate regressions

Two new tests exercise the **production CardController**, not source strings or a duplicate model:

- Pending recheck de-duplication, cancellation recovery, stale-ticket rejection and static grants remaining unverified/open.
- Returning to the original task orders CancelPending → CloseWindow → navigation-only ReturnToTask, ignores late inspection completion and rejects post-close actions.

They are authored and formatted, **not locally executed**. They cover changed pending/cancellation decisions, not the whole shared GPUI model or native behavior. Existing cloud workspace tests and Python checks remain unchanged and must run on the exact candidate. No mandatory RED/GREEN/removal lifecycle is imposed by current policy, and these regressions are not scheduled for deletion.

## Exact source SHA-256

Paths are relative to repository root.

| Path | SHA-256 |
| --- | --- |
| `crates/vega/src/window/cua_onboarding.rs` | `e7cccf25bb4a4b01e5437bb57adb9a06914a24225917a8cfe8044ea8c503c206` |
| `crates/vega/src/window/render.rs` | `f700802261ab90dc18f0e0f73eb0597dd3e8fc5b2ac167b60fb3335f1036782c` |
| `crates/vega_theme/src/lib.rs` | `b79a47bed0adb553d9b6ed53cb2b2c311ef66c633eaab3914da8fb3be37d75de` |
| `crates/vega_ui/src/cua_onboarding/card.rs` | `251ae7ca3a0e1326c807c405d672369ad4ddf747349761d876ac5e1d69417a01` |
| `crates/vega_ui/src/cua_onboarding/card/view.rs` | `b432762e9ac5ee36205997d64265f6dad2a3c6896e2af06842c72c3be5a9dc42` |
| `crates/vega_ui/src/cua_onboarding/controller.rs` | `c3d70ac940ca4a4334700c9d7e410802e573bdba2a7e5146a972d4ea0432e30f` |
| `crates/vega_ui/src/cua_onboarding/controller_tests.rs` | `f5eee0c9730bfb185ea8c1cb3a8738e05e20bf1ea2ba11db1067324368252b4b` |
| `crates/vega_ui/src/cua_onboarding/mod.rs` | `ad30830d63ac2190896cc0b31e827818d73d67eccfff38c92858dead21ec7020` |
| `crates/vega_ui/src/cua_onboarding/model.rs` | `499d91737cb61c291c4e272cf3c76f5f54909063480e73966fd21eb134037b29` |
| `crates/vega_ui/src/settings/agents.rs` | `e4d8b110e843423bd40c70728a56d994861c8c73b9a7af03a9de5b852c6ce885` |
| `crates/vega_ui/src/settings/mod.rs` | `dcf9de93f8097a762d931d85354639dab75477c3a53133eb7a941bebb479c706` |
| `crates/vega_ui/src/settings/preferences.rs` | `cd2c7126e56c06425e09422fc4612e3ef7a971c41ad9f2d89387bd6ca785cc0c` |
| `crates/vega_ui/src/settings/render_impl.rs` | `01e46dbca798b55d28b0abbd3d7d3e0d2a3ecb8659f55a70276450a371446d8f` |
| `crates/vega_ui/src/settings/state.rs` | `b86da7aafbb59cb4880623c8fa56321c08374995981f9597f7fdff603ae11676` |

## Remaining gates and manual acceptance

Non-writer review must inspect model acquisition/release, cached versus current facts, Settings teardown/save callbacks, task-origin errors and visibility, native-follow/drag invariants, exact per-pane dispatch, keyboard single activation, tiny/error/development layouts and final complete content. Current Actions must establish fmt/all-target Clippy, actual Rust discovery/execution counts and retained Python checks; previous #537 GREEN does not validate this source.

After merge, the user must verify the actual compact visual result, real `.app` drag/drop, follow and permanent freeze, no activation theft, administrator-dialog visibility, keyboard/AX, multi-display/Spaces and TCC/restart behavior in a build containing the correction. GPUI's 200ms quit timeout still does not prove process cleanup completion. No app was installed, started, stopped, captured or modified by this correction work. Issue remains open until explicit user acceptance.

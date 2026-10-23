# #307 Stage B recovery and integration checkpoint

## Recovered version, 2026-10-22 17:17 UTC

The active agent directory is empty after continuation; no predecessor execution remains available to resume. The parent inspected the preserved artifacts before assigning a replacement. No existing artifact was reset, removed or regenerated.

- Committed UI-A: `212e50d5a0503fe462c2c2b5443c10c5656c4274`, independently reviewed PASS.
- Updated baseline: `48cef07bce9fdfb8187b2fae08873725b2ac9abf`, conflict-free merge of master `2ba904f7074115de5ee89cc18c376c083f803357`; incoming delta is documentation only.
- UI-A Actions run [39817506348](https://github.com/puzige/vega/actions/runs/39817506348): Nextest job `119959751923`, Clippy `119959752614`, required aggregate `119965500294` all success. Raw Nextest output: 28 tests across 15 binaries; 28 run, 28 passed, 0 skipped. These are 13 runtime + 15 UI tests. This does not validate the untracked and unwired Stage B artifacts.

Preserved untracked SHA-256:

| File under `crates/vega_ui/src/cua_onboarding/` | SHA-256 |
| --- | --- |
| `card.rs` | `75746b1d94c641d0d751bb7159b74b94f18ffae7e1407bbdb35ca30439a36600` |
| `card/view.rs` | `fbcdef5646d6c4b48ff43a5602fcf59293e01abff6afd3ad6557539ea7dd299c` |
| `session.rs` | `e860cdb86ebdc58f703f119a28e092096a9b0643f50fe83230a40132c3dd1b30` |
| `native.rs` | `122a7e9126862ce1beac7bb5c0fecd1a3430b643a5d77097cf1efdabc7d31fb4` |
| `native/macos.rs` | `e5c65124751445bb5edcec0db7bc7fd27133aef8cb19fcc7f994598b0faf03f0` |

The native files and their delivery note have a completed independent static PASS at these hashes. The card/session note still says implementation evidence pending; there is no READY result or independent review for those files. The next writer must finish the handoff rather than treat them as verified.

Frozen tests remain unchanged: UI `26971f63dbaa01dc31e68e76d6237b2ffcc1942234bdc4ade96055069e6e48f2`; runtime `48076fccdbe6546543d4246f45128adf5177b19172015f6b0d3c5ddd94328d2a`. The original UI missing-interface REDs and runtime RED/GREEN remain recorded in PR #537 and the existing test plans. No tests are added, removed or weakened in this integration stage.

## Unchanged acceptance and observation route

The main spec's latest guidance/status-only scope and the nine integration anchors in the frozen UI test plan control this work. Every preparation request remains false/false, component/AX/screen facts remain separate, actual capture remains unverified, and there is no automatic completion, dismissal, resume, old grant or old observation replay. Real TCC, dragging, focus, following, layering and Spaces require later user manual acceptance.

The recovered card already consumes CardController and contains synchronous begin_drag, exact bundle payload, bounded initial Settings acquisition, polling and cancellation hooks. The session adapter serializes/coalesces original tickets on an app-lifetime current-thread Tokio service and uses explicit PreparationProbe open/check/close/cleanup_pending APIs. These are inspected source facts, not compilation or behavioral validation.

Finish and review these invariants:

1. All renderer values and enabled actions use CardView. Mouse and keyboard share CardAction. Close/destruction/return invalidate before ownership drops. URL/Finder dispatch is not presented as verified launch success.
2. One successful preparation session is reused; errors preserve their identity and unresolved cleanup custody. Recheck is the explicit recovery action; do not automatically reopen or stop an unrelated ComputerUseControl. Cleanup status must not falsely report release or hide unresolved resource ownership.
3. Native adapter stays unchanged unless concrete evidence requires a scoped correction and renewed review. Preserve unique marker, nonactivating Floating focus:false options, points and display-ID pairing. Beginning drag freezes following for this lifetime, regardless of drop acceptance.
4. Opening from a retained entry validates both original TaskContext and thread ID against the emitting stream before consuming the event. Return selects only the original thread through existing navigation and never submits or resumes work.
5. Opening from Settings is available manually and after a persisted desktop-enable transition. A save failure must not start guidance or claim enablement. Keep browser behavior outside this change. Open failures remain visible with a retry path.
6. Host ownership prevents duplicate surviving cards; replacing or destroying an owner closes and invalidates its card. Old callbacks cannot close or update a newly opened card. Settings-origin cards need no fabricated task or return destination.
7. New geometry enters typed shared Layout tokens before use: visible body 480 by 420 logical points; transparent shadow allowance 24 per side (528 by 468 outer bounds). Existing placement policy remains unchanged. Existing Typography and ThemeColors remain the styling sources. Required explanatory/error text stays concise, actionable and truthful.

## Implementation sequence and write boundary

One replacement writer owns the recovered card/session files, their delivery note, production module wiring, Settings event/success integration, host subscription/window ownership/navigation, and narrowly related shared Layout tokens. Existing runtime/controller/lifecycle/tests/native files are read-only. Any need to alter those is reported first with concrete evidence. The parent alone owns Git/index/commits and this checkpoint.

Read applicable AGENTS, exec/design guides, the main spec and frozen test plan before edits. Finish the recovered work rather than redo it; inspect actual GPUI signatures from the pinned installed sources. No local Vega execution, builds, tests, Clippy, cargo check, dependency/workflow changes or new acceptance. Allowed validation is focused formatting, whitespace/diffs, static API/call-chain inspection and hashes. Return READY with exact changed-file hashes, commands/results, unresolved items, manual/CI gaps and explicit cessation of writes. A non-writer reviews that immutable version; existing Actions verify the unchanged 28 tests and complete gate after module wiring. Tests remain until the complete implementation is independently reviewed and GREEN, followed by a separate removal commit and latest-head empty-set full gate.

## Integrated review correction checkpoint, 2026-10-22 19:00 UTC

Implementation `aadae198e70959c789f0b86ff30b2c76c113f1d8` and conflict-free master integration `3e32543432d15952d986bda504ff703f63602073` are pushed. The writer stopped and the independent non-writer reviewed this immutable full head. Actions [39832532323](https://github.com/puzige/vega/actions/runs/39832532323) discovered/executed 28 tests, 28 passed and 0 skipped, but is **not full GREEN**: Clippy found the cross-module `sel_registerName` declaration conflict. Original evidence is preserved in [PR comment](https://github.com/puzige/vega/pull/537#issuecomment-6348465410). Correct the new native module's opaque selector declaration to match the existing const-pointer declaration without suppressing the lint or changing ABI/behavior; that isolated repair is assigned to the same writer in a separate worktree so the reviewed original source is immutable.

Independent review returned BLOCK on five existing-scope integration defects. Parent inspected the cited production and pinned GPUI source; these are not acceptance changes or new coverage goals. No test changes are authorized. The current implementation cycle remains open, the same 28 frozen tests remain required, and intermediate assertion success does not complete the feature.

| Defect evidence | Existing acceptance / minimal correction |
| --- | --- |
| `ProbeClient::cancel` only cancels a token; `Reclaiming` is published after the worker obtains the cleanup gate. A new card can see Clear, enqueue an automatic request, then retry a subsequently failed cleanup and acquire a session. | Explicit Recheck recovery and truthful pending custody: fence automatic initial checks at service admission, synchronously mark retiring owners, and revalidate the service generation/status before an automatic request drains or acquires. Do not alter runtime ownership/cleanup; no unrelated run cancellation. Manual intent may recover; automatic queued work may only surface the blocking state. |
| Pinned `gpui-pre 0.3.4` `elements/div.rs:2954–3017` maps focused on_click to Enter/Space keydown/up; custom keydown is registered later and can dispatch the same action twice. | One gesture, one action: use the built-in keyboard click path for on_click controls, not both dispatch paths. Preserve labels/roles/focus and unrelated Settings mouse-up keyboard behavior. |
| `overflow_y_scroll` supplies wheel scrolling, and focus_next/prev does not scroll; fixed-height failure content can hide permission controls. | R15 keyboard access: track real scroll state, provide keyboard scroll/reveal (including overflow failures) using pinned ScrollHandle APIs, and keep fixed close/recheck/return reachable. No visual-only claim. |
| Actual typed component-open errors clear preparation, leaving the independent component row as 未检测. | R1 factual component state: project MissingComponents/IncompatibleComponents/UnsupportedPlatform through CardView while retaining original typed failure and Unknown permissions. A narrow controller view projection correction is authorized; do not invent grants or test-side-only state. Keep frozen tests unchanged. |
| OpenUrl resets acquired Following to Seeking; a missing frame timeout enters Waiting, which never reads geometry again. | R12a: preserve acquired tracking across privacy-route actions, retain last position during loss and resume on reappearance. Only genuinely initial acquisition is bounded; drag freeze remains permanent. |

The same implementation writer will address these five narrowly scoped defects after the isolated selector repair is READY and integrated. Permitted correction source: card.rs, card/view.rs, session.rs, host window/cua_onboarding.rs, narrowly controller.rs for typed component projection, and a new correction report. All other runtime/native/test/shared files remain unchanged except the separately reviewed selector fix. No extra tests after implementation, assertions weakened, ignores, retries-to-green, acceptance revisions or new dependencies. Static source review confirms these are original integration requirements already present before Stage B; any unexpected requirement change must stop and reopen the applicable test-first cycle. After repair, the original independent reviewer reviews the delta and affected invariants at a new stopped exact head, followed by the unchanged-test full cloud gate. Historical findings and failed gate remain visible.

## Selector repair verified; behavioral corrections pending, 2026-10-22 19:45 UTC

Selector-only commit `2e0089cf9cdc769ebbacccbc3831a4157ef41f28` received an independent non-writer PASS. It introduces the alias-equivalent `Selector = *const c_void` and applies it to all 13 dispatch selector slots, preserving both architecture ABIs, object arguments, geometry and ownership. The repair is integrated at current pushed HEAD `757c851e2a3c0daf881b1ba064dc4ec5236f02dc`; see the [narrow report](vega-issue-307-selector-declaration-fix.md) and [independent evidence](https://github.com/puzige/vega/pull/537#issuecomment-6349020243).

Actions [39837897922](https://github.com/puzige/vega/actions/runs/39837897922) is full GREEN at that exact HEAD: Nextest job `120030933592`, Clippy job `120030933870`, and required aggregate `120032957529` all success. The original Nextest output reports 28 discovered across 15 binaries, 28 executed, 28 passed, 0 skipped; Python checks executed 5, 15 and 40 tests. The [durable receipt](https://github.com/puzige/vega/pull/537#issuecomment-6349218192) retains original timestamps and gate details. This is intermediate validation of the selector repair, not clearance of the five behavioral findings.

A fresh fetch at 19:32 UTC found no additional master delta: `origin/master` remains `6615748f91273bfb8ee424011b3b5a09b3777d2b`, already integrated. The five bounded corrections remain with the original implementation writer; they are not included in the pushed HEAD or this GREEN. The frozen runtime and UI test hashes were rechecked unchanged. PR #537 remains draft, Issue #307 remains open/In progress. Next gate: a stopped exact correction version, independent affected-invariant review, and same-test full cloud validation. No local Vega execution or native/manual acceptance is claimed.

## Correction handoff and refreshed baseline, 2026-10-22 19:55 UTC

The original implementation writer returned READY and explicitly stopped writes. The parent independently matched the five-source SHA-256 manifest and production diff identity in the [correction report](vega-issue-307-stage-b-review-corrections.md). Focused rustfmt and whitespace checks exited 0; frozen tests and both native files remain unchanged. READY is not independent PASS or current-head cloud GREEN.

The precommit fetch found new master `5fb9a3d0491c620a901e67a0e3f348ba05bd76c6` (#561, native-delegation wait/context bounds). The baseline guard stopped before staging or committing. Inspection found no source, instruction, dependency, workflow or frozen-test overlap with these corrections; the parent merged it conflict-free as `e4b1fc33e7c360c28bfc20581c535109f965c294`, preserving all five source and two test hashes. The writer's report and diff remain historical evidence relative to its original `757c851e2a3c0daf881b1ba064dc4ec5236f02dc` baseline. The next correction commit combines those byte-identical sources with this updated baseline and documentation; independent affected-invariant review and the unchanged full cloud gate must bind to that new complete head before proceeding.

# #907 Composer action chrome

## Primary action completion increment

The current production Composer still paints Send/Resume and resume-pending with GPUI. Replace these two slots through the existing NSH action host, leaving its native event/layout bridge paint-free. Preserve the original show conditions and the shared `action_focus[1]`; Send/Resume activation must use `activate_composer_primary` only after current source, original interrupted-tail identity, draft revision, mount, visibility, measured display and operation gates are rechecked. Resume-pending is a disabled busy state, never another executable Resume request. NSH paints the actual icon, focus, hover/pressed, disabled colors and lifecycle/reduced-motion-aware busy motion. Stop/Menu behavior remains intact. Development regressions must cover stale source/kind/identity refusal and actual NSH button/disabled rendering. Parent owns this increment and its shared `render.rs` integration; no intermediate independent review is scheduled under the latest user instruction. Local compilation/regressions remain required; native pixels and three-platform acceptance are not implied.

The parent increment now also replaces the production running Queue/Steer send and Add Context trigger. Queue/Steer identity is captured with source revisions; dispatch remains the guarded existing submission path. Add Context retains `action_focus[0]`, the measured original trigger bounds and existing open/close business routine. The actual visible icon/hover/press/focus/disabled rendering is NSH. A local exact-presentation publication gate preserves stable retained native controls while still checking authority on every action. This is implementation, not runtime acceptance: the first scoped Cargo attempt was blocked by in-flight editor-host test E0308 at `vendor/nsh-platform-gpui/src/text_input/host_tests.rs:10` before the app compiled (`target/nsh-composer-primary-test.stderr`). No action tests executed in that attempt. Parent will validate a coherent integrated version after the parallel writers stop; no intermediate review is requested.

## Scope and acceptance (before implementation)

Baseline: `4148291cc5b377362d0ac64aa88b285cc9e06532`. Independent leaf scope owns only `conversation_stream/composer_actions.rs`, its new `composer_actions/**` implementation, and this document. Existing main input, attachments, thinking, shimmer, shared NSH, parent/core/render and other action files are not owned. No dependency/fork changes, builds, network, Git mutations, application execution or delegation are authorized for this leaf.

| Boundary | Required observable result |
| --- | --- |
| Action popup | NSH draws existing controller rows, command tokens, selected modes, skill names/descriptions/source disambiguation and selected skill checkbox state. Existing row order and matching remain authoritative. The current slash/skill search is a read-only projection, not another draft input. |
| Disabled/loading/empty | Keep each original localized disabled reason visible and in its accessible name. Disabled rows cannot activate. Pending skill projection shows actual loading; a completed empty search shows no matching results. No automatic selection/execution or invented success. |
| Keyboard and AX | A real GPUI host focus handle bridges the pinned ordinary-NSH-View limitation without adding keymaps. Tab/Shift-Tab traverse eligible rows, Enter/Space activate only the current focused row, Escape dismisses, and boundaries leave the composite. NSH paints focus separately from selection; paint-free AX proxies use the same guarded activation. Scrolled targets must be revealed before keyboard activation. |
| Staleness | Every activation rechecks exact presentation/mount/epoch, menu generation and kind, input entity/content revision, skill projection generation and candidate identity, thread/backend/project/draft route, projected provider/model identity, enabled/running/stopping state, active assistant identity, permissions, preview, Settings and actual window/mount geometry. Existing trusted controller validation is retained, not replaced. |
| Stop | NSH draws the original stop/stopping state and retains the stop API. Stop eligibility follows the existing worker/pending/queue cancellation condition, not assistant permission to execute. One explicit activation requests stop through `request_composer_stop`; no replay or draft clearing. |
| Lifecycle/API | `RenderOnce` obtains Window without changing callers. Parent synchronous `run_notice_epoch` retirement invalidates prior callbacks. Dropped/unmounted hosts cannot act. Existing structs, rows, command availability, routing, generations, selection and public APIs remain intact. |

## Review correction acceptance (before corrective code)

1. Original main-input Up/Down highlight changes must drive the NSH reveal target, including when the host has no row selection. Original input Enter accepts only a currently measured/displayed exact highlight; an early Enter is consumed without queuing or replaying execution. This narrowly adds a private display fence to the existing accept controller; composing checks and all downstream business decisions stay intact.
2. Pending keyboard activation is bound to its exact originating native focus handle and selection session. Leaving the composite cancels pending activation and retires that session before republishing. Pointer and AX activation are explicit distinct origins, not inferred keyboard intent; old callbacks cannot mint a new intent.
3. Both host and child Surface focus participate in the guarded capture bridge. The child Surface is not a separate Tab stop. Blank/disabled-row clicks must not strand Tab/Shift-Tab/Enter on an unhandled inner focus; no keymaps are added.
4. An oversized reveal aligns its top directly, clamps negative destinations, and settles rather than alternating top/bottom. Production render/controller decision tests must cover input highlight reveal/accept, canceled focus intent, explicit origins and oversized settlement, beyond geometry-only tests.

No builds, network, Git mutations, app execution, delegation or writes outside the owned paths are authorized for this correction. Parent-reported `cargo check -p vega` success and 193 tests (192 passes plus an attachment observer failure fixed separately) precede this correction and do not validate it. The parent's sole `pixels.rs` compiler import fix was read fresh and verified at SHA-256 `78507c7834d5e1776254e50bcf4a8e16ad18b0b4281b64df9fede66a27e1b15b`.

## Explicit gaps and validation boundary

Composer native IME/composition dispatch remains unresolved in pinned NSH. This migration does not change main input, add global Enter/Space bindings or claim native composition safety has been repaired. Host key events must reject character-preferred/composing input. No full Composer migration or three-platform/native keyboard/AX success is claimed by source changes.

Parent owns compilation, regression execution, independent review and final-head CI. The earlier 175 passing tests are baseline evidence, not evidence for this delta. Any new development tests are UNRUN here and must follow repository test archival/removal policy before merge.

## Implementation and integration seams

- Both existing render signatures remain unchanged. Their `NativeChrome` RenderOnce adapter acquires Window and keeps mount state in a keyed host; no parent/core/mod/render changes are needed.
- The corrected pre-render controller delta is limited to the obsolete renderer-only InteractionExt import/module wiring, two private display/navigation fields, Up/Down display invalidation and a display-acknowledgement fence before original input acceptance. Row building/filtering, command availability, existing generations, downstream selection/draft mutation, permission/provider/model/skill routing, stop and all public APIs retain their original implementation.
- NSH draws the popup, current query projection, skill source ordinal/description, command token, selected checkmarks, disabled reasons, pending/empty state and stop control. Real host focus handles bridge Tab/Shift-Tab/Enter/Space; stop reuses `action_focus[1]` and the unchanged `stop_composer_action` → `request_composer_stop` path. No assistant permission gate is added to cancellation.
- Captured authority adds full thread/default/provider-model options, model pending owner, active assistant/response, attachment generation, permission/busy state and exact skill projection to the original generation/menu/input-revision guards. Parent `run_notice_epoch` retirement rejects old callbacks immediately without a parent edit.
- Mounted geometry is retained only by the live rendered element state. Activation compares viewport, visible mount, trigger bounds and foreground geometry; changed geometry invalidates the selection observation. Native row measurements gate activation. Keyboard/AX activation requests a fresh measurement; actual selection can reveal an offscreen row without executing it. Disposed presentation tasks are rejected; no retries or automatic business activation are added.

## Initial implementation checks (historical; superseded by correction below)

HEAD remained `4148291cc5b377362d0ac64aa88b285cc9e06532`. No Git mutations, network, builds, application execution, dependency changes or nested delegation were performed. Other writers' changes were observed outside this scope and left untouched.

Executed successfully:

1. `rustfmt --edition 2024 --config skip_children=true` on the four owned Rust files; final `rustfmt --check` over the same files returned exit 0. Tool reports `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`.
2. `git diff --check -- crates/vega_ui/src/conversation_stream/composer_actions.rs` returned exit 0.
3. Read-only Python comparison against `git show 4148291…:…/composer_actions.rs` confirmed the controller region before `render_composer_actions` is unchanged except import/module wiring. Scoped whitespace and no-new-comments/unwrap/expect inspection also passed. These are static checks, not behavior tests.

Three development tests were authored in `native/pixels/tests.rs`: clipped actions require reveal; oversized actions require viewport-start alignment; missing/nonfinite geometry rejects visibility. **All three UNRUN.** They cover the visibility decision only, not host/engine integration or the security fence. Archive/remove development Rust tests and their cfg wiring under the parent delivery policy before merge.

**UNRUN / parent gates:** Cargo check/Clippy, regression suite, independent review, final-head CI, native pointer/Tab/Shift-Tab/Enter/Space/Escape/AX, geometry/scroll/hide/remount races, theme/narrow-window inspection and all three desktop platforms. Actual Cargo/rustc versions were not queried by this leaf. The baseline 175 passes do not validate this delta.

**Known remaining acceptance gaps:** pinned native Composer IME is unchanged and unresolved; no claim of native composition support. Paint-free AX proxies currently use composite host bounds rather than per-row measured bounds; their activation is guarded and their checked state is exposed, while disabled state is provided by NSH plus the proxy reason/guard rather than a native proxy disabled flag. Native AT bounds/disabled fidelity needs parent review and cannot be claimed passed. Transport/engine errors fail closed; this leaf does not introduce a new retry/error-recovery UI. Host activation/reveal logic needs integrated runtime regression coverage before acceptance.

Initial implementation Rust SHA-256 (historical, not the corrected stopped artifacts):

| Artifact | SHA-256 |
| --- | --- |
| `composer_actions.rs` | `66e02ce5433465e0cf396c4151b1cc9dc3a7016cd35d935670f2a3ac123492cd` |
| `composer_actions/native.rs` | `55aefc4eca50a4e3c236182507949f05386b1073f4745ac88c8e07b4cc0319fe` |
| `composer_actions/native/pixels.rs` | `dc00f3e1cee79d6fbe5124df72049b978808b1b704a9113554a76a646393a76d` |
| `composer_actions/native/pixels/tests.rs` | `47653e1976c908c0949ba44d75f3c4473e95d7f4a0fbafead2c2b434d47c4c36` |

## Corrective implementation and stopped evidence

Parent integration has now executed all ten Composer regressions successfully within Nextest `cb12b46b-ba67-4897-87bf-1d1c399dbd4e` (full run 203/203, 0 skipped), plus workspace strict Clippy/fmt/diff checks. Independent incremental review closed all four original P2s statically, without new findings, on current `native.rs` SHA-256 `a8c80ee858962677799f38936456f809eecfdf0044fbfc7fc3ab9379a13938a8` and the other supplied stopped hashes. That file includes the parent's equivalent `.then_some` lint correction. See [batch validation](nsh-header-batch-validation.md). The writer-local UNRUN statements below are historical handoff status; native focus/AX/IME and final committed-head CI remain unverified.

The four review findings are addressed in the owned code:

- NSH reveal uses the exact controller highlight when no host row is selected. Highlight/navigation are presentation identity: a navigation round trip cannot borrow a previous acknowledgement. Original input Enter calls a private display fence that checks the live weak host, exact presentation/selection/source, current authority/geometry, input focus, enabled row and current displayed acknowledgement before the original selection method. Early Enter is consumed, never queued. The already-borrowed stream is passed into the guard, avoiding recursive entity reads. Old GPUI scroll calls remain for API/controller compatibility but are not the NSH reveal mechanism.
- Keyboard requests retain an exact originating GPUI focus handle plus the exact selected row/session. Both composite focus-out and exact host/Surface blur cancel keyboard intent; composite departure retires selection before republishing. Fresh Displayed cannot resurrect canceled intent. Pointer and AX carry explicit origins; AX still requires an explicit guarded click and fresh measurement. No automatic business activation is introduced.
- The guarded key capture accepts descendant focus (`contains_focused`), and each mounted Surface focus handle has `tab_stop(false)`. Composite entry chooses the correct first/last eligible row without stealing child pointer focus or unnecessarily invalidating the displayed pointer session. Per-Surface blur subscriptions are released when the presentation stops.
- Oversized rows request top alignment directly, with nonnegative clamping and no redundant scroll at an unchanged destination. The original exact visibility gate is preserved: a clamped but still clipped row is not acknowledged for activation. No visibility tolerance was added.

### Checks actually executed for the corrective bytes

- `rustfmt --edition 2024 --config skip_children=true` and final `rustfmt --check` over all five owned Rust files: exit 0. `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`.
- `git diff --check -- crates/vega_ui/src/conversation_stream/composer_actions.rs`: exit 0. Separate all-owned Rust whitespace and production no-new-comments/unwrap/expect audit: passed.
- Read-only Python comparison to exact baseline confirmed that, after removing the explicitly enumerated import/module/private-state/navigation/accept-fence delta, the pre-render controller bytes are identical. The first audit had an incorrect assumption about the baseline InteractionExt import path; inspected unified diff showed this was an audit normalization error, corrected to the actual `crate::interaction` import, then the comparison passed. No product assertion was relaxed.
- HEAD remains `4148291cc5b377362d0ac64aa88b285cc9e06532`. No build/test/compiler invocation, network, Git mutation, application execution, dependency/fork change, delegation or shared-file write occurred. Other writers' artifacts were not changed.

### Development tests: 10 total, ALL UNRUN

Seven added regressions plus the three original visibility regressions:

- Four production Snapshot/controller-decision tests: input highlight reveals without host selection and requires current display; focus departure rejects both old and republished intent; explicit pointer/AX cannot borrow keyboard intent; revision/menu/skill generation/thread/model/permission/epoch/mount/selection changes reject old observations.
- Two actual NSH Core mounts using production `pixels::render`: popup descriptions/source/disabled reason and AX disabled state with no mount activation, plus stop render state independent of assistant permission. These are renderer tests, not native GPUI focus/IME or controller capture acceptance.
- One production reveal-decision regression for oversized direct top alignment, settled alignment, clamping and nonfinite geometry; three original strict visibility regressions retained unchanged.

These tests have not been compiled or executed. Parent must validate the corrective delta, then archive/remove the development Rust tests, fixture and cfg wiring before merge per repository policy. The parent-reported pre-correction check/test results and the old 175-pass baseline are not results for this delta.

### Remaining gaps / parent verification

Required: current-byte Cargo check/Clippy, task regressions, reviewer delta and final-head CI. Native GPUI focus ordering, blank/disabled click → Tab/Shift-Tab/Enter/Space, keyboard activation then immediate focus departure and return, AX and pointer activation, original-input Up/Down → reveal → Enter, early Enter without replay, oversized long skill rows and host/engine scroll/geometry/hide/remount races remain unexecuted integration checks. The pure decision tests do not prove the native focus tree or asynchronous engine scheduling.

Pinned Composer native IME remains unresolved; no new keymaps or native composition claim. Existing composite AX proxy bounds/disabled-fidelity and transport-error recovery gaps remain as recorded above; no scope expansion into shared NSH or main input. Stop routing and assistant-permission independence remain unchanged.

Corrected stopped Rust SHA-256 (paths relative to `crates/vega_ui/src/conversation_stream/`):

| Artifact | SHA-256 |
| --- | --- |
| `composer_actions.rs` | `28a5256a1078acbc88c8c1ef904237294f24fae45ef205150ee9397d11469ea4` |
| `composer_actions/native.rs` | `db9d3048ab816dcc0ebaa993d5eed9c3d4052dd9ca0b8b29b4990f6ed784f18d` |
| `composer_actions/native/tests.rs` | `8a4cbab022854b105e3ba0b0e195c86f374ade053353e6ab08a5840321196a2d` |
| `composer_actions/native/pixels.rs` | `8887d78a47d6776b4176e28161d96edf7fae441e7c2ad27c92aa5b8c14b7156d` |
| `composer_actions/native/pixels/tests.rs` | `b369b036f5be90b06207ac2b046ea211cc38031d4607c4e9382d9b5ffaf32796` |

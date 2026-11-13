# NSH production run notices

## Scope and baseline

Authorized isolated leaf migration for #907, requested baseline `bd89db7d182e6d9a5c60b1388e79a4623e8cacc4` with parent-owned dirty integration work. Write ownership is limited to `crates/vega_ui/src/conversation_stream/run_notice.rs`, new files under `conversation_stream/run_notice/`, and this document. No parent modules, core/render/run_activity/privacy, shared NSH, dependencies, Git/index, or unrelated UI changes. The current user forbids network and Git mutations; no fetch/update is performed. Parent owns builds, integration, independent review and platform acceptance.

## Spec and acceptance established before implementation

- Keep `RunNoticeKind`, `RunNotice`, `text`, `can_resume_credentials`, `estimated_height`, projection/merge/controller APIs and event contracts behavior compatible. Source error/retry strings, compaction ordering and line breaks must remain exact; do not change business state, retry decisions or replay anything automatically.
- All visible notice content is NSH: retry Wifi icon and secondary conversation typography; plain danger-colored error/MCP/compaction text without a card; existing Continue and Check credential settings actions only for the original eligible failures. Reading previews and revoked permissions omit actions.
- Use existing public `Surface` and `OrderedCommands`; retain GPUI Fast solely as paint-free layout/lifetime/native-service host. Do not wrap visible old GPUI labels/icons/buttons. Reuse theme, typography, translations and shared icon source.
- Native actions must have accessible names, keyboard focus and visible focus state, with an actual native dispatcher for keyboard activation rather than unconnected engine handlers or new app keymaps. A paint-free Fast focus/AX host may dispatch into the same guarded leaf action route while NSH paints the controls.
- Continue emits exactly `CredentialContinuationRequested { thread_id, message_id }`; Settings sets the existing `SettingsOpen(true)` and refreshes windows. Both routes must recheck current owner, opened thread, reading-preview/revocation/settings visibility, exact current notice and source row identity, source visibility and mount/viewport authority. Stale/recycled/removed/replaced/hidden controls cannot mutate state merely because an old host remains mounted.
- Surface failure must not expose old GPUI controls or trigger business retries. Preserve the source notice; any presentation-only recovery must remain explicit and cannot resume credentials automatically.
- Obtain Window within the local RenderOnce/native entity host without modifying the existing caller signature. Do not change parent-owned files or weaken native dispatch or lifecycle acceptance.

## Parent integration contract

The initial investigation stopped before production changes because mount/notice/thread checks alone could not detect synchronous withdrawal during loading or thread replacement. The parent supplied the selected seam before implementation resumed:

- Stream-owned `run_notice_epoch: crate::nsh::ProjectionId` is initialized fresh.
- `suspend_ui_presentations` replaces it synchronously at the start of suspension, even when the old opened-thread, notice and source entries remain unchanged.
- Only actual row rendering captures the current epoch. A retained host cannot capture a fresh epoch on its own, and callbacks cannot rearm a suspended presentation.
- Detection of an epoch mismatch stops the old local surface, revokes its lease/selection and rejects the callback. A subsequent actual row render can establish a new source projection. No queued request is replayed.

These field/initialization/suspension changes were verified in actual current core source and remain parent-owned. No additional integration hook or Window argument change is required by this implementation.

## Implementation

- `run_notice.rs` delegates only rendering to the new leaf; all notice text, eligibility, estimated heights, projection/merge and controller methods remain unchanged from the requested baseline.
- `run_notice/presentation.rs` uses a RenderOnce row adapter to find the exact current source row and identity, then retains a native host entity with keyed Fast element state. The host owns the Surface and its main-thread request consumer; entity rendering updates keyboard focus/selection without relying on a parent row callback being rerun.
- A separate prepaint lease records clipped notice visibility, list viewport, source-row bounds and foreground obstruction. Activation checks the live lease and unchanged current geometry, not mount existence alone. Notice/source replacement and recycled mounts stop the old surface and use a fresh local projection identity. Entity release stops its Surface.
- Pointer requests use `OrderedCommands`; native keyboard and AX requests call the same guarded trusted handler. It recomputes the current notice projection and checks exact notice/index/identity, thread/opened thread, preview/permission state, Settings, source visibility, current epoch, local projection, surface health, active window and geometry before performing either original action.
- Fast paints no notice label/icon/button/focus decoration. It provides a native focus group, capture-phase Tab/Shift-Tab and Enter/Space dispatcher using the existing keyboard helper, plus accessible action nodes for both available buttons. NSH paints text, Wifi SVG, hover/pressed state and selected focus border.
- `run_notice/presentation/pixels.rs` renders the exact source text using existing typography/theme/i18n/icon inputs. A measured NSH content-height request updates the local host height and invalidates the source list row, including wrapped text/actions in narrow layouts. The original estimated-height API is unchanged and supplies initial layout only.
- Surface errors reject actions without exposing legacy controls, changing notices, or resuming credentials. No automatic business or presentation retry is added; a failed surface can remain failed until a genuine row remount. Failure/recovery visibility still needs native acceptance, and no claim is made that a failed engine can paint its own source text.

## Actual verification

- Requested baseline: `bd89db7d182e6d9a5c60b1388e79a4623e8cacc4`. During this shared-tree stage, read-only HEAD observation returned `ee3947ac0f17b503e6efb635e0c383cea99576af`; this leaf performed no Git mutation. Final exact leaf hashes are returned separately in the stopped handoff.
- Actual formatter: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`.
- Direct scoped formatting and `rustfmt --edition 2024 --config skip_children=true --check` passed for the three scoped Rust files. No workspace formatter or build was run.
- Scoped `git diff --check` passed. A read-only Python comparison against the requested baseline confirmed unchanged notice kinds/text/credential eligibility and all estimated-height/projection/controller code after the renderer. This is a static source-preservation check, not a behavior test.
- Static scan verified terminal newlines/no trailing whitespace and no added comments, unwrap/expect or Rust tests in the new leaf files. No existing tests were edited or weakened.
- One atomic edit failed because its context did not match; it made no changes, the relevant source was reread, and the corrected edit succeeded. Direct formatting/checks subsequently passed.
- No Cargo/rustc/Clippy/test execution, app launch, network, dependency/config/keymap change or Git mutation was performed. The parent's earlier permission failure was not retried or bypassed.

## Parent verification and limitations

Code is stopped for parent compilation, integration and independent review, not native acceptance. Parent should exercise exact failure/compaction/retry text, Continue event payload, Settings routing, queued callbacks across suspension/preview/permission loss/thread replacement/source removal/recycling/scrolling, delayed old-surface errors, narrow-window wrapping/row measurement, keyboard traversal/activation, AX action names/dispatch and light/dark themes. The pinned platform's plain NSH View controls do not enter its native input Tab collector; the paint-free dispatcher is the local route, not a dependency workaround.

Compilation, regressions, independent review, final-head required CI and macOS/Windows/Linux native keyboard/AX/pixel/failure acceptance are all unrun for this leaf. Static API inspection and formatting do not establish these outcomes. Parent must retain the epoch seam in its final integration; no broader batch, native acceptance, merge or release is claimed here.

## Development renderer regression addition

The parent reports that the integrated leaf compiled, all-target Clippy passed, 166 regressions passed and independent review was clean before this addition. Those results do not validate the new tests. This stage changes only `run_notice/presentation/tests.rs`, its terminal test-module declaration in `presentation.rs`, and this appended section; production behavior is unchanged.

Acceptance for the development tests: mount the actual `pixels::render` through NSH `Core`/`Component`/`Root`; preserve source notice kind and exact `RunNotice::text()` output, including compaction line breaks; retrying notices expose no Press action; credential repair exposes the exact translated Continue and Check credential settings text and accessible button names. Disabled projections omit actionable controls. Dispatch actual `EventKind::Press` and inspect the resulting `Request::Activate`, including immutable projection, epoch, thread, message, row index and `StreamEntryIdentity`. Apply replacement projections through a component-owned signal updated by `ns::spawn` and a channel, then exercise retained callbacks before reconciliation: old actions must retain old authority rather than borrow the replacement. Use production `Source::matches`, not a duplicated matching algorithm. No ConversationStream, business action, native app, dependency or shared harness changes are needed.

The parent owns compilation and execution after this stage stops; this stage will not run builds or tests alongside other work. These are development-only Rust tests and their module declaration must be archived with exact hashes/results and removed before master. They do not establish trusted-handler, native focus/AX dispatch, compositor pixels or platform acceptance.

Added three development regressions: mounted source text and exact action/AX names across light/dark, narrow/wide and enabled/disabled projections; both production Press actions retaining their displayed authority across separately changed projection, epoch, thread, message, row, entry, notice and preview with pending/applied signal updates; credential-to-retry reconciliation removing old callbacks without replay. Assertions inspect mounted text/roles/frames and actual request payloads rather than source strings or a duplicated Source algorithm.

Actual checks in this addition: direct `rustfmt --edition 2024 --config skip_children=true` and its `--check` passed for `presentation.rs` and `presentation/tests.rs`, using `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`. A read-only scoped Python check passed for terminal newlines, trailing whitespace, terminal test wiring and absence of comments/ignore/source inclusion in the new tests. Compilation, Clippy and all three new regressions remain unrun in this stage; parent should execute the `conversation_stream::run_notice::presentation::tests` filter using its normal authorized validation command. No build, app launch, network or Git mutation was performed. Source baseline supplied by parent for this addition: `ee3947ac0f17b503e6efb635e0c383cea99576af`; final stopped file SHA-256 values are supplied in the handoff.


## Parent integrated validation (current development batch)

Baseline `ee3947ac0f17b503e6efb635e0c383cea99576af` plus this stopped delta. Format, workspace all-target Clippy `-D warnings`, and Nextest run `4eb222ed-ad9e-4ff9-8cd6-1ccea4c84ec8` passed: **175 discovered /175 executed /175 passed /0 skipped** across16binaries; runtime0.294s. Existing Python release/preparation/general groups passed5/15/40. Actual tools: rustc1.98.0, Cargo1.101.0 (huacnlee0.102.0-9), Nextest0.9.151; CI versions/dependency pins unchanged. This is headless development regression evidence, not native focus/AX/IME/pixels or Windows/Linux acceptance. Final exact-head CI is pending. Rust tests and exclusive wiring must be preserved as recoverable development evidence, then removed before master.

Failure diagnosis: the initial competing-publication tests found two separate layers. Production command tasks spawned inside a dynamic listener inherited its short-lived owner and could be cancelled before delivery; the two notice renderers now capture the stable component owner outside dynamic and use it only for action-command spawning. Exact observed snapshots still pass existing trusted epoch/route/permission/mount checks; disposed listeners cannot be revived. Tests initially assumed an applied replacement kept old listeners callable and that commit always returned a complete mount batch. Pinned NSH instead drops disposed listeners, emits incremental batches and can return no batch for unchanged pixels. The corrected observer handles Remove **and Delete**, preserves unchanged nodes, asserts zero old actions after disposal, and checks exact current labels/payloads after reconciliation; pending publication still requires both exact old-source requests. Non-implementer static delta review found no actionable issue, and all three new notice tests executed in the175-test run.

# #907 native editor backend contract

## Production editing closure acceptance (current continuation)

The third implementation lane owns native editing only. Preserve the independently checked Native/Synthetic clipboard carrier and all existing budgets, mutation provenance, focus/composition/secure fences. No Core, Kit, Fast, dependency, OS HTML or AX Paste work is authorized. This continuation does not claim native OS acceptance.

- The actual recovery-page Reload request must validate its projection identity and failed current session, preserve the latest local draft, retire the bridge, stop the old editor and recovery surfaces immediately, and remount authoritative text, directed selection and bindings. Attachments remain owned by the existing conversation/importer. Stale or duplicate requests must not stop the new mount; revision exhaustion must reject without wrapping.
- A fresh native hook registration and its initial receipt must settle before submission. Restore remains an explicit owner/serial/revision-fenced authority operation, retaining original TextInput undo and mention behavior, and requires another fresh native mount before send readiness.
- Native context-menu Undo and Redo must remain available when application hooks own history, including after remount clears opaque model history. Each actual LocalItem action invokes the current key policy once; declined opaque history remains fail-closed without a guessed mutation. Composition, stale focus/mount/registration and secure guards remain in force. Legacy Undo keeps its existing model eligibility; legacy Redo uses the actual model redo operation, not an invented can-redo API.
- AX SetValue continues through the existing ordinary full-display replacement boundary; it is not Paste, does not read the clipboard and does not claim HTML preservation. Existing fixed-version APIs suffice; access.rs and dependency sources need no edit.
- Development coverage must retain old assertions and exercise production recovery request handling, fresh hook registration, actual native menu actions and AX replacement through the adapter/authority boundary. Parent coordinates Cargo and shared export/runner wiring; source-only checks are not executed behavior evidence.

## Third implementation lane delivery

Implemented in this continuation:

- `TextInput::reload_native_editor` is shared by the recovery-page `ReloadEditor` consumer and `EditorOperation::ReloadNative`. It validates the current projection, failed session and checked selection revision before changing anything; preserves the latest observed local text; fails/releases adapter-owned pending paste reservation; retires/stops editor and recovery surfaces immediately; clears composition and schedules the existing fresh Surface construction. Old request identities and duplicate reloads are rejected. Existing authoritative bindings/history and conversation attachment ownership are not replaced by recovery storage.
- Native edit-menu Undo eligibility includes current nonsecure application hooks, so new native models after authority projection/remount do not hide retained application history. Redo is added through the same LocalItem closure and current identity/registration/composition checks, using Mod-Shift-Z on macOS and Ctrl-Y elsewhere. There is one guarded policy call and at most one existing fail-closed fallback. The opaque model exposes `can_undo` but no `can_redo`; Redo is enabled for editable nonsecure fields and a legacy empty redo stack remains an ordinary no-op. No opaque-history reconstruction was introduced.
- The existing fixed-version AX route was inspected: `access::perform(Action::SetValue, Value)` calls `set_accessible_value`. It remains ordinary full-display mutation, not clipboard paste. No access source change or new OS/dependency API is needed. Native HTML OS acquisition is still not implemented or claimed.

Development additions, not yet executed:

1. Export/wrap `nsh_platform_gpui::text_input::host_tests::native_menu_history_survives_empty_model_history_and_redo_uses_policy_once` through the parent-owned `host_regressions` list and host runner. Covers accepted/rejected actual Undo/Redo LocalItem actions on an empty model history, exactly one policy call, unchanged native tuple/serial/count and refusal retirement without guessed mutation.
2. Export/wrap `native_menu_redo_rejects_composition_stale_registration_and_secure_history` in the same list/runner. Covers marked CJK/surrogate display, no policy during composition, stale registration and secure Redo eligibility. All existing Undo/mutation/paste regressions remain present.
3. Add `vega_ui::text_input::mounted_native_recovery_history_ax_and_submit_close_production_loop(&mut TestAppContext)` to the parent's real-text-system integration runner with the existing `nsh-reading-integration` feature. No lib/manifest/runner edit was made by this lane. The function is re-exported only through the owned text_input module. Run in the same serialized real-font context as the existing mounted NSH tests, not the fontless standard unit harness.

The mounted regression uses the actual transactional renderer, native Input, registration/receipt queue, actual LocalItem actions and production authority commit. It checks original attachment delivery once to ImagePaste, retained mentions across typing and Undo/Redo remount, ordinary AX full replacement and undo, marked-composition menu exclusion, failed native receipt recovery via the rendered Reload button's Press handler, stopped old surface, stale reload rejection, revision-exhaustion rejection, explicit rendered Restore button, continued typing, and exactly one eligible production ComposerShortcut::Send event. A failed observation is deliberately injected before its queued authority delivery, not replaced by an alternate editing algorithm. Its test-only AppView methods live in existing feature-gated text_input/host_tests.rs; button activation refuses disabled or detached nodes. Keyboard/focus/IME drivers invoke real native backend entry points but are in-process test operations, not OS input acceptance. The submission endpoint is the production event consumed by ConversationStream's existing controller, not a real model/network send. Attachment decoding/importer completion and conversation external storage are not mocked as completed.

Actual checks: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`; scoped edition-2024 formatting and `--check` over the five changed Rust files exited 0. Read-only checks confirmed final newlines/trailing whitespace, retained selected existing regression definitions and no comments in the new test module. No Cargo/rustc query, build/check/Clippy, tests, native launch, Git/index, network, install/config or independent review ran. Parent-supplied previous 33 host / 287 application passes describe the prior version, not these bytes. Parent now coordinates compilation, test execution and unified independent review; do not report this lane as behaviorally accepted before those results. Development-only Rust code remains subject to pre-master cleanup.

## Historical implementation record below

The remaining sections preserve earlier contracts and blockers as historical evidence. Current mutation, visual-key-context and original clipboard APIs supersede their tuple-only/paste-unavailable descriptions; see nsh-native-mutation.md and nsh-native-paste-fidelity.md. This continuation does not reopen the independently checked no-copy carrier implementation.

## Ownership and actual API inspected

Agent2 writes only application editing files and the two editing documents. Parent owns all vendor, shared Surface, manifest and lock changes. NSH 0.4.0 / Kit 0.7.3 / Fast 0.1.9 stay pinned. No dependency or renderer is added. This document records an implemented application adapter and remaining blockers, not native acceptance.

Read `vendor/nsh-platform-gpui/src/editing.rs`, `app.rs`, `input.rs`, `text_input.rs` and the actual `crates/vega_ui/src/nsh.rs` Surface wrapper. Current stable public API:

- `AppView::input_identity(node: u32) -> Option<InputIdentity>`
- `AppView::set_input_hooks(&InputIdentity, Option<InputHooks>) -> bool`
- `InputIdentity { surface: u64, generation: u64, node: u32, focus_epoch: u64, private mount: Rc<()> }`, with `same_mount` and `same_focus`.
- `InputHooks { key: Rc<dyn Fn(InputKey, InputIdentity) -> bool>, edit: Rc<dyn Fn(InputEdit)>, focus: Rc<dyn Fn(InputIdentity, bool)> }`. Main-thread synchronous callbacks; not Send/Sync requirements.
- `InputKey { virtual_key: u16, shift, control, alt, meta, down, repeat }`. Virtual keys are Windows VK values even on macOS, as verified in `keys.rs`.
- `InputEdit { identity, text: String, anchor_utf16: u32, head_utf16: u32, composing: bool, native_edit_serial: u64 }`.

Secure fields cannot register. Registration synchronously sends focus state and current native snapshot. First native report has serial 1; later registration retains the mount's monotonically increasing serial and sends a snapshot even if content is unchanged. Native observer otherwise reports only changed text/selection/composition tuples. Blur/removal/Surface stop take hooks and notify focus false; the old callbacks must not authorize a newly focused mount. The adapter does not auto-replay commands.

## Actual input call chain

Current `input::key`: native marked-text exclusion → guarded input key policy → Escape disarm → hold → ordinary dispatch. Current `key_now` (including held-key replay): native marked-text exclusion → guarded input key policy → ordinary dispatch. Ordinary dispatch remains shortcuts → system → keymap → chrome → input editing → selection/navigation/scroll/Tab. Native marked text is checked in the native model, never guessed from an engine Composition signal. Candidate keys return unhandled before hold/direct/replay and must not become deferred application commands.

`report_native_edit` captures one native model tuple and invokes the edit observer before independent engine change/selection/composition events. Registration, after_edit, selection and composition use this path. Native pointer/candidate geometry, selection direction and scrolling remain backend responsibilities.

`paste` currently reads the clipboard once on the original gesture, emits an engine Paste event, then inserts only for PasteBehavior::Insert. Report mode has no synchronous original-payload hook. Engine `.value()` is guarded by backend edit count; selected_range is a separate asynchronous command. Those are not a conditional identity+serial atomic text/selection projection.

## Implemented application integration

`text_input/native_adapter.rs` consumes these exact types and methods. `TextInput::render_nsh_input` and crate-local `render_nsh_composer` use the shared transactional renderer; `render_nsh_preview` explicitly remains the compatibility preview path. No parent attaches another bridge: the production renderer owns attach/register/delivery.

1. NSH Input ElementRef publishes its actual node through a watch channel at layout and on real focus events. The host resolves it through Surface.entity().read(cx).input_identity(node), stores exact identity and registers hooks synchronously. Initial registration is allowed on the mounted node; re-registration requires current active host focus or native Input focus discovery. Window activation schedules a next-frame current-focus recheck, not a business action.
2. First snapshot initializes the ledger against authoritative text/directed selection/composition, setting its baseline to native_serial minus one before ordinary observation. Mismatch preserves actual native text and fails. Re-registration stays on the same mount with consecutive native serials, including unchanged-content snapshots. Recycled mounts fail rather than inheriting history or command authorization.
3. Native observations synchronously close the shared send gate and preserve local text before enqueueing immutable application edits. The application queue is bounded; the bridge retains 64 / 2 MiB text limits. FIFO delivery calls original TextInput transaction application, preserving mentions, composition-grouped history, directed UTF-16 selection and revisions. Failure or unknown result freezes with copy-local-draft / explicit reload UI; no replay.
4. Compatibility change/selection/composition callbacks do nothing in transaction mode. Authority snapshots are not written into the mounted native Input through engine value/selection during native editing. Earlier receipts cannot replace later local input.
5. Keys are translated from full native modifiers. Plain multiline Return/ShiftReturn stay native newline; ModReturn submits once only from the settled current draft. Popup acceptance/navigation precedes ordinary newline/focus behavior. Key tokens bind owner, focus epoch, text, selection, native serial and menu generation/highlight. Delivery claims once and rechecks current TextInput focus and readiness. Generic nonsecret inputs retain native Tab/Return rather than emitting unconsumed Composer commands; application undo/redo still use original TextInput history.
6. Authoritative undo/redo/prediction/history/send-clear uses `seal_authority`: require no pending native edits/composition, retire owner synchronously, stop old Surface, reconstruct current authoritative TextInput snapshot under a new owner. Conflicting authority changes fail and preserve native local text. Restore actual focus only if the editor/recovery page already owned focus. This is a safe replacement path, not an in-place native projection; candidate geometry and perceived continuity need native verification.
7. Before hiding/unmounting, parent calls `input.update(cx, |input, cx| input.suspend_nsh_composer(cx))`. It preserves authoritative draft, attachments and local recovery copy while retiring old mount callbacks. The enclosing ConversationStream records full-presentation suspension once per hidden lifecycle; repeated omitted/fullscreen renders must not repeatedly notify/revoke the editor. A genuine ConversationStream render resets this suspended marker before rebuilding visible presentation.

## Required backend additions / release blockers

### Original-payload paste transaction

Implementation contract extension (parent, current fixed source): `InputHooks` gains a synchronous paste-begin callback, a borrowed-payload decision callback, and an owned completion callback. Begin runs at the actual paste gesture before the app queue and binds the exact identity, native serial and hook registration; completion is either one actual native transaction plus the original payload, or an explicit cancellation. The application closes a dedicated pending-paste barrier at begin. No earlier edit receipt may reopen the send gate while that barrier is active. Payload validation happens before application retention; approved normalized insertion is applied only after rechecking identity, serial, composition, editability and the same hook registration after callbacks. Attachment-only completion advances one explicit native transaction serial without a compatibility duplicate. Unconfigured inputs retain their original path.

`InputKey` also gains an optional current-layout context containing the native serial, actual first visual caret-row and viewport-top eligibility. It is computed synchronously at key dispatch using the native layout and current selection, after invalidating layout on content edits. It is not inferred from newline count or a previous engine event. The application combines this context with its current route and exact local native serial; popup navigation still takes precedence. These are approved editing-package changes, not wheel-protocol changes.

Current production paste is deliberately NOT completed. Keyboard paste closes authorization synchronously and enters recovery; an engine Paste event also fails closed without cloning/replaying its payload. This is a blocker, not an accepted feature reduction. Do not enable this migration as complete until resolved.

Needed main-thread native interface: a paste hook invoked before insertion and before engine Paste delivery, receiving exact InputIdentity, current native serial, current full text/directed selection/composition, and a borrowed original PasteEvent from that single gesture. It must cover keyboard, native menu and accessibility paste equally. Suggested shape is `Fn(InputPaste) -> PasteDecision`, where a reject consumes without insertion/replay, and an accepted decision supplies normalized replacement text/directed selection or asks Report-only handling. Parent chooses exact names; application currently calls no nonexistent API.

The callback must synchronously close the gate, validate existing text/metadata/URI/image count/per-image/total/in-flight byte limits before retaining a payload, and correlate one observed edit or attachment-only transaction with the original gesture and serial. Retain immutable original payload in the same bounded ordered application queue, then call `apply_native_paste` instead of `apply_native_edit` for that transaction. Attachment-only paste needs an explicit transaction even if the native content tuple does not change. Preserve ImagePaste importer authority; no clipboard reread, MIME trust, duplicate engine fallback or unknown-result retry. Backend must not fabricate serial advancement independently of the application ledger.

### Conditional in-place projection (optional for current replacement path)

For continuity without full Surface replacement, parent can add an atomic UI-thread operation accepting exact InputIdentity, expected native serial, replacement text, directed UTF-16 selection and composing=false. It must reject wrong mount/focus/serial, pending native edits, composition and secure input; apply text+selection atomically, update model/layout/caret/candidate geometry and engine count coherence without an independent stale value echo. Return actual new serial/result. If callbacks are emitted they must occur after releasing application bridge locks; existing `project_native_draft` callback executes under its bridge lock and must not reenter it. No such method is currently called.

### First visual-row history and route-specific key policies

The original history rule uses actual visual caret row/scroll geometry. Current InputEdit lacks those fields. Current Composer route safely enables history for an empty draft or a known recalled history draft only; arbitrary nonempty first-visual-row Up remains incomplete. Needed native snapshot/query tied to identity+serial: whether unmodified Up is at first visual caret row and viewport top (and directed selection policy). Do not use stale legacy TextInput line caches or newline counting as proof for soft wraps.

Commit/sidebar rename/palette key-policy adoption stays parent-owned. Preserve Commit Return=newline/ModReturn=commit, rename Return accept/Escape cancel, palette Up/Down/Return/Escape/Tab/ShiftTab, and native composition exclusion before policy. The generic adapter is not a substitute for these controllers.

## Exact parent entry changes and validation

Replace only the production main Composer input child with `self.render_nsh_composer_input(window, cx)`, implemented in `conversation_stream/nsh_composer.rs`. Keep `prepare_nsh_composer` subscription, existing controller guards, original TextInput entity and attachments. Other nonsecret TextInput owners can call `TextInput::render_nsh_input(entity, window, cx)`; masked credentials must retain their existing path. Shared render/core caller edits were not made by Agent2.

Parent runs coordinated current-source Cargo check/Clippy/development tests. Native checks still required: composition start/commit/cancel and candidate keys, focus retirement/re-register, fast typing with delayed receipts, undo/redo with mentions, replacement focus/caret continuity, real 1–8-row height/scrolling, payload paste, AX, popup precedence and each route-specific policy. No Cargo/test/native app/network/Git command was executed by this leaf. Static parsing and authored tests are not proof these gates pass.

## Current superseding native composition routing contract

The author-development follow-up restores the existing palette contract (see
[native palette contract](nsh-command-palette.md#current-superseding-contract-composition-safe-global-shortcuts)),
not a new shortcut behavior. Native marked text must be checked before hold,
direct/replayed routing and GPUI host dispatch, without waiting for engine state.
Candidate/navigation/editing keys and normal IME text bypass application keymap,
editing hooks, navigation and text/focus mutation. They never enter or replay as
held keys. Existing retained text is not discarded. The host stops their event
propagation without preventing default OS IME delivery; unmapped text keys are
also native. Key releases still clear actual keymap repeat tracking.

A recognized non-editing key with Command on macOS / Control elsewhere and no
Alt/Option may use only the existing window-shortcut, real installed-keymap and
menu-shortcut APIs. Reuse `Keystroke::edits_text()` rather than guessing editing
keys. Space is explicitly excluded even with a primary modifier: NSH classifies
it as a character, but it is also an IME candidate-confirmation key. This class
includes `mod+k`, excludes candidate keys with any modifiers, editing accelerators
and Option/AltGr text, and does not promise every possible
OS/IME-specific shortcut. An unhandled eligible shortcut can bubble to the host;
an accepted one is consumed. Neither falls through into ordinary native editing,
navigation or the held-key queue. The routing operation does not clear marked
text. Existing callback semantics remain unchanged.

Authorized production files are `input.rs`, `events.rs`, and `text_input.rs` only
if strictly necessary; development regression changes stay in the existing
`marked_keys_never_reach_shortcuts_or_waiting_keys` test. No version/pin, Fast,
Core, Cargo or manifest edit is authorized here. Parent owns coordinated builds,
manifest updates and consolidated independent review. Source-only checks do not
prove synthetic GPUI-host event behavior or real OS candidate delivery; those
remain separate from direct native-model regressions and palette-controller
coverage. Current final hashes and parent commands are recorded in the palette
handoff evidence, with document hashes outside the documents themselves.

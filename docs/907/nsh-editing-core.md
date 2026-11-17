# #907 authoritative application editing core

## Frozen scope and acceptance

User supplied branch `feat/907-nsh-full`, HEAD `0b5ea426abfd8d4358489e916273d89474fb190f`, parent-confirmed master `34555cec9c0c4c4c1f7a528d739752f414e53edf` already included. No Git/network/build/app/delegation here. Current writer owns only `text_input/{mod,nsh,editing_core,editing_core_tests,native_adapter}.rs`, `conversation_stream/nsh_composer.rs` and this document. Parent owns the backend-contract document. Existing other dirty batches remain untouched; parent owns production entry/shared integration and all NSH patches.

Original behavior sources: `nsh-full.md` input conflict/recovery and clipboard sections, `nsh-keyboard.md`, `nsh-composer-actions.md`, `nsh-command-palette.md`, `nsh-commit-panel.md`, original TextInput history/input handler, composer guards. This round's explicit native Enter=newline / Mod-Enter=submit requirement supersedes the older composer Enter=send line only; popup acceptance retains priority. Masked credentials are excluded, not migrated.

## Mutation integration acceptance before implementation

This application batch consumes the parent-authorized `InputEdit.mutation: Option<InputMutation>` with public `range_utf16: (u32, u32)` and `inserted: String`. Ranges address the preceding emitted serial's FULL DISPLAY text, including marked text, not committed engine text. Native initial/reregister/selection-only snapshots use `None`; identical replacements use `Some` and advance serial. Normalized text paste reports its original replacement range; attachment-only completion uses `None`. Backend source remains exclusively parent-owned and was still tuple-only at the first read of this batch.

Application observations and authority commits must independently validate exact UTF-16 boundaries, sorted range, untouched prefix/suffix and inserted bytes against the previous/result text. `None` is accepted only when text is unchanged. Unknown or mismatched mutation freezes delivery and preserves local recovery, without guessed diff, broad binding clearing, replay or send readiness. Ordered receipt equality includes mutation provenance; snapshot identity checks for paste compare the native tuple without requiring prior mutation metadata in fresh snapshots. Explicit mutations always call original TextInput history/apply_edit, including byte-identical replacements; composition remains one undo group. F2 restore/reload fences remain independent and unchanged.

### Mutation integration delivery and verification

The stable vendor declaration appeared during this batch and was read directly: `InputMutation` has the requested public tuple/string fields and `InputEdit` now carries `mutation`. No vendor or backend-contract edits were made. Application `NativeMutation` owns an `Arc<str>` insertion; both backend-to-application edit paths preserve its original range and insertion. Native receipt equality includes it. Observation and commit both verify exact transformation against previous emitted/accepted full display text; UTF-16 split-surrogate, descending/outside range, missing mutation for changed text, altered prefix/suffix and substituted receipt are rejected. There is no native LCP/suffix fallback. Failed delivery preserves local text and does not replay.

Original TextInput `record_edit`/`apply_edit` consume every explicit native mutation, even identical replacement, preserving unaffected bindings and actual directed selection/undo/redo. Marked-display updates reuse original composition history grouping. Paste additionally verifies the actual original selected range and normalized insertion; attachment-only completion requires no mutation. Fresh paste snapshots compare native identity tuple without demanding the last transaction's provenance; ordered completion receipts still compare provenance. Queue byte accounting includes retained insertion text; paste reservation includes the additional normalized insertion before payload retention. Existing bounded in-flight/RAII behavior and assertions remain.

F2 restore/reload implementation and its owner/revision/selection/serial/pending/composition/unknown-outcome gates were not changed in this batch. Its existing actual TextInput continued-edit regression now supplies explicit mutation metadata, not a compatibility bypass. Independent F2 review and real recovery button/Surface remount/hook/send-controller verification remain required.

Seven new development regressions; all **defined, UNRUN**:

- `native_selected_mention_replacement_preserves_exact_range_and_undo_redo`: real TextInput, same `@foo` and prefix `@foobar` replacement, preceding Unicode/surrogate and unaffected mentions before/after, real history and directed selection.
- `native_marked_display_mutations_share_history_without_clearing_other_mentions`: actual TextInput marked-display replacement and second mutation relative to previous display, commit-only `None`, one undo group and unaffected mention remapping.
- `mutation_mismatch_or_missing_provenance_freezes_without_applying_or_replaying`.
- `mutation_range_rejects_split_surrogates_and_untouched_text_changes`.
- `ordered_receipt_cannot_substitute_an_equivalent_text_mutation`.
- `missing_mutation_preserves_local_recovery_without_delivering_an_edit`.
- `fresh_paste_snapshot_after_mutation_keeps_barrier_and_exact_provenance`.

The previous 40 tests remain, including all assertions; their synthetic text-changing events now explicitly describe their original replacements. Static definition count: nsh 8 + editing_core_tests 19 + native_adapter 20 = **47 definitions / 0 executed**. No source-string check is presented as a behavior test. Parent must compile/run this exact integrated version, independently review F1/F2, archive/remove development Rust tests before master, and complete native/CI gates.

Executed: `rustfmt --version` reported `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`; scoped `rustfmt --edition 2024 --config skip_children=true` on the four changed Rust files; final same `--check` on all six owned Rust files, exit 0. Read-only Python trailing-whitespace, production no-new-comments/unwrap/expect checks, UTF-8/UTF-16 fixture boundaries, native transaction no-diff check, definition counts and SHA256 checks passed. These are formatting/static evidence only. Cargo/rustc/type-check/Clippy/build/tests/CI/Git/network/native app/install/machine config/delegation were **NOT RUN**. Baseline remains user-supplied `0b5ea426abfd8d4358489e916273d89474fb190f`; no new commit. No shared lifecycle/caller changes are required by this delta. Parent must export the new host type if its host-test shim exposes editing types; no additional backend signature is requested. Backend notification/IME/paste runtime semantics remain under the backend writer's validation, not inferred from matching declarations.

### Mutation integration Rust SHA256

Paths relative to `crates/vega_ui/src/`; this table supersedes historical source hashes below. Document hash supplied separately at READY.

| File | SHA256 |
| --- | --- |
| `text_input/mod.rs` (unchanged) | `f2665acb63a412b07eeb27b01c774390d67d9da9e92d900562c0395b41aa5430` |
| `text_input/nsh.rs` | `802550e8f1495651477f07ce0341d724a796d00cb8b7504a35954bf40eb3e9d8` |
| `text_input/editing_core.rs` | `d68bab42fab2da1b7e3f1858b7218a7b19e3c9d9ad4a37ffbffebdb71c88c07c` |
| `text_input/editing_core_tests.rs` | `3ac79c53cd6319fb116284bf3c28956a1b7ec68cad1d3529279c489040b9236c` |
| `text_input/native_adapter.rs` | `a7964777568b6ffb34520587eaddd655017595c4f3c14ccc278663378497a924` |
| `conversation_stream/nsh_composer.rs` (unchanged) | `55d67420c99251e259fcd9879c1722ff2a9cc760111bb9eb579a68c7ecc9f6dd` |

## Historical reviewer follow-up: mutation provenance and explicit recovery

Historical pre-mutation stage (superseded by integration above). F1 backend delta required: that stage's `InputEdit` was only a resulting tuple and cannot distinguish an identical text replacement from selection/composition-only notification. Ordinary native transactions need `mutation: Option<InputMutation>` with `InputMutation { range_utf16: (u32, u32), inserted: String }`, a sorted pre-transaction UTF-16 replacement range and the actual inserted text. `None` means no text mutation, including initial registration and selection-only observations. Deliver `Some` even for byte-identical replacement; increment serial and notify for such mutations. Marked-text replacement must describe the actual native replaced range, not merely the previous selection. The range applies to the preceding serial's text; untouched prefix/suffix must match the resulting text. Composition commit without text mutation remains `None`. Paste already has its captured directed pre-selection plus original normalized payload and does not need this metadata to recover its exact mutation. Parent owns backend declarations/notification and must provide this delta; application code must not assume it already exists.

F2 acceptance: restoring saved text is an explicit owner/revision/selection/serial-fenced authority operation, never a legacy engine event. Reject pending edits/paste/commands, composition, stale owner and authority conflicts without deleting saved text. Accepted restore records original TextInput history, retires the old native owner and uses supported Surface stop/new Surface construction to install restored authority; no native replacement API is currently exposed by AppView. Failed native sessions reload only through an explicit remount request. Recovery text remains available through rejection and remount. Regression coverage must exercise real TextInput bindings and undo/redo plus failure/reload/restore/continued-edit/send readiness, not just a mirrored diff algorithm.

### Follow-up implementation and verification

- F1 paste: production `apply_native_paste` now uses captured pre-transaction directed UTF-16 selection and normalized original payload in `apply_pasted_replacement`. It records/applies even identical text replacement, invalidates only overlapping bindings through original `TextInput::apply_edit`, and preserves/remaps untouched bindings. Attachment-only and URI paste do not remove the selected text. `commit_native_transaction` is the extracted production authority boundary used by the development regression, not a mock replacement implementation.
- F1 ordinary native edits: **BLOCKED / not fixed** with current tuple-only backend API. Their existing diff path remains; no broad mention clearing, guessed native mutation or nonexistent vendor field was introduced. Parent must implement the exact metadata delta above, then application `NativeAdapter::native_edit/edit` and ordered delivery must transport and validate it against preceding accepted serial before `apply_edit`, including identical replacement. This follow-up cannot close F1 in full until that work is done.
- F2: new `EditorOperation::Restore` is separate from legacy Replace. UI captures `NativeRestoreToken` (owner, serial/full local tuple, authority revision tuple) from a settled current bridge; the consumer checks the displayed revision/selection, saved text and current healthy mount before atomically applying original TextInput history/replacement under the bridge fence. Pending edit/paste/command, composition, stale serial/owner and authority conflict reject without changing saved text. Success retires the bridge, stops the old Surface and uses existing remount construction from authoritative snapshot. No compatibility bypass or invented AppView replacement method.
- Explicit native reload now uses a separate `ReloadNative` request rather than returning early because transactional mode is attached. It retires/stops the failed session, clears obsolete composition and advances selection revision before reconstruction. Host failure recovery remains the existing explicit remount page. Recovery requests use fresh correlated tickets independent of failed compatibility ledgers, never reuse legacy Replace receipts, never automatically replay business commands, and expose rejected/lost response as command failure while retaining saved recovery text.
- Four new development regressions (40 definitions total, **0 executed**): real TextInput selected bound `@foo` paste with both `@foobar` and identical `@foo`, unaffected `@keep` movement/preservation and actual undo/redo directed selection; real TextInput failed draft/rebuilt bridge/restore/conflict preservation/continued native edit/send gate and Submit readiness/undo/redo; bridge pending-paste and pending-edit restore fence plus stale serial/revision and duplicate rejection; old-owner/composition/pending-command rejection. The two GPUI tests run production transaction/history methods in TestAppContext, not a cloned diff. They do **not** execute the real NSH Surface/engine remount, recovery button delivery, hook registration or actual send/controller callback; those integration gates remain parent-owned and unverified.
- Executed `rustfmt --version`: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`. Executed scoped formatting on the three changed Rust files and final `rustfmt --edition 2024 --check --config skip_children=true` on all six owned Rust files: exit 0. Read-only Python whitespace/comment/non-test unwrap checks and UTF-16 fixtures (9/10 units) passed; SHA256 recorded below. One atomic edit was rejected because a test-window line matched twice; confirmed no change, supplied unique function context and applied successfully. No assertion was weakened.
- Cargo/rustc/type checking/Clippy/build/test execution/CI/native App/Git/network/install/config/delegation: **NOT RUN**. Parent's earlier backend count and test results do not validate these application bytes. No new dependency, vendor/backend-contract/shared-render/caller write. This stage changes only `text_input/{nsh,editing_core,editing_core_tests}.rs` and this document; existing other batches and prior assertions retained. No complete acceptance claimed. Parent must coordinate compile/run of the four new regressions, original 36 and actual Surface recovery/hook/remount path, archive/remove development Rust tests before master and complete independent review/native acceptance.

### Follow-up Rust SHA256

Paths relative to `crates/vega_ui/src/`; document hash supplied separately at READY. This table supersedes prior historical delivery hashes below.

| File | SHA256 |
| --- | --- |
| `text_input/mod.rs` (unchanged) | `f2665acb63a412b07eeb27b01c774390d67d9da9e92d900562c0395b41aa5430` |
| `text_input/nsh.rs` | `b16d1949b03b10d6b3e29070309d0dc0d9f1b11f70823670d6e251b8758fff91` |
| `text_input/editing_core.rs` | `9b034258262fc94b8069c314ef73c70af7613a6807b14bef6e5cc934d30d317b` |
| `text_input/editing_core_tests.rs` | `99a83900217c285ee75a07cd47883677dd9bd9f972547fe8d07619ca9af284ff` |
| `text_input/native_adapter.rs` (unchanged) | `3b30493a39ee65589b1c246165cd4816f2c35775705b1a4f9fcc9216bfaac824` |
| `conversation_stream/nsh_composer.rs` (unchanged) | `55d67420c99251e259fcd9879c1722ff2a9cc760111bb9eb579a68c7ecc9f6dd` |

## Application protocol, before implementation

- Original TextInput entity remains authoritative for text, selection, mentions, history and attachment event emission. NSH owns an optimistic local nonsecret draft, not a replacement conversation/controller. Revisions, before-text and selection revision all fence writes. Each editor mount owns an identity and each queued request a local serial; receipts echo the serial. A retired session cannot authorize an edit/shortcut or reopen a barrier.
- Every native edit closes the shared barrier synchronously. The native bridge mirrors text, directed UTF-16 selection, composition and a monotonically checked local serial; it is usable from the parent's native observer without NSH engine access. Engine event compatibility must not replace a newer native observation. Only a matching final receipt may replace the local view. Receipt counters may advance for earlier successful edits without overwriting newer local text/selection. Selection and content are separate fences.
- Ordered commands remain bounded to 64 / 2 MiB text and existing attachment limits. Refusal, dropped receipt, serial exhaustion or stale revision freezes the session, preserves exact local text and stops automatic replay. Explicit reload creates a new session or resets only after all pending work drains. Unknown attachment operations are never retried.
- Clipboard is the original `PasteEvent` from that gesture. No reread, metadata trust, logging or text substitution. Validate count, individual/total bytes and URI bounds before cloning attachment buffers. Text insertion normalizes CRLF; URI paste does not insert paths. Attachments still go to original ImagePaste importer. UTF-16 endpoints must be valid boundaries, including reversed selection.
- Native keyboard decisions use an exact active focused mount, synchronously known native composition and current settled draft; no `When::Always`/asynchronous keybinding removal as IME protection. Plain Return is newline; Mod-Return is submit; popup acceptance/navigation retains priority. A key pressed before synchronization is consumed without saving a delayed business intent. Native composition keys pass through and are never queued/replayed. Backend implementation is parent-owned; see `nsh-editing-backend-contract.md`.
- Keep TextInput FocusHandle as outer identity. Suspension immediately retires the native bridge, closes the barrier and stops its Surface without clearing authoritative text/history/attachments. Remount gets a new owner. Do not focus recovery or accept shortcuts from an old mount. Editor height still follows native content size, clamped to original configured 1–8 rows, with native internal scrolling and candidate geometry.

## Verification plan

Author targeted development behavior tests for fast edit/late receipt, out-of-order receipt, lost/unknown outcome, native composition passthrough, pending submit suppression without replay, old focus/session rejection, reversed/surrogate selection and paste bounds. Keep existing recovery tests. Tests are development-only, parent must archive/remove before master, not now. Only scoped rustfmt and read-only static checks run here; parent coordinates Cargo and full integration/native checks. No empty or unrun set may be called passed.

## Current native-hook integration acceptance (before implementation)

The user now authorizes application production adaptation in `text_input/**` and `conversation_stream/nsh_composer.rs`. Parent retains the actual render caller, shared Surface and all backend files. Consume the stable `AppView::input_identity` / `set_input_hooks` API, not a second renderer. Discover the real Input node through its ElementRef; register the exact current identity on the UI thread. Registration's first snapshot is a transaction (first serial 1); re-registration keeps the native serial and must not replay commands. False focus retires command authorization immediately. Only actual native focus notification, engine focus discovery or actual active host focus re-entry may initiate registration again.

Native callbacks synchronously close the send barrier, retain latest local text and enqueue bounded immutable transactions. UI delivery applies the original TextInput history/mentions/selection logic. Transaction-mode compatibility change/selection/composition events are ignored. Nonsecret single-line and multiline TextInput use the same adapter; masked input remains excluded. Plain Return remains newline in multiline inputs; ModReturn produces a fenced Composer command, with popup precedence unchanged. Heights come from actual native content measurement within configured row bounds.

Inspected API gaps: there is no synchronous conditional native text/selection projection and no synchronous original-payload paste callback. Neither legacy `.value()` nor engine paste callbacks prove native serial/focus atomicity. Authority projection can use a synchronously sealed replacement: only when the bridge has no pending edits/composition, retire the old owner, stop its Surface on the UI thread and reconstruct from the current authoritative TextInput snapshot with a new owner. This is replacement, not an in-place backend projection. A pending/conflicting authority replacement fails and preserves local text. Transaction-mode paste must close authorization, preserve local text and expose explicit reload until the original gesture can be correlated synchronously; no blind replay or false completion. Required native API parameters and exact lifecycle are recorded in the backend contract. Implement all independently supported transaction/focus/keyboard/controller paths now; do not call nonexistent methods.

Development regressions must exercise the consumed application adapter, first snapshot/re-registration, authority mismatch, fast local observations, compatibility echo suppression, focus retirement and original draft history, not duplicate the backend algorithm. They remain unrun here under the parent's exclusive Cargo authorization.

## Paste completion and native history acceptance (current round, before implementation)

Current write ownership is limited to `text_input/{mod,nsh,editing_core,editing_core_tests,native_adapter}.rs`, `conversation_stream/nsh_composer.rs` and this document. Parent now owns the backend-contract document as well as vendor/shared/manifests. Parent reports remote unchanged at `34555cec9c0c4c4c1f7a528d739752f414e53edf`; no leaf Git/network/build/app execution.

Consume the actual proposed `InputHooks::{paste_begin,paste,paste_complete}` and `InputKey::context` from vendor `editing.rs`. Paste begin runs at the actual gesture and synchronously closes a bridge-level pending-paste barrier even before payload delivery. Earlier edit receipts cannot settle it. Reject, cancellation, blur during paste, identity/serial mismatch and unknown completion freeze authorization and preserve local text until explicit recovery; they never replay business intent. Validate original borrowed payload metadata, URI/count/image bounds and aggregate retained bytes before normalized allocation or backend retention. Reserve a bounded RAII budget through queued completion, including attachment-only paste; drop releases the reservation even when the receiver or stale session discards it. Completion is one immutable original payload plus one native transaction and uses `apply_native_paste`, never ordinary edit delivery or engine fallback. Nonattachment TextInput accepts ordinary text paste without emitting attachment imports.

Use same-dispatch `InputKeyContext` only with the current native identity, native serial, collapsed selection and bridge tuple. History availability is independent from eligibility: an existing recall continues its original behavior, otherwise first visual row and viewport top authorize Up when history exists. Popup navigation remains higher priority; no newline counting or stale TextInput layout cache. Missing/stale context cannot authorize a new history recall. Existing assertions remain and development tests must exercise the application adapter's pending-paste/receipt/key, budget/drop/cancel and history boundaries. Cargo and native execution remain parent-owned.

## Historical application delivery: paste and native history

Implemented against the actual proposed API in `vendor/nsh-platform-gpui/src/editing.rs`, without editing it:

- `paste_begin` synchronously closes the bridge gate and records the exact native tuple. `commit`, `ready`, command validation, projection and authority replacement cannot reopen/bypass pending paste. An earlier successful edit receipt cannot settle the barrier.
- `paste` validates the original borrowed payload, metadata/URI/image limits and shared main-thread in-flight budget before normalization or retention. Limits remain 64 transactions / 2 MiB text-and-metadata / 16 MiB encoded image bytes, with existing per-image/count limits. Text budget conservatively includes before/after text and normalized insertion. Encoded image signatures are checked without trusting MIME labels. No clipboard reread, payload clone, new dependency or renderer.
- `paste_complete` owns exactly one original payload and serial-checked NativeEdit. Its production delivery is consumed by `apply_native_paste`; attachments retain the original importer. Attachment-only completion advances one serial without a duplicate ordinary edit. The RAII budget lives through the queued delivery and releases on success, failure or discarded queue. Rejection/cancel/blur/serial mismatch preserve local draft and freeze until explicit recovery; late completion after failure does not replace saved text. Engine paste echoes are ignored in transaction mode rather than used as fallback.
- Ordinary nonsecret TextInput can paste text without Composer attachment authority. Keyboard paste forced failure is removed; actual backend paste owns every gesture path. `render_nsh_preview` remains distinct and unchanged in role.
- Composer publishes `has_history` independently of actual `recalling_history`; empty text is not itself a recalled draft. Same-dispatch native context plus current serial/collapsed tuple/first visual row/viewport top enables a new recall. Actual recalled drafts retain continued recall semantics, and popup navigation wins. Shift-Up remains selection; stale/missing layout cannot start history; pending edits consume business keys without replay.
- Added 12 application development regressions to the prior 24 definitions: **36 definitions, 0 executed**. New cases cover begin/late receipt/submit, cancel/late completion, invalid original payload, attachment-only metadata, shared budget/queue discard, image budget, visual history eligibility, recall/popup precedence, paste followed by typing, serial mismatch, ordinary text paste and pending-history no replay. Existing assertions and test sources are retained; parent archives/removes development tests before master.

### Actual verification and remaining gates

Executed scoped `rustfmt --edition 2024 --config skip_children=true` and final `--check` on the six owned Rust files: exit 0. Actual rustfmt `1.9.0-stable (88d9e12ae1 2026-08-18)`. Read-only Python final-source checks passed for whitespace, no new production comments/unwrap/expect in core/adapter/composer, forbidden asynchronous key policies, four compatibility echo guards, actual paste consumer, history route and SHA256/test definition counts. The first Python check failed because it assumed rustfmt kept a call on one line; inspected the multiline call, corrected only the observation regex, and reran successfully. No product assertion was changed for that failure.

Cargo/rustc/Clippy/build/test execution/CI/native app/Git/network/install/config/delegation: **NOT RUN**. These static checks are not type checking or runtime evidence. No new Git HEAD exists from this writer; supplied baseline remains the only Git version evidence.

At that earlier paste/history delivery no additional backend signature delta was needed; the reviewer follow-up above now requires mutation provenance. Parent must keep the inspected ordering/lifetime contract: begin before queued clipboard work, exact same registration/identity/serial before payload and after callbacks, completion once with one serial including attachments, cancellation notification and no ordinary-edit duplicate. Parent owns production main input child wiring (`self.render_nsh_composer_input(window, cx)`), shared hide/unmount calls (`suspend_nsh_composer(cx)`) and Commit/sidebar/palette route policies. Existing shared guards/controllers must remain. The application now consumes paste/history APIs but full backend, original TextInput/controller/attachment integration, focus remount, IME/candidate geometry, real 1–8 row height/scrolling and AX/native menu paste still need coordinated compilation, development test execution, independent review and native hand checks. No complete migration/acceptance is claimed.

### Historical paste/history Rust SHA256

Paths relative to `crates/vega_ui/src/`; unchanged files are included to fix the owned source set. This document's hash is supplied separately at READY.

| File | SHA256 |
| --- | --- |
| `text_input/mod.rs` (unchanged) | `f2665acb63a412b07eeb27b01c774390d67d9da9e92d900562c0395b41aa5430` |
| `text_input/nsh.rs` | `20999446a99e82fe545cd639fb5a776f0c363ee2eadfdd4ead7024b1ddb5a2ce` |
| `text_input/editing_core.rs` | `028d0ca2d935750c926160c823e39d8d24c71fb4fb456140a4616e6f9912a79b` |
| `text_input/editing_core_tests.rs` (unchanged) | `de564c198ed66225ac8b983c677ed2da9a0969c947de7b494239996d8a84d65b` |
| `text_input/native_adapter.rs` | `3b30493a39ee65589b1c246165cd4816f2c35775705b1a4f9fcc9216bfaac824` |
| `conversation_stream/nsh_composer.rs` | `55d67420c99251e259fcd9879c1722ff2a9cc760111bb9eb579a68c7ecc9f6dd` |

## Historical delivery: native-hook application adaptation

**READY for parent integration review; writes stopped after final hashes. Not full acceptance or a compile/test pass.**

### Implemented this round

- Added `text_input/native_adapter.rs`, consumed by the production TextInput renderer. It translates the actual stable NSH InputHooks into bounded ordered application edit/command deliveries, initializes against the registration snapshot, keeps native serial continuity on re-registration, and binds callbacks to real mount/focus identities. False focus immediately invalidates old tokens; stale registration callbacks cannot retire a newer registration.
- Production `TextInput::render_nsh_input` and crate-local `render_nsh_composer` use transaction mode. `render_nsh_preview` remains explicitly separate compatibility mode. Real Input ElementRef layout/focus discovery registers actual AppView hooks through existing Surface.entity(); no shared file changes or hypothetical backend calls.
- Native transactions apply original TextInput content, mentions, directed selection, composition grouping and history. Compatibility change/selection/composition echoes do nothing in transaction mode. Original draft and attachments remain authoritative. Native observation synchronously closes the send gate; earlier receipts cannot settle later local text.
- Undo/redo/prediction/history/send-clear authority changes seal the settled bridge, stop the old Surface and rebuild from current authoritative state under a new owner. Pending native text/composition refuses this replacement and preserves local recovery. Existing real focus is restored without stealing another control's focus. Actual measured content/root height now notifies the host, retaining configured 1–8 row bounds.
- `ConversationStream::render_nsh_composer_input(window, cx)` consumes the transactional renderer with current popup/menu route. Parent must replace the production input child with this method; that shared caller is outside this writer's scope. Generic nonsecret inputs retain native Tab/Return instead of emitting unconsumed Composer commands. Masked inputs are excluded.
- Added 6 executable development tests calling production NativeAdapter delivery/key/replacement boundaries. Existing 18 protocol/recovery tests retained unchanged. Current static definition count is **24; executed 0**. Assertions were not relaxed and tests were not removed.

### Remaining blockers, not accepted degradation

- No synchronous original-payload paste hook exists. Production keyboard paste currently fails closed with local recovery; engine paste also freezes and never rereads/replays a payload. This is an incomplete feature and a release blocker. Full required API parameters, attachment-only transaction and lifecycle are specified in `nsh-editing-backend-contract.md`.
- Native InputEdit has no first-visual-row/caret-scroll eligibility. Empty/recalled draft history works through current route; arbitrary nonempty first-visual-row history remains incomplete. Do not substitute stale legacy line caches or logical newline counting.
- Native focus re-registration, composition/candidate geometry, real height/scrolling, Surface replacement caret continuity, controller undo/redo and attachment behavior still require parent integration/native evidence. Current tests exercise the application adapter, not real backend/native dispatch or complete controller integration.
- Commit/sidebar/palette integration and all vendor/shared/manifest/lock edits remain parent-owned. Parent must preserve route-specific semantics and use `suspend_nsh_composer` before hiding/removal. Do not mark full migration accepted or enable it as complete with the paste blocker unresolved.

### Current actual verification

- Executed scoped rustfmt and final `--check` with `--edition 2024 --config skip_children=true` on `text_input/{mod,nsh,editing_core,editing_core_tests,native_adapter}.rs` and `conversation_stream/nsh_composer.rs`: exit 0. Actual rustfmt: `1.9.0-stable (88d9e12ae1 2026-08-18)`.
- Executed read-only Python final-source checks: trailing whitespace, no new comments/non-test unwrap/expect in core/adapter/composer file, no WhenAlways/remove_keybinding/on_key_with/deferred ShortcutIntent in editing sources, explicit preview/production separation, three compatibility-event guards, actual set_input_hooks and apply_native_edit call sites. Exit 0. These are static checks, not Rust type checking or behavior execution.
- Cargo/rustc/Clippy/build/development tests/CI/native app/Git/network/install/config/delegation: **NOT RUN** under this leaf's authorization. Parent owns coordinated current-source compilation and execution, independent review, archive/removal of development Rust tests before master, final-head CI and user hand tests.
- No commit/hash of a new Git head exists from this writer. Baseline HEAD/branch/master are user-supplied, not rechecked with Git. Only authorized files were written; other dirty batches were not touched.

### Current Rust source SHA256

Paths below are relative to `crates/vega_ui/src/`.

| File | SHA256 |
| --- | --- |
| `text_input/mod.rs` | `f2665acb63a412b07eeb27b01c774390d67d9da9e92d900562c0395b41aa5430` |
| `text_input/nsh.rs` | `1d5dc32f96700ae1d27b006c5c2b4d41ab6007cc19e8616f51b98960a8b1f4c5` |
| `text_input/editing_core.rs` | `efbbd59d01c1f5eaf876e269ad535a00dc0156fb78988c3f8958d4dd7d49e6d1` |
| `text_input/editing_core_tests.rs` (unchanged) | `de564c198ed66225ac8b983c677ed2da9a0969c947de7b494239996d8a84d65b` |
| `text_input/native_adapter.rs` | `1b14c201b19ce81aabd3c8dba65790b1f412080fac4cb8e3e34ff043739deba3` |
| `conversation_stream/nsh_composer.rs` | `630c4404ee82906696ca29dee20a62219cfb61a24649ef591ed33a725388a7dd` |

Document hashes are supplied separately in the final response, avoiding self-referential hashes.

## Historical delivery status — prior application-core stage

The material below records the preceding stage only. Its hashes, verification and parent-integration gaps are historical, not current-source evidence.

**Historical READY — application-core stage only.** Native source patch, production entry, shared wiring, required CI, independent review and real IME/keyboard/AX acceptance remain separate pending gates. This is not a completed production migration or a compile/test pass.

### Actual implementation

- New `editing_core.rs`: owner-identity request ledger, strictly ordered echoed receipts, checked local/native serials, directed UTF-16 validation, mount/focus epoch fences, bounded native transaction queue, latest-local recovery, composition-first key decisions, consume-without-deferred-intent, opaque single-claim native command tokens and conditional authoritative-to-native projection. Original TextInput remains authority.
- `nsh.rs`: request receipts carry ledger tickets; only the last current accepted request synchronizes local view. Native edit/paste/command/project/suspend APIs validate exact previous text/selection/composition and content+selection revisions. Clipboard retains original PasteEvent and metadata with count/bytes/URI/text budgets; images are sniffed by bytes before original attachment import. Preview ambient `When::Always` and deferred shortcut intents removed; native key policy replaces that responsibility only after parent wiring. Preview function name/role retained, not relabeled as production.
- `mod.rs`: editing core export and development regression wiring only. `conversation_stream/nsh_composer.rs` was read, not changed. No composer/core/render/palette/commit/shared/vendor/manifest/lock/masked-input changes. Existing status/timeline/resume/i18n batches were not written.
- 16 new development tests plus the 2 existing recovery tests are retained (18 present, all UNRUN). They exercise receipt order/current-session, fast text edits, unknown outcome/no replay, native composition, pending submit, single claim, popup priority/focus retirement, menu generation, surrogate selection, composition end, overflow, conditional projection, stale native notification, paste text/URI/metadata/MIME. They do not prove native delivery or full controller integration.

### Exact source content SHA256

| File under `crates/vega_ui/src/text_input/` | SHA256 |
| --- | --- |
| `mod.rs` | `70b476e2d1eae841a1cb4d9a4c304cd62b8e7cb46588059b8dbe15e32c6b27c0` |
| `nsh.rs` | `9caddf02a249de26f1390a840adb89e51f2fa2e0b5704ab068a02cea1d81bc98` |
| `editing_core.rs` | `00dcd1d3586c9fd052f38834f9db9149ea6d05858bc7dcc85139848b8b8aa3a0` |
| `editing_core_tests.rs` | `de564c198ed66225ac8b983c677ed2da9a0969c947de7b494239996d8a84d65b` |

HEAD/branch/master above are supplied baseline evidence, not rechecked with Git. No commit was made.

### Actual verification

- Executed `rustfmt --version`: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`.
- Executed `rustfmt --edition 2024 --config skip_children=true crates/vega_ui/src/text_input/{mod,nsh,editing_core,editing_core_tests}.rs`, then same with `--check`: exit 0 on final source.
- Read-only Python checks: final hashes above, no trailing whitespace, no non-test unwrap/expect, no ambient WhenAlways/on_key_with/remove_keybinding/deferred ShortcutIntent in editing protocol; new core/test source has no comments. Exit 0. UTF-16 fixture length checked as 11. These are scoped source checks, not type checking or execution of assertions.
- Cargo/rustc/Clippy/build/tests/CI/native app/installation/network/Git/delegation: NOT RUN under leaf authorization. No old parent test count is used for these bytes. Parent must run the coordinated current-source compile and 18 development tests, fix actual failures at their responsible layer without weakening assertions, archive their source/results, then remove development tests and exclusive wiring before master.

### Parent integration / remaining coverage

Exact public method signatures and ordered call lifecycle are in `nsh-editing-backend-contract.md`. Parent supplies native marked-text guard before hold/direct/replay, full modifier policy and atomic native edit observer; attaches the bridge before first input, forwards actual focus epochs, observes before business keys, delivers exact ordered edit/paste transactions, and suppresses legacy echo callbacks. Native paste retained-byte bounds and unknown-result failure UI must be wired at that adapter. Current preview rejects its compatibility requests after native attach, intentionally failing closed rather than mixing protocols.

Use conditional `project_native_draft` to apply authoritative undo/redo/prediction/history/send-clear to the actual native model without edit echo; its callback runs under the bridge lock and must not reenter. Call `suspend_nsh_composer` before route hide/unmount so old callbacks cannot act. Shared send/resume/compact guards and original focus handle remain intact; parent owns their production integration and Commit/sidebar/palette adoption.

Uncovered: native event/receipt integration and recovery UI path, first-row history/caret route observation, native attachment queue lifecycle, candidate window geometry, original undo/redo controller integration, actual selection/composition ordering, real 1–8-row scrolling, Tab/focus/AX, macOS/Windows/Linux hand checks, independent review and final required CI. All are pending, not accepted feature degradation. The new pure protocol is valuable independently but must not be presented as full authoritative native editor delivery until these gates are wired and verified.

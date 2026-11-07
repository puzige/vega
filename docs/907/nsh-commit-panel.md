# #907 Commit panel NSH migration

## Scope and acceptance recorded before implementation

- Workspace `/Users/puzige/Workspace/worktrees/vega-907-nsh-full`, branch `feat/907-nsh-full`, starting HEAD `bd89db7d182e6d9a5c60b1388e79a4623e8cacc4`; parent already fetched/updated and integrated `origin/master` `34555cec9c0c4c4c1f7a528d739752f414e53edf` this cycle.
- Only write `crates/vega_ui/src/commit_panel/{mod.rs,panel.rs}`, new files in that directory, and this document. No delegation, dependency/platform/shared API/Git/index/network/config changes. Other writers' changes are not this leaf's evidence.
- `model.rs` is immutable SHA256 `251136fc02c45fa37686df0cac02f0af302f7634bce0f963ba3c0778041051d6`; starting `panel.rs` SHA256 `82f0f58fc3c42b2601ce4e58d47664b21dc2e0a978ea6b12b6c154a00ada630e`, `mod.rs` SHA256 `4964ea428133a6cd2904803e800d8eabb7981575105ce39ce0f7627ca457eda8`.

| Surface/behavior | Acceptance |
| --- | --- |
| Visible production panel | NSH title, cancel, checklist/status/selection, virtualized bounded rows, prepare, prepared confirmation, multiline message, generate, commit, loading/busy/disabled/errors; no GPUI product painting |
| Business authority | Original model/controller events and public actions/APIs, pending operation identity, IndexSnapshotId, PreparedCommitId, WorkspaceFileId and current thread/project route remain authoritative |
| Commands | Mounted visible page/session, displayed projection, freshly reconstructed controller authority, row recycle lease, disabled/stage/pending gates apply also to accessibility activation; unknown outcomes never replay |
| Draft | One nonsecret shared full-text draft with checked revision; submit captures text+revision atomically; generated results cannot overwrite newer text or composition; unrendered TextInput is a compatibility carrier only |
| Keyboard | Real native host focus; Tab/Shift-Tab and cancel/selection/generate/confirmation navigation; Enter remains newline in message; Cmd-Enter commit must not race IME |
| Lifecycle | Hide/unmount invalidates UI commands but does not cancel model pending work; recovery explicitly rebuilds UI only, never retries business operations |
| Desktop verification | macOS/Windows/Linux native IME, keyboard, AX, resize/virtualization and error/recovery are separate native proof, not inferred from unit/static checks |

## Independent review correction acceptance — recorded before correction

- Retained pointer, host-key and AX commands bind the immutable displayed draft revision, never the current shared revision sampled at activation. The displayed, command and fresh revisions must match under the draft lock before any prepare/draft/commit transition. An external carrier update without a new projection acknowledgement invalidates old commands; a new acknowledged projection may submit exactly its full text once. Native local edits update the shared draft and local input immediately, then publish/acknowledge a new immutable projection; local rendering is not permission to adopt an unknown carrier revision.
- An old generated-draft completion must return false before touching carrier, revision token or controller unless its operation is pending and the controller is Drafting. A newer B operation retains its token after stale A, accepts its own unchanged result, and still rejects results after a newer edit.
- Add only `pub fn suspend_presentation(&mut self, cx: &mut Context<Self>)`: invalidate page/displayed/mount/editability immediately without closing the model, clearing draft/controller/pending/operation, emitting close/cancel or retrying/replaying business work. Parent invokes this before route hide, preview/project unbind and `ConversationStream::suspend_ui_presentations`; the next visible render remounts from current controller state.
- Author production-boundary development regressions for stale/new acknowledged commands, stale completion ownership and presentation suspension. All are UNRUN here; scoped formatting/static inspection only, no Git commands (including diff), compilation, tests, app, network, dependencies or shared edits. Independent review and runtime gates remain outstanding; earlier READY/hash records are historical, not acceptance of this correction.

## Pinned API investigation and affected-feature blocker

Inspected public `Surface` renderer `fn(Signal<S>, mpsc::Sender<C>)`, bounded `OrderedCommands`, pointer-based `ProjectionId::matches`, and `signal_with(..., SignalEquals::Never)` in current shared `nsh.rs`. Ordinary NSH `View.focusable` is not treated as a native GPUI tab stop.

Installed source is NSH `0.4.0`, Kit `0.7.3`, Fast `0.1.9`; no upgrade/fork/platform patch is authorized. In `nsh-platform-gpui-0.4.0/src/text_input.rs`, `key` checks native composition before handling Return. However `return_key` returns newline for every multiline `SubmitBehavior::Newline` Return, regardless of Command/Ctrl. `Submit` and `BlurAndSubmit` submit on plain Return too. Public `Input::on_submit` receives only text; public `on_key_press` receives only a key string, not modifiers. Thus public native input defaults/on_submit cannot represent Enter=newline plus Cmd-Enter=commit. The platform keymap runs before native input handling (`input.rs::key_now`), so `When::Always` plus an engine-thread composition signal is not an acceptable safety proof.

**Blocked affected feature: message Cmd-Enter submit.** Do not intercept it with an engine keymap or pre-input GPUI business action. Keep native newline/composition behavior and an explicit commit button; this is an incomplete acceptance item, not an approved degradation or a full migration claim. Parent must obtain a compatible pinned-public-API solution or a new explicit dependency/platform decision. Continue independent UI/controller migration without claiming this blocker resolved.

## Validation boundary

Only scoped rustfmt and static inspection run in this leaf. No builds, tests, app execution, install, package, Git mutation, network or dependency changes. Parent owns one coordinated compile/test pass and integration. Authored development tests are UNRUN until parent executes them and must be removed before master per repository policy. The parent's baseline 124 green tests do not validate this delta.

## Implemented leaf and preservation evidence

- Production `CommitPanel::render` now returns the paint-free host with NSH Surface content. NSH draws title/cancel, loading/busy/errors, bounded virtual checklist rows and selected/forced states, prepared file count, full multiline editor, generate and prepare/commit controls. The old TextInput entity is never rendered.
- All 16 original public panel methods remain in their original order/signatures; all public actions, model exports, EventEmitter types and event payload routes remain. Prepare/draft/commit still enter the original model transitions and emit the original events. `model.rs` remains byte-for-byte identical. No store/Git/provider I/O moved to the engine.
- Host requests check live visible mount, page identity, displayed/shown/fresh controller authority, route, stage, pending operation, snapshot/prepared identity, selection and disabled state. Virtual callbacks and focused AX geometry use weak row/control leases; retained recycled callbacks cannot acquire them. Native focused-control AX activation rechecks authority and actual host focus. NSH View.focusable is not used as a native tab-stop substitute.
- The nonsecret shared draft is authoritative; an unrendered TextInput remains a compatibility carrier. Revision increments are checked, full text including trailing newlines is retained, submit/generate validate revision under the same lock as the original controller transition, and commit freezes further late edits. Drafting remains editable so generation completion rejects a newer revision/composition instead of overwriting it.
- UI unmount/recovery does not clear/cancel pending business operations. Reload remounts only the Surface and returns focus to cancel. No business retry, replay, execution or dependency/platform modification is introduced.
- Native message Enter and IME are left to the published input. Escape uses post-native input key reporting. Host focus implements model Tab/Shift-Tab navigation and noninput activation; native-input key context is not the old CommitPanel context. Cmd-Enter **while editing remains blocked**, as above. Native Tab/IME ordering and focus transitions are not proven by this leaf's static work.

## Initial READY static checks (historical, before review corrections)

Formatter: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`. Cargo/rustc compilation versions were not invoked or inferred.

```sh
rustfmt --edition 2024 --config skip_children=true crates/vega_ui/src/commit_panel/{mod,panel,draft,host,nsh_view,tests}.rs
rustfmt --edition 2024 --check --config skip_children=true crates/vega_ui/src/commit_panel/{mod,panel,draft,host,nsh_view,tests}.rs
git diff --check -- crates/vega_ui/src/commit_panel docs/907/nsh-commit-panel.md
shasum -a 256 crates/vega_ui/src/commit_panel/{model,mod,panel,draft,host,nsh_view,tests}.rs
```

All completed successfully. A read-only Python static comparison confirmed the original 16 public method names/order and found no added production comments, unwrap/expect, engine keymap, try_send, filesystem/process I/O or abort patterns in owned production files. This is not type checking, Clippy, test execution or native evidence.

## Authored development regressions — UNRUN

Eight tests, all explicitly not executed (the original four retained plus four review regressions):

1. `production_multiline_preserves_full_text_and_rejects_edits_after_freeze`: mounts the production renderer through public `Root`/`size`, checks multiline newline config, full Chinese/multiline/trailing-newline content and late-event freeze.
2. `edits_keep_full_multiline_text_and_invalidate_generated_revision`.
3. `revision_exhaustion_never_revalidates_an_old_draft`.
4. `hidden_or_retired_mount_rejects_retained_editor_callbacks`.
5. `retained_commands_require_their_immutable_displayed_revision`: calls the production locked gate shared by pointer, host key and AX; rejects a retained revision even if the live shared draft changes with no fresh projection, rejects a fresh command without new acknowledgement, accepts exact acknowledged full text, and rejects stale-after-transition/composition. This is boundary coverage, not native key/AX execution.
6. `stale_generated_completion_preserves_newer_operation_and_edit_guard`: calls the production completion transaction; stale A cannot mutate pending B/token/text, unchanged B succeeds with full generated text, later edits reject overwrite, and non-Drafting delivery preserves ownership. Public opaque business constructors are not fabricated or bypassed; the transaction accepts the already extracted draft text used in production.
7. `suspension_retires_live_draft_authority_without_losing_text`: calls production unmount, retains a live external mount, checks mount/editability retirement, exact draft/revision retention and command rejection.
8. `production_pointer_callback_cannot_borrow_an_undisplayed_revision`: mounts the real production button with public `Root`, `size`, `Component::element`, `Core::component_event` and task polling; a retained press emits its immutable old revision and cannot enter the controller gate with a newer shared revision.

The tests use explicit imports rather than GPUI wildcard test-macro imports. They are development evidence hooks, not native IME/shortcut/AX proof. Parent must archive/remove Rust tests and their exclusive wiring before master according to repository policy.

## Frozen source hashes after review corrections

| File within `crates/vega_ui/src/commit_panel/` | SHA256 |
| --- | --- |
| `model.rs` (unchanged) | `251136fc02c45fa37686df0cac02f0af302f7634bce0f963ba3c0778041051d6` |
| `mod.rs` | `25e8c65d07248a9ed489e92b1c62938a836ec11592a2824dfc8c20a2b4ac95db` |
| `panel.rs` | `7fef008b57d50c3d33abc17ff14e0bc7e21afca769d4fe06c4cf9ef2a349c6d5` |
| `draft.rs` | `6c6e6378878f96348c2c46db94ab27925553a8b940e4cd233fc0c61b3b4e0a25` |
| `host.rs` | `88ac75ba0789ac4c050955753d31ec0103f949467a67a3ba89cce16134740c08` |
| `nsh_view.rs` | `ca472807b49d899205c0c6b17ffa8293840267a72784bab833248b56bf8b88cb` |
| `tests.rs` | `83e019d56ff5505cfe5320e46c5e18ea57d2e30d16d4be36e834c349ae9709c7` |

## Review correction implementation and current static evidence

- `Snapshot` now freezes draft revision, exact full text and overflow. `same_authority` includes revision/overflow; button requests no longer sample a live revision. Fresh host snapshots atomically capture the shared value. `with_current_draft` compares command/displayed/fresh authority and current revision/full text, mount and composition under the same lock that surrounds prepare/draft/commit. Host key and AX still pass displayed/shown/fresh route and live-mount checks and enter that same gate. Carrier reconciliation precedes received commands and native host activation, closing the delayed-observer window.
- Input hydration uses immutable projected text only when its revision still matches the shared value. Local native edits can advance their own tracked revision before host acknowledgement without taking an unrelated external update as theirs. All commands still require a new immutable acknowledged revision; composition state is recorded even when a local revision is stale, not used as a pre-native shortcut interception proof.
- `finish_draft` checks pending operation and Drafting before carrier/revision mutation. Its production `finish_generated` transaction repeats ownership validation and only clears the token after accepted completion, retaining composition/overflow/newer-edit rejection. Original model transition code is unchanged.
- New public method only: `pub fn suspend_presentation(&mut self, cx: &mut Context<Self>)`. It calls presentation unmount and notify, retires page/displayed/geometry/visibility/mount/editability, preserves full draft/revision/model/pending/token, emits no business/close events and performs no retries. Next render creates a new page from the current model; no visible setter or business cancellation is needed.
- Current executed commands (from the worktree): scoped `rustfmt --edition 2024 --config skip_children=true crates/vega_ui/src/commit_panel/{mod,panel,draft,host,nsh_view,tests}.rs`, then the same with `--check`; `shasum -a 256` on the seven panel files; read-only Python SHA256/model+mod preservation, 16 original public method names/order plus the one hook, production forbidden-pattern and scoped trailing-whitespace checks. Final checks passed. The first Python API-list check failed because its expected list incorrectly called existing `apply_error` by `fail_if_stage`; inspection confirmed actual unchanged API and the corrected checker passed. No production assertion was weakened.
- No Git command, Cargo/rustc, build/test, app, network, dependency, install/config, delegation or shared-file edit was performed during this correction. Initial READY checks above are historical. Eight authored tests are UNRUN; native pointer/key/AX delivery, full controller event integration, pending preservation through the public hook/remount and platform behavior still require parent runtime validation. The parent 124-test baseline is not evidence for these bytes.

## Parent integration hooks and remaining gates

- Before removing/hiding the panel route, previewing, unbinding its project or suspending conversation UI presentations, parent calls `panel.update(cx, |panel, cx| panel.suspend_presentation(cx));` from `ConversationStream::suspend_ui_presentations` and the corresponding hide/unbind paths. Call it while the retained entity still exists; do not call `request_close`, clear pending or cancel business work for presentation hiding. This leaf does not edit those parent-owned callers.

- Run the single coordinated build/Clippy/test cycle on the integrated current source; fix responsible type/harness/product defects without weakening assertions. Suggested task filter after compilation: `cargo nextest run -p vega_ui --locked --offline -j 2 commit_panel`, in addition to parent-required regression coverage. This leaf did not execute it.
- Independent nonimplementer review must inspect this exact source, especially command/mount/recycle guards, atomic shared draft, native focus and AX geometry lifecycle. The prior READY received the user's independent review findings; these corrections still need a fresh nonimplementer review of the exact hashes. No review was delegated here.
- Native macOS/Windows/Linux checks remain entirely unrun: Chinese/Japanese IME start/commit/cancel, Enter newline, blocked Cmd-Enter behavior, Tab/Shift-Tab into/out of editor, focus after preparation/error, optional-row scroll/recycle/AX activation, theme/locale, narrow resize, hide/unmount/reopen, busy cancel with eventual operation delivery, and UI recovery without business replay. Current static checks are not evidence these native states pass.
- Resolve the pinned-public-API Cmd-Enter dependency or obtain a new explicit user decision before claiming full acceptance. Do not merge this development increment as a completed full migration. Required final-head CI, independent review, test cleanup and user hand testing remain parent-owned.

**READY after scoped review corrections; writes stopped. Review/runtime acceptance remains pending and full migration still has the pinned Cmd-Enter blocker.**

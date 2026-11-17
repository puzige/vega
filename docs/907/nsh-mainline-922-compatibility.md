# #907 / #922 mainline compatibility — author integration preparation

## Status, authority and exact versions

**DIAGNOSTIC STAGE READY — original mounted command-control runtime failure inspected; named original-budget waits and bounded current-state failure diagnostics authored, not yet rebuilt or rerun. The failing phase/predicate remains unestablished; no runtime fix or passing result is claimed.** The preceding implementation handoff was READY for parent central build, not independently reviewed, compiled, tested or merged. The followup explicitly authorized the six mixed paths, existing NSH tool child and necessary exclusive development support. This lane made only those source changes and this document update. No Git/index/ref command or mutation, fetch/network, Cargo/rustc, dependency/vendor, user configuration or user-data access occurred during implementation. Parent owns exact backend/SQL/document imports, shared provenance, ordinary-validation/read-only seam work, central validation and final-baseline preparation. The investigation below is retained as historical evidence; its earlier read-only/absent-backend statements describe the earlier snapshot, not the implementation handoff.

Worktree: `/Users/puzige/Workspace/worktrees/vega-907-nsh-full`. Inspection completed against local objects on 2026-11-09; snapshot time `2026-11-09T10:43:19Z`. HEAD alone does not identify this extensively dirty migration.

| Identity | Exact value |
| --- | --- |
| Observed worktree HEAD | `0b5ea426abfd8d4358489e916273d89474fb190f` |
| OLD: previous master / #920, already dirty-byte integrated by parent, including #916 | `0b3dcd3603a8e97e0ef74ae25a26f17fc3a094e5` |
| NEW: locally observed `origin/master` / #922 | `2a29adf9b2ea78eaf59d591838531b45adc7373e` |
| NEW first parent | `0b3dcd3603a8e97e0ef74ae25a26f17fc3a094e5` |
| OLD tree | `06edf197b5963370499cc876cbe1f19c5c300717` |
| NEW tree | `a54ccd7591fd6886817dad7343634044119a28ab` |

The parent supplied the latest-fetch fact; this lane did not refresh it or independently check the remote. NEW is `feat(shell): Full Access nonexclusive background sessions (#922)`, committed 2026-11-09 17:57:38 +0800. Exactly one commit separates OLD and NEW. Both `git diff --name-only OLD NEW` and `git diff-tree --no-commit-id --name-only -r NEW` enumerate **18 paths**, not the supplied estimate of 19: 15 modified, three added; 495 insertions / 82 deletions. Do not invent a nineteenth path or import unrelated changes. The feature's issue documents live under `docs/921/`; #922 is the integrating PR.

Read applicable root `AGENTS.md` and `docs/vega-exec-guide.md`; no nested `AGENTS.md` was found. The current task's explicit no-fetch/no-Git-mutation boundary applies; baseline updates remain the parent's responsibility. Relevant migration contracts: [full NSH migration](nsh-full.md), [recovery controller/surface](nsh-shell-recovery.md), [prior #916/#920 compatibility](nsh-mainline-compatibility.md), and [tool-group/native host behavior](nsh-tool-activity-group.md). NEW's `docs/921/full-access-background.md` is the new business contract, not permission for this investigation to execute background commands.

## Conclusion before the final baseline

The dirty tree is **not yet #922-compatible**: every affected existing backend file still equals OLD, and all three new paths are absent. The six affected locale/UI files differ from both OLD and NEW because they retain #907 migration changes. Import the backend as a coherent exact-upstream unit; adapt, rather than overwrite, the six UI/locale paths.

**#922 introduces production-visible paths that need explicit NSH adaptation.** Its command-card cancel/retry control and failure notice were added to the old GPUI renderer that #907 removed. Current rendering lives in `crates/vega_ui/src/tool_card/nsh_tool.rs`, which NEW does not change. Merely adding fields, events and the recovery service would compile only part of the contract and leave new controls/feedback unwired. Conversely, restoring upstream's old renderer or recovery button helper would violate the full NSH migration.

The majority is safely exact-copyable **after rechecking bytes**: nine existing backend files plus the three added SQL/document files, 12/18 paths. No whole-file copy of the six mixed UI/locale files is safe.

## Byte identity inventory

`D` means current raw worktree bytes, not HEAD/index; comparisons used `Path.read_bytes()` against `git show OLD:path` and `git show NEW:path`. `absent` is not an empty file. All 18 OLD and NEW entries differ (including absence versus added content). **No affected current file equals NEW.** A path being dirty relative to HEAD does not prevent it equalling OLD: notably conversation recovery and store `lib.rs` already contain earlier integration.

| Path | D versus OLD | D versus NEW | Parent treatment |
| --- | --- | --- | --- |
| `crates/vega_conversation/src/agent/shell_operations.rs` | identical | different | exact NEW copy |
| `crates/vega_conversation/src/shell_recovery.rs` | identical | different | exact NEW copy |
| `crates/vega_i18n/locales/ui.en.toml` | different | different | add only two keys |
| `crates/vega_i18n/locales/ui.zh-CN.toml` | different | different | add only two keys |
| `crates/vega_runtime/src/agent/mod.rs` | identical | different | exact NEW copy |
| `crates/vega_runtime/src/agent/shell_sessions.rs` | identical | different | exact NEW copy |
| `crates/vega_runtime/src/agent/tools_exec.rs` | identical | different | exact NEW copy |
| `crates/vega_runtime/src/lib.rs` | identical | different | exact NEW copy |
| `crates/vega_store/migrations/0063_shell_concurrency.sql` | both absent | absent in D | exact NEW addition |
| `crates/vega_store/src/lib.rs` | identical | different | exact NEW copy, retains #920 |
| `crates/vega_store/src/shell_operations.rs` | identical | different | exact NEW copy |
| `crates/vega_tools/src/bash/session.rs` | identical | different | exact NEW copy |
| `crates/vega_ui/src/conversation_stream/content.rs` | different | different | add event subscription only |
| `crates/vega_ui/src/conversation_stream/mod.rs` | different | different | retain NSH/dev exports; see visibility below |
| `crates/vega_ui/src/conversation_stream/shell_recovery.rs` | different | different | transplant controller hunks, not renderer |
| `crates/vega_ui/src/tool_card.rs` | different | different | transplant model hunks; move presentation to NSH child |
| `docs/921/full-access-background.md` | both absent | absent in D | exact NEW addition |
| `docs/921/implementation-evidence.md` | both absent | absent in D | exact NEW addition, historical claims only |

Expected Git blobs for the 12 exact-copy/add paths (verify raw content against the pinned NEW object, not moving `origin/master`):

```text
db1722c234c50e2504072ed961c74bc34047cf90  crates/vega_conversation/src/agent/shell_operations.rs
13079f60f74d810784f86fe6fa3bb1273d0a66c7  crates/vega_conversation/src/shell_recovery.rs
19338661525b3233850878c4dbbb5bc5b8fb38ed  crates/vega_runtime/src/agent/mod.rs
6a99a2f6433c03f5660b2f4e01349966c9b66308  crates/vega_runtime/src/agent/shell_sessions.rs
a27d2010d08f616a813deaa5d4c1913f8deb0120  crates/vega_runtime/src/agent/tools_exec.rs
e384b3456ca5f70ac4388a6712139d3633cb6f33  crates/vega_runtime/src/lib.rs
dbf87a3554e5eecf1bfec4ff7b841567bc7d0ea9  crates/vega_store/migrations/0063_shell_concurrency.sql
cccd312127f68763d3f7e2d5f371194eb2770fb9  crates/vega_store/src/lib.rs
800434b7686d1acd8cce6116099a6ccfc02005fd  crates/vega_store/src/shell_operations.rs
03cdaea1de0a703ef16a6f261df4c7cb3a92b0a7  crates/vega_tools/src/bash/session.rs
380d6b51de0de92f72015e0e8a3cc07df3abe1c6  docs/921/full-access-background.md
0e462d268274f538e18546ba16549ad11c0d7ba5  docs/921/implementation-evidence.md
```

## What the backend import must preserve

This is a compatibility map of changed contracts, not a new product investigation or a request to redesign them.

- **Classification and admission:** `RuntimeShellOperationRecord`, active-shell and store records gain immutable `concurrent`; only already-authorized `BashLaunchPolicy::FullAccess` sets it. Full Access background sessions become nonexclusive; workspace-sandbox coordinated sessions retain their blocker/admission rules. ReadOnly and extra writable roots remain incompatible with background sessions; macOS/session identity and existing authorization/danger checks remain. Do not change permission mode to obtain compatibility.
- **Resource capacity is not write exclusion:** Full Access retains application active/retained and per-run capacity accounting, resource ownership and bounded cleanup, but does not reserve workspace write ranges or install persistent/in-memory write blockers. Stop closes the run and initiates all cancellations together; do not replace it with serial cleanup, global locks, silent fallback or cross-run lifetime changes.
- **Read-before-write:** global read-observation invalidation occurs at Full Access start/completion (including blocking execution). Preserve the explicit failure results and never rerun a command because completion invalidation failed.
- **Store/SQL as one unit:** `0063_shell_concurrency.sql` appends `concurrent INTEGER NOT NULL DEFAULT 0 CHECK (concurrent IN (0, 1))`; register it after 0062 in `MIGRATIONS`. Existing records default coordinated. Persist classification immutably, include it in exact record identity, and preserve original terminal commits/readback. Concurrent cleanup observations and restart outcomes do not create sandbox blockers or advance coordinated recovery epochs; old coordinated behavior remains.
- **Recovery service:** project `concurrent`, `can_cancel`, `can_retry`, `cleanup_unresolved` into `ShellCommandProjection`. Determine capabilities using the current source thread and trusted active/recovery owner. Filter concurrent commands out of workspace-recovery `active`; keep them in `commands` so original cards continue receiving state and polling. Do not add Full Access to the risk-acceptance/unlock panel.
- **Single operation control:** `ShellRecoveryService::cancel` reads/reconciles first; if a concurrent command has neither live control capability, return observation without launching it. With a retained concurrent cleanup owner, retry only that operation's cleanup, append facts, reconcile original terminal commits, and release only after safe cleanup/confirmation. Otherwise invoke the existing session cancellation. Preserve thread/workspace/operation checks and never acquire authority from a stale PID.
- `agent/shell_operations.rs` and recovery reconciliation both forward `concurrent` into store records; `agent/mod.rs` and runtime `lib.rs` export concurrent cleanup APIs. Partial backend import would break these cross-crate contracts. No new dependency, Cargo change, shared type copy or workflow change is required by this delta.

## Exact overlap/hunk plan for the six mixed paths

Hunk coordinates below are OLD → NEW coordinates from `git diff OLD NEW`, not stable current line numbers. Source-symbol anchors are authoritative after parent edits.

### 1. Locales — context conflict only

Both upstream hunks start at line 1 and add the same two keys after `ui.collapse_save_failed_retry`. Current first-line copy is already migrated, so the full hunk fails its context check. Insert only the new keys, once per locale; do not restore upstream's first-line wording or overwrite the rest of either locale.

| Key | English | zh-CN |
| --- | --- | --- |
| `ui.shell_session.control_failed` | `Could not confirm command cleanup. Retry or check the command manually.` | `无法确认命令清理结果，请重试或手动检查命令。` |
| `ui.shell_session.cleanup_unresolved` | `Cleanup not confirmed` | `清理结果未确认` |

Existing `ui.shell_session.cancel`, `ui.shell_session.stopping` and `ui.shell_recovery.retry` supply the control labels; no new wording, panel or explanatory banner is needed.

### 2. `conversation_stream/content.rs` — additive subscriber

At `observe_tool_card` (current line 2208; upstream hunk OLD 2190 / NEW 2190), add the `ShellControlRequested` subscription calling `request_shell_control(event.operation_id.clone(), cx)`, and detach it as upstream does. Preserve the existing `SubagentOpenRequested` subscription, source-thread fence, `cx.observe`, item invalidation and layout notifications. This is shared by ordinary/grouped/restored cards; do not wire only a special demo card. Read-only patch check found this hunk matches with a +16-line offset.

### 3. `conversation_stream/mod.rs` — visibility, not helper resurrection

Upstream's first hunk changes `mod shell_recovery` to `pub(crate) mod shell_recovery` so its old ToolCard renderer can call the recovery GPUI button helper. Current module additionally exports `mounted_regressions` under `nsh-reading-integration` between the declaration and public business exports, so direct patch context fails.

With the required NSH-local button there is no sibling helper call; keep the current private module unless a real caller requires visibility. If retaining upstream visibility for source alignment, change only that declaration. Preserve the current feature-gated exports and public `ShellRecoveryAction/Outcome/Requested` API. **Do not reintroduce the old helper merely to justify this visibility change.**

### 4. `conversation_stream/shell_recovery.rs` — controller hunks, keep NSH lifetime fences

Transplant these upstream changes by symbol:

1. OLD 71 / NEW 71: add `pending_read: bool` beside `pending`; OLD 109 / NEW 110: initialize false in `ShellRecoveryUi::new`.
2. OLD 151 / NEW 153: add `pub(super) request_shell_control`. A pending non-read mutation rejects the card request and finishes its pending control with failure; a read poll may be superseded. Clear the pending read and dispatch `ShellRecoveryAction::Cancel(operation)` through `request_shell_recovery` (not direct runtime calls). Preserve the checked sequence increment so the old read completion cannot win. Make `request_shell_recovery` `pub(super)` as upstream, without changing its business decisions.
3. OLD 169 / NEW 190: set `pending_read = matches!(&action, ShellRecoveryAction::Read)` on dispatch. Current code also rotates `observation` at this seam; retain it, all current projection/confirmation identities, and the original source/draft/authority snapshot.
4. OLD 189 / NEW 211: after `request.matches_route(...)` succeeds, clear `pending_read`, finish matching card controls for `Cancel(operation)` using `result.is_err()`, and notify matching cards. Do not process a stale request before the existing thread/project/backend/mode/permission revision/draft authority/source/draft-route/sequence checks.
5. OLD 218 / NEW 253: suppress duplicate recovery-panel error only when the matching command card handled the cancellation result (`card_control`). Preserve `Changed` confirmation invalidation; errors with no matching card still use the existing recovery error.
6. **Do not import OLD 578 / NEW 615 (`fn button` → `pub(crate) fn button`).** Current migration removed OLD lines 580–635 along with GPUI recovery rendering. The helper no longer exists and must not be restored. Existing `nsh_view`, `keyboard`, reveal, mounted-regression wiring, summary/details distinction, Prepare dismissal, submitted Resume lifetime and late-completion fences remain.

The existing refresh predicate also inspects nonterminal `projection.commands`, so filtering concurrent entries out of `active` does not by itself stop their card refreshes. Keep that predicate. Do not route card Retry to panel `ShellRecoveryAction::Retry(projection)`; upstream intentionally emits `Cancel(operation)` for both cancel and retained-owner cleanup, with the service selecting the operation-local behavior.

`crates/vega/src/window/shell_recovery.rs` is unchanged by #922 and currently byte-equals OLD/NEW. Preserve its stream/draft ownership, exact Resume confirmation/source check, trusted mutation lease and worker service dispatch. No new card event may bypass it. Do not alter existing recovery dismissal/Resume behavior as an incidental redesign; verify the new card control alongside that behavior during parent integration.

### 5. `tool_card.rs` — retain the migrated model, not the old renderer

Bring across upstream's non-rendering changes:

- OLD 135 / NEW 135: `ShellControlRequested { operation_id: String }` plus `EventEmitter<ShellControlRequested>`; insert beside the current `ToolCard`, not the upstream doc-comment context which #907 removed.
- OLD 144 / NEW 149: `shell_control_pending` / `shell_control_failed`; initialize both false in **all four** current constructors (upstream OLD 174/203/227/261 → NEW 182/213/239/275).
- OLD 1496 / NEW 1557: in `apply_shell_command`, retain the exact original-result operation match and Bash-input guard. Clear failure only for resolved terminal `succeeded|failed|cancelled|timed_out` with `!cleanup_unresolved`; ordinary polling must not erase unresolved failure or pending control.
- Add `finish_shell_control(operation_id, failed)` matching the stored command operation, clearing pending and recording the explicit completion outcome. Notify through callers as upstream.
- OLD 1580 / NEW 1662: prepend ` · ` plus `ui.shell_session.cleanup_unresolved` to `bash_detail_title` only for `concurrent && cleanup_unresolved`. Preserve existing truncated/reused/incomplete-output suffixes and actual command-result facts. Existing `ToolDetail.title` already flows into NSH expanded details.

**Semantic modify/delete overlap:** current #907 deleted OLD lines 790–965 (the `render_with_title` GPUI body), while upstream changes OLD 826–834 and 951–954 to create the control/notice and attach them to that renderer. Do not recreate those hunks in `tool_card.rs`; express them in the existing NSH child below. The event insertion also touches the removed OLD 138–139 comments; preserve the repository's no-new-comments rule rather than resurrecting their context.

### 6. Required existing NSH child adaptation (NEW has no hunk here)

Parent implementation needs `crates/vega_ui/src/tool_card/nsh_tool.rs` in addition to the upstream path list. This is the directly affected existing presentation seam, not a new UI subsystem or unrelated product work. Keep its `Presentation`, `Snapshot`, retained `Surface`, `Request`, `publish_nsh`, layout authority/measurements, grouped-scroll behavior and main-thread controller ownership.

- `nsh_snapshot` (current lines 189–230) currently derives `notice` only from `headline_notice`. Give `shell_control_failed` the upstream priority, with exact translated failure copy and secondary text color; otherwise preserve normal headline notice behavior. Carry any color discriminator in the snapshot/equality rather than treating a failed cleanup as a successful terminal state. Include feedback in the existing accessibility label where applicable.
- Project an optional operation control only for `command.concurrent && (command.can_cancel || command.can_retry)`. Upstream label precedence is pending → `ui.shell_session.stopping`, otherwise retry capability → `ui.shell_recovery.retry`, otherwise `ui.shell_session.cancel`; enabled is `!shell_control_pending`. Visibility is not gated by expansion: upstream appends it outside expanded details. Do not hide cancellation behind expanding a card.
- Render that actual button with existing NSH controls, beside the existing notice/detail flow (currently notice at lines 712–721), not a legacy GPUI `shell_recovery::button`, raw DOM label, model-owned entity or replacement recovery panel. Preserve native pointer/keyboard/accessible control behavior and original cancel semantics; no blanket keyboard interception, fake completed state or copy-only substitute. Keep disclosure, copy-command/copy-output and subagent navigation intact.
- Send a bounded Surface request back to this original ToolCard. Bind the displayed operation and presentation identity; reject stale/unmounted/replaced presentation or no-longer-capable/mismatched operation and duplicate pending activation on the trusted main thread. Do not treat a rendered operation string as authorization. Use the existing async send/ack pattern without silent `try_send`, automatic retry or command re-execution.
- On accepted activation set pending true, failure false, emit `ShellControlRequested` and notify; actual completion comes only through the existing recovery request/result loop. Extend `Snapshot::same_content` for new projected fields so publication and measured-layout validity see notice/control changes. Let existing measurement update card/list geometry when control/error content changes; do not use a fixed height that clips the new control.
- Retain the GPUI host/controller/native focus and accessibility infrastructure; do not repaint old GPUI product controls as a fallback. NSH gets nonsecret labels, capability facts and opaque presentation/operation identity, not confirmation tokens, `Entity`, `Rc`, process handles or a new authority owner.

## New text-path accounting

| Upstream addition/change | Production path and adaptation |
| --- | --- |
| `control_failed` translation | NEW GPUI card `notice` → adapt current NSH `nsh_snapshot.notice` and visible/accessible notice. Persistent until explicit control result/resolved terminal facts, not a short success toast. |
| `cleanup_unresolved` translation | `bash_detail_title` → existing `ToolDetail.title` → `nsh_tool::render_detail`; model hunk carries it into NSH. It describes cleanup uncertainty, not failure of the command's true exit result. |
| Cancel / retry / stopping labels | New original-card action; existing translations, but a **new NSH button/request** is required. Native control operation and completion semantics must remain. |
| New Full Access read-invalidation failures and `session_policy_incompatible...` rejection | Runtime tool result/error content → existing audited result projection/output rows → `ToolCard::detail` / NSH detail output. Import exact upstream business strings; no separate label/renderer or rewriting of model/business data is needed. Preserve `Command NOT started` versus `command returned ... was not rerun`. |
| `bash` / `bash_wait` definitions and `wait_ms` schema description | Provider/tool contract text in runtime, not a new desktop control. Import exact backend bytes; do not change the investigation's available tool permissions based on source definitions. |
| Concurrent fields / SQL / feature documents | Internal facts/documents, not a reason to add status bars, counters or explanatory UI. |

No new panel, popup, unlock flow, icon system, theme dependency or startup/workspace route is introduced by this delta. Existing recovery/native controls are preserved; only the newly reachable original-card controls/text need the migration adapter.

## Safe minimal parent integration recipe (planned, not executed)

1. Coordinate with the current writer before source edits. Re-read HEAD, pinned OLD/NEW objects, affected bytes and current dirty/untracked status. Preserve the parent's other in-flight fixes. Do not stash/reset/checkout/restore the migration wholesale, operate on index concurrently, or assume this document freezes shared source. If any exact-copy candidate no longer equals OLD, stop that candidate's automatic copy and reconcile its newly observed delta.
2. In the parent's authorized integration operation, import only the nine byte-identical backend paths and three absent additions listed above from **pinned NEW**, not the task branch HEAD or moving master. A path-restricted binary OLD→NEW patch is also appropriate after `git apply --check` against that subset. Read back all 12 paths and compare raw bytes to NEW; ensure migration registration and SQL arrive together. This preserves earlier #916/#920 content because those current bytes are OLD and NEW includes that parent.
3. Add just the four locale entries and the `observe_tool_card` subscription. Apply the controller/model hunks by symbol, preserving migration identities and feature-gated development wiring. Resolve module visibility deliberately; never restore the deleted recovery helper or ToolCard GPUI renderer. Do not use `--reject`, force/ours/theirs wholesale, or conflict removal as acceptance evidence.
4. Implement the scoped NSH child adaptation above. Keep the public window/recovery business route unchanged. Parent should add this compatibility delta to its existing author contract before implementation, without reopening unrelated product investigation or its ordinary/read-only seam work.
5. Inspect final scoped differences and raw import equality, then run the parent's authorized current-cycle checks. Only after integration/adaptation and relevant regressions pass, take a new precise stopped baseline (commit plus dirty content manifest as applicable). An ancestry merge, source compilation, independent review, final-head CI and user/native acceptance are separate facts; none is established by this document.

### Textual application evidence, including an invalid probe

An actual read-only `git diff --binary OLD NEW | git apply --check --verbose -` equivalent was executed via Python subprocesses (no patch was applied), exit **1**. It checked all 18 paths:

- Nine existing backend plus three added paths: no reported application errors.
- `content.rs`: hunk 1 matched at 2206, offset +16.
- Locales: hunk at OLD line 1 failed (changed existing first-line context).
- `mod.rs`: first hunk failed (current feature-gated exports inserted into context).
- UI recovery: first three hunks matched at offsets +4/+9/+16, then the OLD 169 hunk failed at the current NSH observation-rotation seam. The command stops checking that file there; later hunks are **not** claimed checked. Source comparison additionally establishes the deleted-helper overlap.
- `tool_card.rs`: first hunk failed at OLD 135 because removed comments no longer match. Later hunks are **not** claimed checked by this command; the deleted renderer overlap was established by OLD/current/NEW line comparison.

A preliminary `git merge-file -p` probe using shell process-substitution `/dev/fd` inputs returned exit 0 and unchanged current-file hashes, without the known upstream additions. Its observation was not a valid three-way merge result and is explicitly discarded: **no conflict-free merge is claimed**. No ordinary-file temp artifacts, object writes or repository mutation were used to repair that probe. The evidence above instead uses raw byte equality, `difflib.SequenceMatcher(..., autojunk=False)` old-line overlap mapping, full upstream diff/source inspection, and the valid `git apply --check` result. There is no completed real merge and therefore no actual resolved conflict set.

## Parent verification required after implementation

These are next checks, **not executed/passed here**; use the parent's existing bounded runners/toolchain and preserve its acceptance assertions.

- Recheck all 12 exact-import identities, locale uniqueness/formatting, module exports and absence of resurrected GPUI card/recovery product painting. Review only the affected text/control paths for copy, keyboard and accessibility, including narrow layout and changed list geometry.
- Compile/lint the coherent runtime/tools/store/conversation/UI/application integration, not just the exact-copy backend subset. Verify SQL migration registration/defaults and constructor/projection consumers. No local Cargo/toolchain version was queried in this lane.
- Cover original-card cancellation while a Read poll is pending: one mutation dispatch with a new sequence, old Read completion rejected, matching result clears pending. A pending non-read mutation must reject the new card control with truthful local failure and no duplicate service action.
- Verify original-operation/call/thread matching, stale presentation/operation/capability rejection, duplicate press suppression, explicit failure visibility, ordinary polling retaining unresolved failure/pending, resolved terminal facts clearing failure, and retry using retained-owner operation-local cleanup. Confirm controls remain reachable when details are collapsed and grouped/restored cards use the subscriber.
- Preserve #916 recovery Prepare/close/Escape/late-confirmation and submitted Resume completion regressions, existing authority/draft/route fences, and no blocker release by presentation dismissal. Confirm concurrent commands stay out of the workspace unlock surface but still refresh original cards; retain coordinated recovery behavior.
- Verify Full Access nonexclusive versus sandbox-exclusive admission, capacities/resource retention, read invalidation, exact terminal commit readback, restart lost-control facts and operation-local cleanup. Reuse approved historical #922 evidence as context, not current #907 test results; do not restore master-cleaned Rust tests indiscriminately.
- Run the parent's ordinary-validation/read-only seam work separately as already owned. Final stopped-head independent review, required CI, Rust development-test archival/removal and native/user acceptance remain independent gates. Upstream historical docs/CI are not evidence of this migration's final version.

## Current content fingerprints / handoff fence

SHA-256 of raw inspected bytes. Rehash before edits because the parent remains active; changed inputs invalidate the corresponding copy/overlap conclusions rather than authorizing overwrite.

```text
77acacdd21e4283c8310d312a0ff67b6c69968be35e3eb067f51d0934a46364d  crates/vega_conversation/src/agent/shell_operations.rs
264c8148fd1816d1affbc17f83343ec4653e82c55b2440f8705dfe67c1032433  crates/vega_conversation/src/shell_recovery.rs
8ccb7e141d994c32368d2f2ed4ae2d3d5fcb321d23b09e0faf03c6990f4296c5  crates/vega_i18n/locales/ui.en.toml
b438ddf88929582631070f0d523fe52b9d14602e09f254ebf25ae686d35fed29  crates/vega_i18n/locales/ui.zh-CN.toml
206ac0d280f133095f358fe357bf4a6f355902923393f7e8f119aa742af2f950  crates/vega_runtime/src/agent/mod.rs
0e6d73836e9f6d1883187701654e4f5e66c33782e5718aee0d953c5a252ca34b  crates/vega_runtime/src/agent/shell_sessions.rs
2d3878b320ad5f35d350adf30a5baeab802c62bf3c18d0c79a5a15117cf94849  crates/vega_runtime/src/agent/tools_exec.rs
082353359c84c9578885bb9b026455f1f7d43d686f9f75e342f3d4161adb610a  crates/vega_runtime/src/lib.rs
ec23a8663ccd428e75f9ae8bec0131331fd2f72dd52d2f04c4a03cc84cbdd4f3  crates/vega_store/src/lib.rs
563424011f99731ea567b5867146382bd21451addac6cc66a862a71baeb3cb95  crates/vega_store/src/shell_operations.rs
957b25e5a521abcd28763a8aabee07fee19f8b9fc5233ef1df5e4fb4d6d9aeb6  crates/vega_tools/src/bash/session.rs
a10102a704a0269917c8b4a1a3b879ac35e9a10b165b3e5e9a38d352c4d7e823  crates/vega_ui/src/conversation_stream/content.rs
53418c247ce45ceb04611b1684976d9f9fa2a62f57873b9207815493eee20484  crates/vega_ui/src/conversation_stream/mod.rs
df104c45e6915dce6b2b48d92f3d5e43004ee024d4b3f95c8720a57bff687a1b  crates/vega_ui/src/conversation_stream/shell_recovery.rs
eff2a6fedc9ef9df1ff78267f052c98b81e66379038a0f840b9257b6c14ff5c9  crates/vega_ui/src/tool_card.rs
6e040fa3d224bbc103c3005b5d89f3cdc1cf7b55028d4389941d8be8ab24f400  crates/vega_ui/src/tool_card/nsh_tool.rs
17d8fe467197781be1ad8bd6831d0501e34cb9f050cd0af2310d9a7fbd2d5e66  crates/vega_ui/src/tool_card/nsh_tool/layout.rs
07a0e27df4a1f85858ec1bdea94b4b2b5ae7e95d034309be12cbbb7d32d73c1a  crates/vega/src/window/shell_recovery.rs
b0d36bf87e0865819c5042277f382739e831c623fed90d77c1582a098594affb  crates/vega_ui/src/conversation_stream/shell_recovery/nsh_view.rs
```

The SQL and two `docs/921/` additions were absent at the original investigation snapshot. The original investigation created only this document. Followup implementation status and current byte identities follow; do not use these historical fingerprints as current source identity.

## Authorized followup implementation — stopped READY

Pinned inputs remain HEAD `0b5ea426abfd8d4358489e916273d89474fb190f` (parent-supplied; no Git commands in this followup), OLD `0b3dcd3603a8e97e0ef74ae25a26f17fc3a094e5`, NEW `2a29adf9b2ea78eaf59d591838531b45adc7373e`. Dirty-byte identities below, not HEAD alone, identify this lane's implementation. Parent imports progressed concurrently: a final focused source read confirms the command projection now declares `concurrent`, `cleanup_unresolved`, `can_cancel` and `can_retry` with the expected types. This lane did not claim all 12 parent imports verified.

### Implemented source changes

- Both locales: only the two exact upstream cleanup error/uncertainty translations; existing Cancel, Stopping and Retry copy reused.
- `tool_card.rs`: upstream event, all four constructor defaults, pending/failure lifecycle, operation-matched explicit completion, resolved-terminal-only polling failure reset and cleanup uncertainty detail suffix. Added a pure original operation/call/thread predicate used by the common subscriber.
- `content.rs`: common observed-card event subscription covers ordinary/grouped/hydrated cards. Before dispatch it verifies emitting entity remains the map owner at the original call id and command source thread matches the current thread; rejected source finishes locally with failure, without service dispatch.
- `shell_recovery.rs`: pending Read supersession via a fresh checked sequence, pending mutation rejection, matching Cancel completion and card-local error handling. Sequence exhaustion also finishes the card with failure rather than leaving a permanent pending state. Existing route/project/backend/mode/permission/draft authority and sequence completion gate still runs first; observation rotation, Prepare dismissal, submitted Resume, refresh predicate and NSH presentation code remain.
- `nsh_tool.rs`: native Button outside expanded details, capability/label/enabled projection, secondary-colored persistent cleanup failure in visible and accessible notice, measured-layout invalidation through snapshot equality. Bounded async send/ack binds full displayed content, opaque presentation and mount authority plus current original Bash operation and live capability; stale/unmounted/replaced/duplicate/no-capability requests do not dispatch. Explicit activation clears old transport feedback. No legacy GPUI recovery helper or product renderer was restored; disclosure, copy controls and subagent navigation remain.
- `mod.rs`: existing private recovery module remains private; only feature-gated exclusive regression export added.

Business authorization still belongs to the existing `ShellRecoveryRequested` application/service route; both Cancel and Retry dispatch `Cancel(original_operation)` and never launch/relaunch a command. Full Access classification/admission, retained-owner cleanup, read invalidation, resource ownership and coordinated blockers are parent-imported backend contracts, not reimplemented in UI. No unlock/confirmation token/process handle or entity is transported into NSH, and no Full Access recovery panel was added.

### Development regression authored, NOT run

Exclusive files:

- `crates/vega_ui/src/tool_card/nsh_tool/mounted_regressions.rs`: bounded native-control observations, operation mismatch/stale request probes, model assertions and Surface shutdown.
- `crates/vega_ui/src/conversation_stream/shell_recovery/command_control_regressions.rs`: real mounted ToolCard/NSH Surface plus existing common subscriber and recovery controller. Pointer input is dispatched through the GPUI window to the actual native button; test service outcomes are injected at the existing recovery result boundary, not a real runtime/process service.

The authored case covers collapsed Cancel reachability; Stopping/disabled pending projection; newer Cancel sequence superseding pending Read; obsolete Read rejection; duplicate/stale and forged-operation requests; explicit cleanup failure/accessibility notice; unresolved polling preserving failure; retained-owner Retry selecting the original operation; pending non-Read mutation rejection without a second service dispatch; terminal resolution clearing failure; changed source thread/call map rejection; coordinated-capability loss; and unmounted presentation rejection. Assertions are authored coverage, **not passed evidence**. Existing #916 fixture/tests and scroll fixtures were not overwritten or broadly restored. Grouped/restored behavior uses the shared subscriber but is not newly executed here. Native keyboard/accessibility behavior, narrow/wrapped layout, actual cleanup service and existing #916 recovery regressions remain parent validation requirements.

**Parent shared runner wiring needed (this lane did not edit a shared runner):** with existing `nsh-reading-integration`, invoke:

```text
vega_ui::conversation_stream::command_control_regressions::mounted_cancel_supersedes_read_and_retry_keeps_operation_authority(cx)
```

It accepts the existing `&mut gpui_kit::TestAppContext` and needs the same NSH/native font test environment as existing mounted entries. The feature-gated support/export/instrumentation is development-only and must join the parent's Rust test archival/removal inventory before master. Do not remove this unrun case to make validation green; first run/diagnose it in the central build.

### Actual verification and remaining gates

- `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)` queried. Final `rustfmt --check --edition 2024 --config skip_children=true` succeeded over the seven owned Rust source/support files, without traversing shared child modules. Initial formatting differences were formatted in scope. This is syntax/format evidence, not type-check/build evidence.
- Python `tomllib` successfully parsed both full locale files (duplicate keys are rejected), and raw SHA-256 fingerprints were recorded. New support was statically checked for no comments. Manual focused source checks confirmed native request predicates, snapshot equality, existing route gate and imported projection API.
- **Not run:** Cargo/rustc/build/Clippy, mounted regression or other tests, native/manual acceptance, CI, independent review. No compilation success or passing behavior is claimed. Parent central build must check coherent imported backend/UI/application, wire/run the case, retain #916 recovery/scroll/native regressions, validate actual operation-local service cleanup and source-route authority, and resolve any resulting compiler/harness/product failures at their responsible layer.

### Final implementation SHA-256

```text
66cca53efa9046c30a35cb163d20a052abbf70181c98d1cf10e94892cefc969b  crates/vega_i18n/locales/ui.en.toml
7e4f60166ec6cdd45f30dc0277e6c702101a3df76c8b5ce85112d8c5120f7254  crates/vega_i18n/locales/ui.zh-CN.toml
eb5d90ccbe38b2f4ed2449dd5c9e8d3c7729917cbfb6f6ed569314e9ea43a318  crates/vega_ui/src/conversation_stream/content.rs
1328f7c5e5fb25aa21d000581be3a864825e5b6ef23d70c21ca765d3fa822657  crates/vega_ui/src/conversation_stream/mod.rs
9859138de6361ec4673b0dbb8dcba890b4ed08ce681668cd73943a66eea99a4c  crates/vega_ui/src/conversation_stream/shell_recovery.rs
1610afc2d3bde2279308e4fdbf38b07d2095b6863968e459000711291bab5285  crates/vega_ui/src/tool_card.rs
46b6a66071922cbca0aa73bef5a7ea137e076322b7b7304e8d0382a41a4efafb  crates/vega_ui/src/tool_card/nsh_tool.rs
9181fae79f639c311af05c07213e4086b66f28ae47a9ab18b73b7cdd446844df  crates/vega_ui/src/tool_card/nsh_tool/mounted_regressions.rs
7259aed7395d8aff14dbd7ce7517eacd7ffa5cb0050ec1a8acd62d81ba506453  crates/vega_ui/src/conversation_stream/shell_recovery/command_control_regressions.rs
```

STOP READY at the preceding implementation handoff: all authorized implementation precedes independent review. This was **UI implementation readiness only, not full integration/test/review/merge completion**. The subsequent authorized runtime-diagnostic stage follows.

## Runtime failure followup — diagnostic stage READY

Pinned HEAD/OLD/NEW remain the parent-supplied versions above; no Git commands were used in this stage. Root instructions and execution guide were reread; a worktree file search found no nested `AGENTS.md`. This stage owns only the original authorized UI paths, two exclusive development fixtures and this document; parent owns the central build/run and shared runner. No Cargo/build/test execution, Git/index/network, vendor/backend/dependency/configuration changes, comments, suppressions or timeout-budget changes were made here.

### Original evidence inspected

- `target/nsh-current-text-mainline-execution.json`: ordinary integration exit 0, elapsed 4.704s; recovery integration exit 1, elapsed 14.943s, watchdog false; full UI exit 0. Ordinary stdout records 4 passes; full UI stdout records 358 passed, 0 failed. These are prior parent-run results, not validation of the diagnostic bytes below.
- Recovery stdout: both existing recovery cases PASS, new mounted command-control case FAIL; 2 passed, 1 failed, 0 unexecuted/ignored. Recovery stderr lines 2265–2268 identify the panic at original `command_control_regressions.rs:145`, `mounted command control deadline`. This is `ready()`, not the click acknowledgement or drain wait. The old message contains no phase, and no original backtrace/host/projection/control snapshot establishes which `ready()` call failed.
- The repaired build JSONL ends with `build-finished`, success true. The parent reports exit 0 / 51.98s and its compile repairs: `ToolCallStatus` import, control-key static lifetime, `view.stop()` signature, and existing recovery record `concurrent: false`. This stage preserves those repairs and does not write the existing recovery fixture/runner.

### What is established, and what is not

`controls::observe()` requires presentation without app error, a recorded control matching the current snapshot id and all content, positive recorded bounds, and current accepted measured size. Any of those missing conditions produces the same old timeout. `record_control()` currently runs only from a native `on_layout` callback. Layout-event snapshot freshness is therefore a concrete observation risk, not an established diagnosis of this particular original run.

The inspected native API (`vendor/nsh-core/src/core/measure.rs`) explicitly defines `Measurement.frame` as parent-relative and `Measurement.page` as surface-root-relative with surrounding scroll offsets applied. The fixture currently adds recorded `frame.origin` to the GPUI host origin. This is a coordinate-contract risk for nested controls, but the original failure precedes an unidentified readiness result and does not prove a missed click. No speculative production/observer/coordinate rewrite was made before phase evidence. In particular, a historical layout record is not newly claimed to establish current displayed text, nor is helper rejection/activation claimed as native accessibility acceptance.

### Diagnostic changes in this stage

- `command_control_regressions.rs`: every readiness wait and actual mouse click acknowledgement wait now names its scenario (initial Cancel, pending Stopping, failure visibility, cleanup Retry, completion, pending-mutation rejection, source thread/call rejection, capability loss and unmount preparation). Both original 10-second limits, frame/drain behavior, readiness predicate and all interaction/business assertions are unchanged. On deadline only, diagnostics read GPUI host bounds, root mounting, request count, current source-thread/call ownership and recovery sequence/pending/read state.
- `nsh_tool/mounted_regressions.rs`: bounded failure-only snapshot includes presentation/app errors, mounted/current-layout width, accepted measured size, published/current id/content/layout matches, current operation/label/enabled/notice, and up to eight recorded controls with current id/content/layout comparisons. It independently reacquires `AppView::integration_text_observation()` for up to eight current native text nodes with frame/content bounds and line counts, truncating diagnostic strings at 160 characters. Missing live observation remains `None`, not success. No engine commands or mounted-state changes are used for this text observation.
- All production source remains untouched in this diagnostic stage. The actual mouse move/down/up → NSH press → `Request::ShellControl` → `ShellControlRequested` → recovery controller sequence route is retained; pending Read supersession, mutation rejection, exact operation/source authority, error/Retry assertions and stale-control rejection were not removed or relaxed. None is newly claimed passed.

### Exact byte delta and verification

| Path | Before this stage SHA-256 (includes parent compile repairs) | Diagnostic-stage SHA-256 |
| --- | --- | --- |
| `crates/vega_ui/src/tool_card/nsh_tool/mounted_regressions.rs` | `046e92fd2e16746766001f6a8a27c69d19d33453218dbf330b6ca7c9da051994` | `86637b9eadb1f3d1e24535047abb25087980087fc1f0ae3c8bb6fe3205b302a4` |
| `crates/vega_ui/src/conversation_stream/shell_recovery/command_control_regressions.rs` | `c47ef0fb10f3777e9ec1c0f1b070cc2700e950027e551ffc2e3d03d2215fed0d` | `035818b40b0afe409cfd4565a1b1086fca16a0b741f53cbf007fd2728539efcd` |

Unchanged production `nsh_tool.rs` SHA-256: `46b6a66071922cbca0aa73bef5a7ea137e076322b7b7304e8d0382a41a4efafb`.

Executed: focused `rustfmt --check --edition 2024 --config skip_children=true` over both exclusive fixtures succeeded after formatting only the diagnostic support file; hashes were read; both fixtures were checked for comments/suppressions with none found. One atomic multi-edit initially failed due to ambiguous matching and made no changes; the fixture was reread before a uniquely contextualized edit. These checks do not establish Rust type correctness or runtime behavior.

**Next concrete verification:** parent central rebuild and run the unchanged three-case recovery integration target against these bytes. If it fails, return the named deadline and bounded snapshot from stderr, plus current execution/build records. That evidence will distinguish missing presentation/mount/measurement, stale layout-event snapshot, missing/current native text, missed click and controller acknowledgement; fix the responsible layer in one scoped batch, retaining the original 10-second deadlines and every business/safety assertion. The known measurement-coordinate risk must be addressed with any evidenced observer/click correction, not by relaxing predicates or invoking the control helper instead of the actual button.

**STOP READY — diagnostic stage only (historical handoff).** No Cargo/build/test, independent review, native acceptance or passing mounted-control claim for those bytes. The next authorized observer-repair stage follows.

## Current-control measurement repair — authorized contract

Pinned HEAD, OLD and NEW remain the exact versions above; no Git command is authorized in this stage. The parent-supplied central run is now available as `target/nsh-command-control-predicates-execution.json`: executable SHA-256 `482a768f6ae94b22f93f13b00a113a1287e7cf08ad5ab7ab5a70e074b2e3f282`, exit 1, 14.902 seconds, no watchdog timeout. Original stderr establishes `phase=pending Stopping`, sequence 2, pending Read false, all published id/content/layout comparisons true, accepted size 360 × 49, current control `(original-operation, Stopping command, false)`, and fresh native texts `Command running` / `Stopping command`. The sole recorded control has obsolete id/content and unchanged layout. Parent reports the prerequisite build succeeded in 26.55 seconds; that is not validation of the upcoming repair.

The responsible layer is the development measurement observer: its sole `on_layout` callback cannot observe same-size content/projection changes. There is no reactive measurement effect on that button to rerun. The actual production press callback reads `state.get_untracked()` at press time and the model/controller check current authority; the evidence does not establish a stale production action closure.

Repair acceptance, without changing the business contract or budgets:

- Development-only observation must measure the actual existing button ref on tracked snapshot changes and real layout events. Measurement is a new `ElementRef::measure()` request, never a historical geometry record rebound to a new snapshot.
- Keep the snapshot captured before the request; accept a result only while id, full content and layout authority still match the current signal. Revalidate those same fences against the card before readiness, and independently reacquire current native text containing the control label and any current notice. Missing live text or measurement is not readiness.
- Store the actual measurement's surface-relative `page` origin with its measured frame size. Parent-relative `frame.origin` is not a surface/window coordinate; no text frame or label-derived estimate substitutes for button geometry.
- Retain the original 10-second deadlines, actual GPUI pointer move/down/up, native button press, subscriber/controller sequence route, pending Read supersession, pending mutation rejection, original operation/source validation, failure/Retry visibility, completion and stale-control rejection. Do not add product rerenders, copy, helpers that activate the action, vendor APIs, dependencies or shared runner changes.

### Implemented observer repair and byte delta

- `nsh_tool.rs`: only feature-gated development instrumentation changed. A tracked `effect` requests measurement on each snapshot change after reactive bindings; the existing real layout callback uses the same measurement routine for initial mount and geometry changes. No production label, disabled state, press handling, controller or forced rerender changed.
- `nsh_tool/mounted_regressions.rs`: new measurement routine captures the requested snapshot before awaiting the actual button ref measurement, discards results whose id/content/layout authority no longer match the current signal, rejects nonfinite/nonpositive geometry, and retains bounded records. Recorded origin is `measurement.page`; size is `measurement.frame.size`. Readiness now rechecks id/content/layout against the actual card, accepted card size and app errors, then reacquires current native text and requires the exact current control label plus any current notice. This strengthens observation rather than relaxing readiness. It does not claim native accessibility or pixel acceptance.
- `command_control_regressions.rs` is byte-identical to the previous diagnostic stage. Every original deadline, real pointer click, operation/source/sequence assertion, Read supersession, pending mutation rejection, failure/Retry/completion and stale-control check remains unchanged. The shared measurement correction applies to all named future phases, not only Stopping.

| Path | Before repair SHA-256 | After repair SHA-256 |
| --- | --- | --- |
| `crates/vega_ui/src/tool_card/nsh_tool.rs` | `46b6a66071922cbca0aa73bef5a7ea137e076322b7b7304e8d0382a41a4efafb` | `19274fe66c25eb181662c7df0089582808191847f3c807b7ade38205ebf398ae` |
| `crates/vega_ui/src/tool_card/nsh_tool/mounted_regressions.rs` | `86637b9eadb1f3d1e24535047abb25087980087fc1f0ae3c8bb6fe3205b302a4` | `f3634c60abfafbe2b01a255f7a84136d65157785b580e8898a1140cc94a2f7f6` |
| `crates/vega_ui/src/conversation_stream/shell_recovery/command_control_regressions.rs` (unchanged) | `035818b40b0afe409cfd4565a1b1086fca16a0b741f53cbf007fd2728539efcd` | `035818b40b0afe409cfd4565a1b1086fca16a0b741f53cbf007fd2728539efcd` |

### Actual final-version checks and next verification

Executed `rustfmt --check --edition 2024 --config skip_children=true` over the two changed Rust files plus unchanged command-control fixture: passed. Re-read the edited measurement/hook ranges and recorded SHA-256. Scoped fixture search for comments and lint/test suppressions returned no matches (the combined shell command therefore exited 1 from the final `rg`, not from rustfmt). No source formatting rewrite was needed. No Cargo, build, runtime test, Git, network, dependency/config change or vendor write was performed. Shared runner, existing recovery fixture, backend and parent ordinary-text repair files were not edited.

**Next concrete verification:** parent central rebuild and execute the same three-case recovery integration target, retaining the original budgets and every assertion. The named Stopping phase must now acquire new button geometry and live text; subsequent failure/Retry/pending-mutation/source/capability/unmount phases must also finish without direct helper activation. Preserve ordinary 4/4 and full UI regression checks as independent parent gates; prior results do not validate these new bytes. If a phase still fails, inspect its current bounded snapshot rather than widening budgets or substituting historical geometry/text.

**STOP READY — observer repair implemented, runtime result unverified.** This is author implementation handoff, not independent review, integration completion, native acceptance or a passing mounted-control claim. Parent owns central build/run and final baseline; this lane stops writing after handoff.

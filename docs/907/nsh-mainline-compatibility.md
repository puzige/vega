# #907 mainline compatibility — author integration preparation

## Scope and exact versions

This is an author integration map, **not independent review or a review gate**. Finish all implementation before the consolidated review required by [nsh-full.md](nsh-full.md). This lane writes only this new document; the parent owns Git/index and runtime integration. Preserve the existing dirty/untracked tree. No fetch, network, Git mutation, Cargo/rustc, application execution, credentials or user-data inspection was performed here.

Worktree: `/Users/puzige/Workspace/worktrees/vega-907-nsh-full`.

| Identity | Full SHA |
|---|---|
| Observed worktree HEAD | `0b5ea426abfd8d4358489e916273d89474fb190f` |
| Mainline comparison base | `34555cec9c0c4c4c1f7a528d739752f414e53edf` |
| #916, parent = comparison base | `f3688f5ae0b7becf1f46434c7b7d732e3a3c759a` |
| Supplied fetched master / #920, parent = #916 | `0b3dcd3603a8e97e0ef74ae25a26f17fc3a094e5` |
| Historical #920 development evidence | `75fb56d14ac913828e569b09137e2ef05e14ab6b` |

The supplied master commit was read from local objects, not refreshed or checked against the network. HEAD alone does not identify this dirty implementation. Read `AGENTS.md`, [execution guide](../vega-exec-guide.md), [workflow](../vega-issue-workflow.md), and the affected #907 recovery, full-migration, editor and subagent contracts. No nested `AGENTS.md` was found in this worktree.

## Parent integration result after resumed inspection

Parent fetched `origin` and observed master `0b3dcd3603a8e97e0ef74ae25a26f17fc3a094e5`. The task branch's remote remains `0b5ea426abfd8d4358489e916273d89474fb190f`; `git merge --ff-only origin/feat/907-nsh-full` reported already up to date. No rebase, commit or master ancestry integration is claimed for this dirty tree.

After rechecking that all three runtime/store files matched #916 and both #918 documents were absent, parent applied only the exact five-path #916 → #920 binary diff. All five resulting files were read back and byte-compared with #920 successfully. Existing migration changes were preserved; no tests, dependency or CI wiring were imported.

Actual official Cargo/rustc 1.98.0 commands, with `CARGO_FORK_AUTO_UPDATE=0`, offline/locked, two build jobs and `--target-dir target/official-nsh-validation`:

- `cargo check -p vega_conversation -p vega_store`: exit 0, 2.92s; `target/nsh-resumed-mainline-check.{stdout,stderr}`.
- `cargo clippy -p vega_conversation -p vega_store --all-targets -- -D warnings`: exit 0, 8.54s; `target/nsh-resumed-mainline-clippy.{stdout,stderr}`.
- Scoped `git diff --check`: exit 0.

These checks establish integration compilation/lints, not execution of historical #920 development tests. Earlier missing-delta statements below describe the author handoff before this parent application; current branch ancestry, mounted recovery and final exact-head gates remain pending.

## Later parent verification — superseding runtime status only

Parent fetched again and observed the same mainline/task identities above. A fresh read-only byte comparison confirmed all five #920 paths and the two service/window recovery sources still exactly match `0b3dcd3603a8e97e0ef74ae25a26f17fc3a094e5`. This remains dirty-byte compatibility, not ancestry integration or a new commit.

The subsequent current development cycle compiled/executed the real recovery runner **2/2**, ordinary application tests **31/31**, workspace mounted runner **6/6**, and workspace lib/bin Nextest **378 run / 378 passed / 0 skipped**. Strict workspace/all-targets Clippy with both NSH development features passed. Exact commands, executable identities and logs are recorded in [integrated validation](nsh-reading-editing-integrated-validation.md). The earlier recovery notification, selector and keyboard-fixture failures below are historical and no longer current blockers; their original evidence is retained.

There is still **no current behavioral execution of #920's historical 12-case suite**. The ordinary workspace pass does not recreate removed runtime/store tests, and the exact upstream bytes plus compilation/lints do not certify every concurrent-persistence invariant below. No old test module is restored. These are explicit validation limits, not a new runtime implementation scope or permission to modify upstream services. Later sidebar rename/IME test increments still require their affected checks; consolidated review, final-version gates and desktop-native acceptance remain pending.

## Smallest integration map

### #920: missing production runtime delta, no NSH API migration

The exact #916 → #920 delta has **five paths**: three production Rust files below, plus `docs/918/preparation-concurrency.md` (33 added lines) and `docs/918/validation.md` (30 added lines). No UI, locale, Cargo manifest/lock, migration or event-type changes are part of #920.

| Production path | Exact delta | Required parent action |
|---|---|---|
| `crates/vega_conversation/src/agent/pipeline.rs` | +162 / −87 | Integrate the exact preparation delta with both store additions, not an NSH-side workaround. |
| `crates/vega_store/src/lib.rs` | +16 / −0 | Retain `Store::immediate_transaction_with_budget`, forwarding the runtime-event category and cancellation/budget. |
| `crates/vega_store/src/write_coordination.rs` | +63 / −0 | Retain `WriteTransaction::new_with_budget`; do not substitute longer SQLite/FIFO timeouts. |

At the author handoff, all three files were byte-identical to both comparison base and HEAD: **#920 was not yet integrated**. Parent subsequently rechecked and applied the exact delta as recorded above. The resulting five paths now match #920; the Git HEAD remains unchanged and does not alone identify this dirty integration.

Affected runtime invariants, not additional implementation scope:

- Drop the initial permission-coordination guard after initial authority capture. Prepare redaction, delegated payloads/history and cross-thread tool-ID prefilter without holding the final writer or permission guard. Reacquire in **permission → writer** order before final authority verification/install; retain the later source, Skill and credential checks.
- Set the preparation connection's SQLite busy timeout to zero before budgeted admission. Share a **10-second cumulative final-admission budget** across FIFO, BEGIN and backoff. This is not an end-to-end preparation limit and excludes lock-free computation and permission-mutex acquisition. Retry only exact plain `SQLITE_BUSY` before BEGIN succeeds, releasing the FIFO permit between attempts; retain cancellation and nontransient error diagnostics. Do not replay a successful/unknown commit.
- Delegated snapshot conflicts roll back and recompute at most twice (three projection attempts total), subject to remaining admission budget/cancellation. Changed authority rejects rather than acquiring fresh permission. Preserve revision/schema/message-sequence validation in `crates/vega_store/src/preparation.rs::{PreparedDelegatedInstall::validate,PreparedDelegatedInstall::install}` and atomic source/redaction/checkpoint/assistant installation.
- `foreign_call_ids` becomes a prefilter outside the writer, not execution authority. Preserve the fresh `RuntimeEvent::ToolCallProposed` identity check in `crates/vega_conversation/src/agent/events.rs` and database uniqueness, persistence-before-execution, existing usage/idempotency and unknown-commit handling. #920 does not rewrite these paths.
- NSH remains presentation/command transport. Preserve current Composer submission/readiness and `SubagentOpenRequested { source_thread_id, thread_id }`; hiding/unmounting a presentation must not cancel business work. No composer, source-runner, validation-evidence or #919 implementation belongs to this lane.

### #916: retain the manually integrated authority, not the old GPUI rendering

- `crates/vega_conversation/src/shell_recovery.rs` and `crates/vega/src/window/shell_recovery.rs` are currently **byte-identical to #916 and #920**. The service's draft route validates an unsaved active Native root thread, permission revision/epoch and reset state against the project workspace. Confirmations bind the draft route; Resume rejects a different service route. The window rejects a mismatched stream/draft/source/confirmation and selects `ShellRecoveryService::for_draft` only for the current draft. Keep these files; do not overwrite with pre-#916 versions.
- `crates/vega_i18n/locales/ui.{en,zh-CN}.toml` retain the exact #916 `ui.shell_recovery.status_unavailable` value. Each has four additional NSH editor entries (`ui.reload_view`, `ui.editor_sync_failed`, `ui.copy_local_draft`, `ui.restore_local_draft`); thus whole-file locale equality with #916 is **not** a current claim.
- `conversation_stream/shell_recovery.rs` retains request identity `(thread, project, draft, sequence)` and `same_recovery_authority` over backend, mode, permission mode/revision, draft epoch, default permission and full-access grant. Closing unsubmitted Prepare invalidates its pending sequence/late confirmation; closing submitted Resume preserves its pending decision until matching completion. NSH observation/projection/confirmation identities add presentation fences, not replacement business authority.
- `conversation_stream/core.rs` retains all four #916 reset sites: changed thread authority, changed draft route, backend switch and draft rebind. `conversation_stream/render.rs` mounts `render_shell_recovery_overlay(window, cx)` outside reading preview. Keep the current NSH implementation in `shell_recovery/{nsh_view,keyboard,reveal}.rs`, not #916's GPUI buttons/details: ordinary draft hidden, blocked/unavailable draft summary, explicit Review opens separate details while retaining summary, persisted route inline details.

These are source compatibility observations, not proof of mounted layout, keyboard/focus, AX or service lifecycle behavior. The [recovery handoff](nsh-shell-recovery.md) remains the owner of that acceptance and its diagnostic evidence.

## Current content identities

Observed raw file bytes, SHA-256 (no Git filters). Paths in the second table are relative to `crates/vega_ui/`. These identities are a snapshot, not a freeze imposed on other authors; rehash after their changes and before parent integration/validation.

| Path | Current SHA-256 |
|---|---|
| `crates/vega_conversation/src/agent/pipeline.rs` | `69666769bbea39aebaa93f44a9c72af9b5b45988077ae2be1020f81ff6f52be7` |
| `crates/vega_store/src/lib.rs` | `118ef2b1000ad39473e8363c8b52568dac278a9738d7edbb18d72f85e7f078a0` |
| `crates/vega_store/src/write_coordination.rs` | `922080033cf601fe2d5b825675acbd0b4009563467d00887f13b02f04e7e41e4` |
| `crates/vega_conversation/src/shell_recovery.rs` | `264c8148fd1816d1affbc17f83343ec4653e82c55b2440f8705dfe67c1032433` |
| `crates/vega/src/window/shell_recovery.rs` | `07a0e27df4a1f85858ec1bdea94b4b2b5ae7e95d034309be12cbbb7d32d73c1a` |
| `crates/vega_i18n/locales/ui.en.toml` | `8ccb7e141d994c32368d2f2ed4ae2d3d5fcb321d23b09e0faf03c6990f4296c5` |
| `crates/vega_i18n/locales/ui.zh-CN.toml` | `b438ddf88929582631070f0d523fe52b9d14602e09f254ebf25ae686d35fed29` |

| Relative UI path | Current SHA-256 |
|---|---|
| `src/conversation_stream/core.rs` | `5222be27db179c54076f4a162ca9ea04c0ff04d6ee616b8674e08bbc8cd23270` |
| `src/conversation_stream/render.rs` | `64174d622992fd53901ffa16787a42b15914067b331b648ec40a900b96163712` |
| `src/conversation_stream/shell_recovery.rs` | `df104c45e6915dce6b2b48d92f3d5e43004ee024d4b3f95c8720a57bff687a1b` |
| `src/conversation_stream/shell_recovery/nsh_view.rs` | `ebcc6eb0ed52579adf4e86418b866f368e0685fd2c36b2900a191707c7cf76b8` |
| `src/conversation_stream/shell_recovery/keyboard.rs` | `7cb931e71512a4d165a407a6522f65019d566a537396e1f59fe16ff26696b08c` |
| `src/conversation_stream/shell_recovery/tests.rs` | `cd54dbdd7385e7df588aab707b57dbaea8b0f40175a33d7663f25c94c2dfcf66` |
| `src/conversation_stream/shell_recovery/mounted_regressions.rs` | `c77184684377f91199483d1345f8404f84117efeabf5bc60e4939822a03c3c3b` |
| `tests/nsh_recovery_integration.rs` | `1c7bb60deb24323afc88bf19175e99961128c5c8c2b959c834ee7669eaf5337b` |
| `Cargo.toml` | `fb91a1361919c742c34f81651c5e82d1bd4ae73b6cdf250e123fb68391e7e4b5` |

Expected Git blobs after exact #920 production integration:

```text
9796da7a229c71a73c9a97d285b00929a3e2653a  crates/vega_conversation/src/agent/pipeline.rs
672076ad3032f04d896a2ee6e225916104874681  crates/vega_store/src/lib.rs
d05709a3fe2b429c851eba833065fc148aba27d9  crates/vega_store/src/write_coordination.rs
```

## Exact inspection and parent validation commands

Read-only reproduction from the worktree root; these commit/path reads were used in this lane:

```sh
cd /Users/puzige/Workspace/worktrees/vega-907-nsh-full
git rev-parse HEAD
git log --oneline --first-parent 34555cec9c0c4c4c1f7a528d739752f414e53edf..0b3dcd3603a8e97e0ef74ae25a26f17fc3a094e5
git diff --no-ext-diff f3688f5ae0b7becf1f46434c7b7d732e3a3c759a 0b3dcd3603a8e97e0ef74ae25a26f17fc3a094e5 -- crates/vega_conversation/src/agent/pipeline.rs crates/vega_store/src/lib.rs crates/vega_store/src/write_coordination.rs
git show 0b3dcd3603a8e97e0ef74ae25a26f17fc3a094e5:docs/918/preparation-concurrency.md
git show 0b3dcd3603a8e97e0ef74ae25a26f17fc3a094e5:docs/918/validation.md
git diff --no-ext-diff f3688f5ae0b7becf1f46434c7b7d732e3a3c759a -- crates/vega_conversation/src/shell_recovery.rs crates/vega/src/window/shell_recovery.rs crates/vega_i18n/locales/ui.en.toml crates/vega_i18n/locales/ui.zh-CN.toml
shasum -a 256 crates/vega_conversation/src/agent/pipeline.rs crates/vega_store/src/lib.rs crates/vega_store/src/write_coordination.rs
```

**Parent/validation owner only; not executed here.** After integrating #920 and coordinating active writers/build capacity, bind output to final source hashes and actual toolchain. Use existing fixed tooling, no install/update:

```sh
CARGO_FORK_AUTO_UPDATE=0 rustup run 1.98.0 cargo check -p vega_conversation -p vega_store --locked --offline -j 2 --target-dir target/official-nsh-validation
CARGO_FORK_AUTO_UPDATE=0 rustup run 1.98.0 cargo clippy -p vega_conversation -p vega_store -p vega_ui -p vega --all-targets --features vega_ui/nsh-reading-integration --locked --offline -j 2 --target-dir target/official-nsh-validation -- -D warnings
CARGO_FORK_AUTO_UPDATE=0 rustup run 1.98.0 cargo test -p vega_ui --lib --features nsh-reading-integration conversation_stream::shell_recovery:: --offline --locked -j 2 --target-dir target/official-nsh-validation
CARGO_FORK_AUTO_UPDATE=0 rustup run 1.98.0 cargo test -p vega_ui --features nsh-reading-integration --test nsh_recovery_integration --offline --locked -j 2 --target-dir target/official-nsh-validation --no-run --message-format=json
```

The last command is **compile only**. Do not execute the known-blocked mounted binary unchanged or repeat the combined source/recovery timeout. Only after the separate composer author has repaired/tested the notification loop should the validation owner execute the original two-case runner, with its existing external watchdog policy in [nsh-shell-recovery.md](nsh-shell-recovery.md). This `harness = false` runner takes no libtest case filter: use the emitted executable, not a guessed artifact or appended test-name argument. The source-runner lane remains separate.

Historical #920 tests are reference evidence, not current executable coverage:

```sh
git show 75fb56d14ac913828e569b09137e2ef05e14ab6b:crates/vega_conversation/src/agent/pipeline/concurrency_tests.rs
git show 75fb56d14ac913828e569b09137e2ef05e14ab6b:crates/vega_conversation/src/agent/pipeline/concurrency_tests.rs | shasum -a 256
```

That object is locally readable and hashes to `0628fccc194284d990514272433934b132e85a1fa5298ea6ee7c3ab4292201c9`, matching upstream's record. #920's delivered tree removed the module/hooks. **Do not restore historical Rust tests or claim that a `concurrency_tests` filter on the current tree ran 12 tests.** Parent/validation owner must identify current authorized development coverage for the behaviors below and record its exact runnable command; no such current #920 test target was established by this lane.

## Necessary regressions, evidence gaps and next steps

1. **Runtime preparation/persistence:** on synthetic temporary stores, pause slow foreign-ID preparation and prove parent snapshots plus at least two child actors commit before resume; cover initial/followup/recovery, not merely eventual completion. Preserve final admission budget, cancellation during FIFO/BEGIN busy, transient recovery, budget exhaustion with no partial writes, and nontransient/nested errors without retry. Check authority/input changes fail closed, ordinary history conflict recomputes without duplicate assistant, and late cross-thread tool identity cannot overwrite/execute. Retain atomic checkpoints/redaction and unknown-commit/no-side-effect replay semantics. Upstream records 12/12 on its historical development SHA, not this combined dirty tree. Its FIFO→engine-busy combination, repeated-conflict exhaustion and owner-change dynamic evidence were explicitly missing; do not mark those as covered. No new performance benchmark campaign is required for this NSH integration map.
2. **#916 policy:** retain and rerun the existing **26** tests in `crates/vega_ui/src/conversation_stream/shell_recovery/tests.rs` with the exact filter above. They cover draft visibility/summary, route/authority rejection, unsubmitted Prepare dismissal, submitted Resume continuity, stale identities, safe keyboard policy and UI-only reload. Prior 26/26 is parent-reported, not rerun here and not mounted evidence.
3. **Mounted recovery:** retain `draft_summary_opens_separate_overlay_with_real_focus` and `pending_draft_close_escape_and_authority_keep_recovery_fenced` in the existing two-case runner. Require real ConversationStream rendering, summary/overlay/inline geometry, focus/reveal/AX eligibility, actual close/Escape, real service confirmations, late-confirmation rejection and blocker release only by explicit matching completion. Current status: first case interrupted during initial `open_window` effect flushing; second not reached. The existing sample identifies the composer observer notification loop, not a recovery-service/SQLite failure. Keep assertions and 2s/512-tick/10s budgets unchanged; that repair belongs to the composer author, not #920 or this document.
4. **Parent order:** preserve dirty work → recheck identities → integrate the exact #920 runtime/store delta and its upstream docs under parent ownership → finish separately owned composer/source implementation → run coordinated current-content checks/regressions and record actual counts, commands, toolchain and gaps. Only after all implementation is complete move to consolidated independent review, development-test archival/removal and final-head required CI. No intermediate review gate is introduced. Native/three-platform/user acceptance remains separate and unverified here.

This lane's validation is read-only commit/diff/source comparison, SHA-256/blob identity checks and document static checks only. No runtime change, build/test pass, CI status, merge or acceptance is claimed.

# Delegation / background writer migration

## Prior stage contract and frozen scope (historical)

The statements in this section and the former gap/verification sections describe the preceding stage, not the newly authorized store implementation below.

- User-provided base/head `555b0c0b3c00031ad22b69c7a5622e3c1dc4116b`, branch previously recorded as `feat/919-database-writer`. This continuation did not run Git or independently re-read HEAD; content identity below identifies the actual scope bytes.
- Shared worktree `/Users/puzige/Workspace/worktrees/vega-919-database-writer` has concurrent writers. Existing diff preserved. No store/shared-writer/pipeline/events/permission-contract implementation/Git/index changes, fetch/rebase, commits or pushes by this stage. No Cargo, global fmt, real application data, machine configuration or installation operations.
- Continuation writes: only seven Rust files listed below plus this document. Previous thirteen-file implementation is retained, extending the delivered inventory to fifteen Rust files with lifecycle ownership and diagnostics.
- One whole-repository migration delivery, not permission to merge a partial migration. Implementation, static verification, behavioral verification and acceptance remain distinct.

## Retained implementation

Originally 23 sealed owned command implementations; this continuation adds three, for 26 in the fifteen-file inventory. Ordinary commands use `crate::database_writer::commit(path, identity, command)`; Drop quarantine uses the shared pre-reserved `NecessaryLease`. No Fn command payloads or new queue/service.

All Rust paths below are relative to `crates/vega_conversation/src/`.

| Area | Delivered files | Commands / behavior |
| --- | --- | --- |
| Delegation | `agent/subagents.rs`, `agent/subagents/writer.rs`, `agent/subagents/stage_recovery.rs`, `agent/subagents/precompute.rs`, `subagents.rs`, `agent/subagents/lifecycle_ownership.rs` | create/followup/resume, cancel, checkpoint, terminal+usage completion+final checkpoint, normal release, necessary quarantine, precompute cleanup, abandoned-child recovery |
| Compaction | `agent/compaction.rs`, `agent/compaction/writer.rs`, `agent/compaction/consume.rs`, `agent/compaction/micro.rs` | final install+precompute retirement, ready consumption, micro install, settings, usage and status |
| Background precompute | `agent/compaction/precompute.rs`, `agent/compaction/precompute/writer.rs`, `agent/diagnostics.rs` | generation admission, stage usage+delegated attribution+automation accounting, finish, publish, retire, atomic diagnostic batch |
| Shell | `agent/shell_operations.rs`, `shell_recovery.rs` | resource receipt, durable facts, generation recovery, cleanup observations, pending-commit reconciliation, user-risk resume |

- File/artifact verification, credentials, provider construction/calls, runtime confirmation and workspace invalidation stay outside commands. Child settings carry reduced model/authority/automation identity, not credentials, providers or callbacks.
- Registry registration/update/finish ordering stays at callers; normal release waits for the actual SQL receipt before registry finish/disarm. Superseded precompute cancellation runs after the SQL receipt, outside the actor.
- Precompute retains admission through its background lifetime. Begin/publish compare captured authority against the current SQL-only fence; publish rechecks through its transaction guard. Necessary accounting/retire remain independent of cancellation. Usage callbacks and atomic counters stay after receipts outside commands.

## Continuation: three reported gaps implemented

| Modified file | Actual change |
| --- | --- |
| `agent/subagents.rs` | Opens an existing database, captures identity, reserves the shared one-shot necessary-terminal lease and verifies current database identity before `DelegatedOwnership::acquire`, thread admission and registry insertion. Transfers the reservation into lifecycle ownership without moving registry registration or runtime host timing. |
| `agent/subagents/lifecycle_ownership.rs` | Removes Drop-path SQLite open/write. `quarantine_runtime` retains the existing process-local admission and owner quarantine, cancels execution and marks registry isolation; then submits owned `QuarantineChild` without waiting. The command carries an owner clone, admission, captured database identity and previous execution fallback; verifies owner/admission, selects the new or previous execution, and commits `set_release` in one short transaction. Submit failure logs `necessary_terminal_not_confirmed`; accepted-command failure is recorded by the shared NecessaryRequest even when the receiver is dropped. Enqueue is not called durable. Existing memory isolation is deliberately retained on success or failure, preserving the old quarantine contract; it is not a new unbounded command queue. `disarm_unstarted` now verifies the captured identity before treating an execution as absent. |
| `agent/subagents/writer.rs` | Reuses the now-present `permission_contracts::capture_delegated_fence_for_commit` and deletes the duplicated local helper. Inspected the shared implementation: the committing branch uses SQL-derived parent/child workspaces, not `existing_task_workspace_root`. Existing source/resume/budget checks remain. |
| `shell_recovery.rs` | Adds owned `ResumeWrites`. Caller snapshots project path before filesystem workspace validation, then submits with no permission coordination guard or transaction held. Command acquires permission coordination and a short transaction; rechecks authority epoch/reset, persisted thread project/revision/mode/permission mode and original row validity, or draft absence/native/active/nonfork/ULID/revision/epoch predicates; compares current SQL project path; invokes the supplied `resume_writes_in_transaction` with the expected recovery version, operations and stable decision identity, then commits. Runtime invalidation/release remain after receipt. Unknown result checks the stable decision on the identity-verified connection before releasing runtime state; it does not replay external actions. |
| `agent/diagnostics.rs` | Replaces background SQL flush with `take_background_compaction_events() -> Result<Vec<NewDiagnosticEvent>, DiagnosticError>`. Mutex protects only taking the bounded sink; the returned owned events no longer retain the guard. The unrelated buffered `flush_to` interface was not changed. |
| `agent/compaction/precompute.rs` | Takes the owned diagnostic batch before invoking the writer; neither the diagnostics service nor mutex is passed to the actor. Failure remains explicitly logged. |
| `agent/compaction/precompute/writer.rs` | Adds `RecordDiagnostics` with admission/thread/generation and owned events, verifies batch identities, inserts the complete batch inside one Diagnostics WriteTransaction and returns the actual commit result. Existing `NewDiagnosticEvent` DTO suffices; no store DTO/API change was needed. |

## Prior integration constraint (historical; implemented by the current stage)

The three original API gaps above are no longer missing and their direct online bypasses are removed. Static call-chain inspection found an additional **unresolved actor-boundary constraint**; no safety assertion was deleted to hide it:

- `vega_store/src/context_compaction.rs::load_source_version_in_transaction` calls `load_source_inner(..., false)`. Despite the metadata-only description, `load_source_inner` calls `load_messages` and `load_tool_calls`, materializes their content/input/output vectors and computes the fingerprint. The flag avoids image materialization, not full text history construction.
- Remaining delegation call sites: `agent/subagents/writer.rs::CreateChild` continuation/source check (around line 260), `FinishChild` returned continuation boundary (around line 137), and `agent/subagents/stage_recovery.rs::verify_resume_in_transaction` (around line 216). `stage_recovery::CheckpointStage` calls `delegated_stages::append` (around line 442), whose store implementation also reconstructs this source. Terminal completion additionally calls `delegated_stages::final_checkpoint`, which calls `append`. Original resume source version/fingerprint, permission source, checkpoint, release, pending-input and explicit reconciliation checks are retained.
- Do not replace the fingerprint check with max sequence, omit resume validation, move a borrowed transaction across threads, or permanently exempt these calls. A scope-authorized store change is needed to prepare source/fingerprint and checkpoint/effect projections outside the actor and validate a compact **owned** database/schema/content-mutation fence in the final transaction. Suggested APIs: `prepare_delegated_source(...) -> OwnedDelegatedSource` with `validate_in_transaction(tx)` preserving same-sequence content edits and external-process conflicts; `append_prepared_in_transaction(tx, owned_checkpoint)` and a terminal equivalent. A snapshot of `data_version` or a connection pointer alone is insufficient across actor/read connections. Main agent owns the store design and integration; this stage has no authorization to change those store modules.
- Existing compaction store install/consume helpers also need main-agent review for transitive source reconstruction; this continuation did not broaden into their implementation.
- `history::with_restart_recovery` parent-thread writes and unrelated buffered diagnostics `flush_to` remain the owning main-agent scopes, not newly granted bypasses.

This is an integration blocker for the strict no-history-construction actor acceptance, not a missing NecessaryLease/shell API and not a reason to discard their completed migration.

## Owned source validation continuation (current authorization)

The current stage may modify the necessary delegated/source/checkpoint and compaction store modules. Full source loading, coverage/projection validation, recovery source projection and effect preparation happen before enqueue. Owned preparation is bound to database identity, exact schema definitions and monotonic per-thread mutation revision; final short transactions compare that fence before any command mutations, retaining owner/admission/authority/reconciliation checks. Same-sequence edits and external-process SQL writes invalidate prepared work. Additional revision triggers for continuation/read evidence/permission snapshots and precompute accounting are supplied as reserved migration 0065 SQL under docs; the main agent registers it. No migration registry changes in this stage. Startup APIs remain distinct from online prepared APIs; no online exception is introduced.

## Prior verification and content identity (historical)

Current-cycle static validation at 2026-11-09T08:12Z:

- Tool: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`.
- Formatting mutation: explicit seven-file `rustfmt --edition 2024 --config skip_children=true <the seven continuation files above>`; exit 0. No child-module/global formatting.
- Validation: explicit fifteen-file `rustfmt --check --edition 2024 --config skip_children=true <all Rust paths in delivered inventory>`; exit 0, elapsed 0.227 s.
- Read-only Python check over the same fifteen files: trailing whitespace, missing final newline and merge-conflict markers: PASS. No Git command used in this continuation; previous-stage `git diff --check` is historical evidence only.
- Scope SHA-256 `3dd0084df7abc180f9e08538992e36e3972bf990631c8ec2207a7b0f10417c65`. Hash over sorted full project-relative Rust paths, each UTF-8 path + NUL + big-endian u64 byte length + bytes; excludes this document and other agents' files.
- Continuation file SHA-256 values:
  - `agent/subagents.rs`: `8ea7fa8db38ded54775a83666b5b22d15c697517f0d3b8c16d059c79330e9d75`
  - `agent/subagents/lifecycle_ownership.rs`: `71ca9d5e26af10707af4a5a659e3c754cd11129e918e5c15747cbce7d93e7756`
  - `agent/subagents/writer.rs`: `e86ebc5d69c389e6253915fe648adb6984c28eff14734fb91d14e117d926c60d`
  - `shell_recovery.rs`: `e7b80b36627e9b2cc6382af962e7bf824263b9a6cbdd264ad54fd66f01708b08`
  - `agent/diagnostics.rs`: `bc70ce3e56bd82e36f419a925a8643198d5526d416219d2cbe93075a5f09c29d`
  - `agent/compaction/precompute.rs`: `2ab9f3ef70a768af70bf49072275a08f46bc12fa576ffabaa296765c2d485e65`
  - `agent/compaction/precompute/writer.rs`: `8a396fed90c88c6aa70ba15119d4e926a1c0b858152dc03aa9e556a7f383d5c4`
- No Cargo, rustc compilation, tests, Clippy, CI or behavioral/concurrency checks run by this stage; main agent owns unified Cargo. No tests added. The user-reported historical 22 conversation + 1 store checks do not validate these bytes; the later identity-replacement regression was reported blocked by pipeline install type errors and is not claimed passed.
- Main-agent next checks: compile final integrated bytes; necessary-queue capacity/ordinary saturation/Drop latency/drain/identity replacement/submission and execution failure; retained owner isolation; previous-execution fallback; delayed permission revocation and lock inversion; persisted/draft shell expected-version and decision retry/unknown receipt atomicity; bounded diagnostics batch atomic failure and identity mismatch; checkpoint/resume/source and terminal-history constraints above; final independent review/CI/user gates.

Prior stage stopped writing after that evidence update. No full-migration, compile-success, durable-enqueue or acceptance claim.

## Current delivery: owned source/checkpoint/effect preparation

User-provided HEAD remains `555b0c0b3c00031ad22b69c7a5622e3c1dc4116b`; no Git command was used to independently verify it. Old uncommitted changes are retained. This stage writes 18 Rust files below, this document and reserved migration SQL `docs/919/0065_source_validation.sql`. No shared writer, pipeline/install, preparation, privacy, permission-contract implementation, store shell operations, migration registry, Git/index or real application database changes. No Cargo/rustc compilation, tests, Clippy, CI, global fmt or installation operations.

### Actual implementation

- `context_compaction/source_version.rs`: owned `SourceFence` / `PreparedSourceVersion`. Capture verifies exact `preparation_revisions` table and 0061/0065 trigger definitions, database identity, schema version and per-thread monotonic mutation revision in the same read snapshot as the full source fingerprint. Final transactions compare those compact values and exact schema, not connection pointers, maxseq or data_version. Different threads do not share a revision. Same-sequence edits and ordinary external-process SQL mutations invalidate preparation. No history vectors are loaded by fence validation.
- `delegated_stages/checkpoints.rs` plus `delegated_stages.rs`: `PreparedStageCheckpoint`, `PreparedTerminal`, `PreparedAbandonedRecovery`. Source, unknown-effect projection, credential continuation source/raw-user checks and interrupted source projection are prepared outside actor. Final append uses the prepared source; effects are execution-bound and guarded by the same revision. Terminal recovery retains owner validation, shell recovery, messages/tools normalization and credential continuation decisions in the final atomic transaction. The recovery timestamp is captured once and shared by projection and SQL. Pending approvals become rejected (not unknown execution), approved/running become cancelled. Abandoned recovery retains quarantine rejection, register/release decisions and final interrupted checkpoint.
- `agent/subagents.rs`, `agent/subagents/writer.rs`, `agent/subagents/stage_recovery.rs`, `subagents.rs`: prepare before enqueue, final compact fence before command mutations. Followup/resume still compare the full prepared fingerprint against the prior continuation; resume retains permission fence, checkpoint/source, release, pending input, effect resolution and execution checks. Checkpoint file/credential checks remain outside actor. No registry registration/finish timing change. Existing NecessaryLease quarantine is unchanged and performs no source/history construction.
- `context_compaction.rs`, `context_compaction/prepared_checkpoint.rs`: coverage/UTF-8/tool-response/projection validation extracted without dropping assertions. `PreparedContextCheckpoint` performs it against full source before enqueue, then final transaction retains compact fence, current model, incomplete status, previous checkpoint CAS, duplicate/idempotent request checks, insert and precompute retirement/cancellation boundaries. No full source in the command.
- `context_micro.rs`: `PreparedMicro` owns selected call digests and projected installed fingerprint, not history. Final transaction validates fence/model/previous checkpoint, inserts the prepared digests atomically and returns the prepared resulting fingerprint.
- `context_precompute/ready.rs`: `PreparedPublication` performs existing candidate prefix/current projection and dependency checks outside actor; publication retains candidate/age/fence/finished accounting/current generation/authority checks and atomic write.
- `context_precompute/consume.rs`: existing candidate/current source/projection preparation remains outside actor; `ConsumptionCommit` drops full source before enqueue and carries owned candidate/encoded payload/fence. Final validation compares ready payload, generation, retirement, age, model and finished accounting under fence; `install_prepared_with_guard` installs the prepared checkpoint plus retirement in one transaction. Old non-actor store facades remain available but none of the migrated actor callers invoke full-history helpers.
- `agent/compaction.rs`, `agent/compaction/{writer,consume,micro,precompute}.rs`, `agent/compaction/precompute/writer.rs`: all four online install/consume/micro/publish paths call the new prepared interfaces. Commands carry no arbitrary closure payload, model callback or source history; tiny cancellation/authority guards remain local implementation details.

### Migration registration required

Main agent must register reserved **0065** using the exact `docs/919/0065_source_validation.sql` bytes (or preserve these bytes while relocating and update the include path coherently). This stage intentionally does not create a numbered migrations file or edit registration. Source validation currently includes that docs SQL as its exact schema contract; it fails closed if triggers are absent/different. No runtime auto-install or fallback bypass. The 27 BEFORE INSERT/UPDATE/DELETE triggers extend 0061 per-thread revisions to credential continuations, read receipts, delegated/run permission fences, precompute accounting/stages/slots/ready and token usage. They cover old/new ownership, rowid and unique-key REPLACE victims without relying on recursive delete triggers. Conservative same-thread accounting changes invalidate pending preparation; unrelated-thread changes do not.

### Current static verification (2026-11-09T09:16:09Z)

- `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`; explicit 18-file `rustfmt --check --edition 2024 --config skip_children=true`: exit 0. Formatting was local to those files, not global/child-module fmt.
- Python read-only hygiene over 18 Rust files plus migration SQL: final newline, trailing whitespace, conflict markers: PASS.
- Python SQLite **3.54.1**, isolated `:memory:` connection only: all existing migration SQL plus reserved 0065 executes; exact 0061/0065 trigger definitions equal SQLite schema text. SQL probes passed: same-seq content mutation advances owner revision; other thread revision unchanged; cross-owner `INSERT OR REPLACE` advances both victim/new owner; credential continuation mutation advances owner; revision rollback and delete rejected. These are SQL/schema probes, not Rust tests or actor/concurrency regression results.
- Static actor caller scan: delegated/compaction writer modules have no `load_source`, full `ContextSource`/`PreparedConsumption` payload, old recovery helper, old install/publish facade or candidate `validate_current` call. Stage resume direct source loading is only in the outside-actor preflight; its commit helper receives owned prepared source values.
- No Cargo, Rust tests, Clippy or CI executed. Historical 26 regressions and earlier results do **not** validate these new bytes. No Rust tests added.

### Current file identity

SHA-256 aggregate `dad510736a49cc23c1f96cab938166dd6b79da194469b9b950929b67bb891699`, over sorted project-relative paths + NUL + big-endian u64 byte length + file bytes for the following 19 implementation/SQL files; this document excluded.

| Project-relative path | SHA-256 |
| --- | --- |
| `crates/vega_conversation/src/agent/compaction.rs` | `84252d396bd749293948f6071bb4bd186f5afdf14eb088ef0838902f94d8ab50` |
| `crates/vega_conversation/src/agent/compaction/consume.rs` | `8440ae16e75aafbc57af146b7c13040223a03a8cea009cbf9a64ac2d066d67af` |
| `crates/vega_conversation/src/agent/compaction/micro.rs` | `3a308cfdc33f45316f96127072dd08d26ba50b0ee4efbaa030a1aa24f09f60df` |
| `crates/vega_conversation/src/agent/compaction/precompute.rs` | `9e0aea366035074ab458757f974d8d6eb482447cbde3eabf82f4a332eb316894` |
| `crates/vega_conversation/src/agent/compaction/precompute/writer.rs` | `a04694704db7d9454ee2d30ba13a537d9a0a5e1b3699d878e147640b2f65d0b6` |
| `crates/vega_conversation/src/agent/compaction/writer.rs` | `0c44346f08a39580e97562dff25c617118da1b52931805b9afaefc6713b27587` |
| `crates/vega_conversation/src/agent/subagents.rs` | `9ca239c80ed16f5d3a58db4afa0d6a507f8653da94af82d03d87913a5141be25` |
| `crates/vega_conversation/src/agent/subagents/stage_recovery.rs` | `eec29679da0d7685d03e0a902680c2ce408acf57b45ea659fe5f071aee38809f` |
| `crates/vega_conversation/src/agent/subagents/writer.rs` | `1cf1effadc08492f9a4a2e12e330e10be86ffdc93d48f4773fec76685eb0c9ce` |
| `crates/vega_conversation/src/subagents.rs` | `3ae01cdd491e81ffdac3755cff58da8deda3082aceb2dda46d87b2e53f3402fd` |
| `crates/vega_store/src/context_compaction.rs` | `0407606bf67a5e8472822979fe2b451249cb576f0622de34f418814cffd8875d` |
| `crates/vega_store/src/context_compaction/prepared_checkpoint.rs` | `2222c038add03dd2579eb1d4d5ca45c9aa26021c2cd4e99e8d15005a7050c119` |
| `crates/vega_store/src/context_compaction/source_version.rs` | `010bf4f42269ab4e9a4c4f8f3db2ac242da19ae094288e0901f23310a4c26264` |
| `crates/vega_store/src/context_micro.rs` | `a448aa1ba89caf76ac629c07c204fc80aa29fc396ac09e6a96cbb841d99dd2cc` |
| `crates/vega_store/src/context_precompute/consume.rs` | `45e5d2fa10339be2ddbab7de9d0885cb87dc4cbc212e3b97569165d8b665c901` |
| `crates/vega_store/src/context_precompute/ready.rs` | `0592e95c8d49b493284c3205f7c2d9b18226782f690a98d69b00a36efc6c8447` |
| `crates/vega_store/src/delegated_stages.rs` | `cdac1763e27becebfac116c604b71b2abd7f6ee59e1bb60ae6abaa51b7c9503b` |
| `crates/vega_store/src/delegated_stages/checkpoints.rs` | `3effeaa89002e9b64235d94aeba1411902fff4b94100038d88d7d322199c47fc` |
| `docs/919/0065_source_validation.sql` | `3534ae18be93bb634286e06ad168f4e189f90f2a9d50dc21a5d78105dfba760a` |

### Real remaining validation / integration risks

1. Register 0065 before final unified Cargo and online use. Compilation/type checking and all Rust behavioral results remain unverified here by user direction; no missing private API is being handed back as implementation work.
2. Regression needed: pause after preparation, same-seq edits to message/tool/historical delivery/image/response/micro rows, external second-connection writes, schema trigger removal/replacement, REPLACE victims, identity replacement. Expect stale/error with no partial checkpoint/recovery/retirement. Unrelated-thread SQL should continue without stale conflicts.
3. Compare `PreparedTerminal` interrupted fingerprint against actual post-normalization source for streaming messages, pending/approved/running calls, historical model content, images and micro markers. Credential claimed-continuation recovery must preserve unchanged/changed raw input, permission invalidation and unknown-effects decisions. Verify terminal receipt before registry finish and quarantine on failure.
4. Verify checkpoint/resume permission authority/release/evidence/reconciliation assertions, UTF-8 assistant cursor, response-group completeness, duplicate request/previous CAS, cancellation at lock acquisition/pre-insert/pre-commit; install+retire atomicity, candidate expiration/accounting change, borrow consumption, micro output fingerprint equality and precompute publication authority revocation.
5. Schema validation currently checks fixed trigger definitions on each compact validation; cost is schema-sized, not history-sized. Same-thread ancillary mutations conservatively cause stale rather than reprepare/replay automatically. Full callback/file/model/credentials work remains outside commands; existing filesystem-based database/owner identity checks are intentionally retained security checks.

Current stage stops writing after this evidence is verified. Implemented and statically checked, not compiled, merged, CI-validated or user-accepted. Whole-repository final merge remains the main agent's responsibility.

## Development behavior regression continuation

This section supersedes the earlier pending-migration/no-new-tests statements for this continuation only. Main agent installed and registered formal migration `crates/vega_store/migrations/0065_source_validation.sql` and changed SourceFence to include it. Both formal SQL and retained docs draft have SHA-256 `3534ae18be93bb634286e06ad168f4e189f90f2a9d50dc21a5d78105dfba760a`; this continuation did not edit either migration, registration, shared writer, pipeline, permission contracts, fork or privacy files. User-supplied HEAD remains `555b0c0b3c00031ad22b69c7a5622e3c1dc4116b` plus concurrent uncommitted integration; no Git command was executed to re-establish HEAD.

### Original expectations fixed before test implementation

- Prepared work must reject related same-sequence content/authority changes made through a second Store connection; unchanged other-thread writes must not invalidate a fence. Moving a unique-key victim with REPLACE must invalidate both owners even with recursive delete triggers disabled.
- Missing or replaced 0061/0065 triggers must fail closed, including when a writer restores the prior schema cookie. A max sequence or schema-cookie-only implementation is insufficient.
- Real prepare must reject nonterminal covered projection and invalid UTF-8 assistant cursor. Valid install/publication/consumption/micro requests must succeed; stale requests must not partially install, retire, or change usage/accounting. Cancellation observed after retirement must roll back checkpoint and retirement together.
- Delegated preparation must retain unknown effects, reject wrong child identity and post-prepare mutation, preserve interrupted recovery source fingerprints, and require native owner for child recovery. Resume must retain permission snapshots, release state, explicit matching reconciliation and successful evidence checks.
- A paused source preparation with a real WAL read transaction must not occupy the shared actor: an owned SQL command to the same database must receive its actual receipt before preparation is released; prepared work from the older snapshot must subsequently become stale.

### Actual incremental diff

Only five Rust files plus this document changed in this continuation; all preceding implementation diffs retained:

| Path | Increment |
| --- | --- |
| `crates/vega_store/src/context_compaction/prepared_tests.rs` | New development-only real Store fixture and 16 behavior tests. No mocks, source-string assertions, new dependency or fallback migration install. Uses `Store::migrate()` with formal migrations. |
| `crates/vega_store/src/context_compaction.rs` | Adds only `#[cfg(test)] mod prepared_tests;`. |
| `crates/vega_conversation/src/agent/subagents/stage_recovery/prepared_tests.rs` | New development-only real delegated permission/execution fixture, two resume tests and shared-actor source-prepare gate test; three tests total. |
| `crates/vega_conversation/src/agent/subagents/stage_recovery.rs` | Adds only development test module declaration. |
| `crates/vega_conversation/src/agent/compaction/precompute.rs` | Removes uncalled `StageAccounting::open`; no `allow(dead_code)`, accounting ordering or Agent registry ordering changes. |

Production SourceFence and prepared APIs were not weakened or redesigned. No product defect was established by the allowed static/SQL checks; Rust behavior execution is still required before claiming that conclusion at runtime. Retain `delegated_stages::final_checkpoint`/`append`: `vega_store/src/recovery.rs` still uses that startup recovery path. The online actor callers use prepared paths. Public legacy compaction facades were not blindly removed as dead private helpers. `ordinary_submissions::stage_permission_fence` remains main-agent scope.

### Added development tests and real API expectations

Store module filter: `context_compaction::prepared_tests` (16 tests):

1. `compact_fence_rejects_external_same_sequence_edits_without_rejecting_other_threads`: real `SourceFence`/`PreparedSourceVersion`, unrelated thread positive, wrong thread negative, external same-seq edit with unchanged source_version negative.
2. `compact_fence_tracks_transitive_source_and_authority_mutations`: tool output, assistant response, micro marker, permission mode and credential continuation writes invalidate the captured fence.
3. `replace_invalidates_both_displaced_and_incoming_owners_with_recursive_triggers_disabled`: message and credential-continuation REPLACE invalidate old/new owners.
4. `missing_or_replaced_schema_trigger_fails_closed_even_when_schema_cookie_is_restored`: removal and no-op replacement of both 0061 message and 0065 continuation update triggers reject capture and validation despite restoring schema_version.
5. `prepared_checkpoint_installs_and_retires_generation_atomically`: valid prepared install returns Applied and retired generation; checkpoint/ready/retired/usage/stage counts become `(1,0,1,1,1)`.
6. `stale_prepared_install_preserves_usage_accounting_and_ready_generation`: external same-seq edit returns Stale/None with complete durable row snapshots unchanged.
7. `cancellation_after_checkpoint_and_retirement_rolls_back_both`: cancellation guard observes actual transaction-local retired flag, then requires Cancelled and complete durable snapshot restoration.
8. `prepare_rejects_nonterminal_projection_and_invalid_utf8_partial_boundary`: real projection prepare rejects streaming coverage and a byte cursor inside `é` before installation.
9. `prepared_publication_and_consumption_commit_with_priced_usage`: valid publish, real `PreparedConsumption`, priced usage, prepare_install, compact commit projection after dropping full source, then Applied with atomic retirement.
10. `prepared_publication_rejects_source_or_accounting_changes_and_revoked_permission`: source/usage/finished-accounting mutation causes Stale; final authority guard revocation causes Cancelled and no publication.
11. `prepared_consumption_rejects_source_or_usage_changes_without_half_commit`: stale compact consumption and prepared install preserve all durable rows, including usage values and accounting payloads, not only row counts.
12. `prepared_micro_installs_expected_fingerprint_and_rejects_later_tool_edit`: actual installed source fingerprint equals prepared output; external tool edit yields Stale with zero micro rows.
13. `delegated_checkpoint_preserves_unknown_effects_and_rejects_wrong_or_changed_source`: running Bash effect survives manifest append, wrong owner thread and later tool mutation cannot append another checkpoint.
14. `prepared_terminal_recovery_matches_normalized_source_and_keeps_unknown_effects`: streaming/running becomes interrupted/cancelled, prepared fingerprint equals actual normalized source, original running unknown effect remains in interrupted checkpoint.
15. `prepared_terminal_and_abandoned_recovery_reject_post_prepare_mutations`: terminal validation and abandoned install reject changed source without normalization or final checkpoint side effects.
16. `prepared_delegated_recovery_requires_matching_native_owner`: real file-lock-backed wrong/no native owner rejected; matching owner can recover a child.

Conversation module filter: `agent::subagents::stage_recovery::prepared_tests` (3 tests):

17. `prepared_resume_preserves_permission_source_release_and_explicit_effect_reconciliation`: valid outside preflight and final SQL resume check; missing/unmatched/duplicate reconciliations, nonexistent evidence and quarantined release rejected.
18. `prepared_resume_rejects_external_source_and_permission_snapshot_changes`: compact fence plus existing final resume check reject same-seq source or saved permission mutation; release remains unchanged.
19. `outside_source_preparation_keeps_shared_actor_available_and_prepared_commit_stale`: real shared `database_writer::commit` with sealed owned `TouchThread` SQL command; reader pauses with a snapshot for up to 10s, same-db command must return receipt within 3s before release, old prepared install must then return Stale. Timeout is an assertion failure, not a skipped case. No arbitrary closure command payload or shared writer modifications.

Failure paths compare all values in checkpoint, ready, slot, stage, usage and accounting rows before/after the attempted command. The SQL probe itself does not execute these Rust assertions.

### Executed checks and actual limits

- `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`. Explicit five-file local format followed by `rustfmt --check --edition 2024 --config skip_children=true` on the five files above: exit 0. No global or child-module formatting.
- Read-only Python content checks: 20-file retained Rust inventory plus new tests has final newlines, no trailing whitespace/conflict markers; two test files have 19 unique, nonignored tests, no comments. `StageAccounting::open`/`self.open()` scan has no remaining definition/call in its module.
- Isolated SQLite 3.54.1 `:memory:` SQL fixture probe: all 65 formal migrations executed with foreign keys enabled; 21 extracted store mutation SQL statements accepted; six extracted conversation fixture statements and delegated relationship preconditions accepted. This is SQL fixture checking, not Rust execution or a model of expected API outcomes.
- The first auxiliary conversation extraction probe failed with Python `KeyError: id` because it omitted the Rust fixture's parent/child loop mapping. Corrected the diagnostic mapping, then reran successfully. No product/test assertion was changed for that diagnostic failure.
- Exact schema probe compares all 93 trigger definitions in 0061/0065. 100 Python in-memory repetitions averaged approximately 0.973ms per 93-trigger scan on this machine; this excludes Rust, actor queueing, disk and full transaction cost, so it is not an actor benchmark. The current implementation remains unchanged. If central measurement shows material latency, the smallest candidate is a single filtered sqlite_schema result scan with the same exact name/count/SQL comparison and preparation_revisions table comparison; do not replace exact schema validation with schema_version/data_version alone.
- **No Cargo, Rust tests, Clippy, CI, Git/index, commit/push/Issue operations or real user database access.** No claim of 19 passing tests. User-reported prior conversation 40/40 and older 26-test results do not validate these new bytes. Toolchain compilation/type checking remains unverified here.

### Main-agent validation and final cleanup

Central commands to run (not executed by this continuation):

```sh
CARGO_FORK_AUTO_UPDATE=0 cargo test -p vega_store --lib context_compaction::prepared_tests --locked --offline -j 2
CARGO_FORK_AUTO_UPDATE=0 cargo test -p vega_conversation --lib agent::subagents::stage_recovery::prepared_tests --locked --offline -j 2
```

Then run the task-required integration/regression commands and retain actual logs/toolchain/content identities. If a fixture/type failure appears, repair that responsible layer without weakening behavioral assertions. Required CI, independent review and user acceptance remain separate. These are exclusively development-stage Rust tests: archive exact two test sources/patch and actual results, remove both test files and the two test module declarations before final master integration, then verify final-head zero Rust-test discovery and required CI. Retain the production `StageAccounting::open` removal. No permanent online bypass and no partial merge.

Remaining coverage limits: this selection does not exercise every historical delivery/image/claimed-credential-continuation recovery variant, complete assistant-response-group cursor matrix, all cancellation phases, borrow consumption, final owner-to-registry receipt lifecycle or database file replacement. Relevant previous regressions may supplement only after main-agent applicability/current-head verification. The new gate tests shared actor availability with a real source snapshot and owned command, not an end-to-end model provider or UI lifecycle. No implementation issue was intentionally left as a missing private API.

### Current exact content identity

Aggregate SHA-256 over the retained 18 Rust paths from the preceding identity table plus the two new test paths (20 files total), sorted path + NUL + big-endian u64 byte length + bytes:

`59145723c594e13d2ed57164ac8777daab9d029d6b67d2cf037bc7370c79716a`

Unchanged entries retain their preceding hashes except `source_version.rs`, whose main-agent include path integration is observed as `c6b0f7a65a2e95004191332f6697eb9e3e663a135aee20dc469a37efc96e917f`. Exact five-file increment identities:

| File | SHA-256 |
| --- | --- |
| `crates/vega_store/src/context_compaction.rs` | `a86e13e6560456820f13a50ced75c3b05fe19340d6c129b3fbdfaf1a1e35b274` |
| `crates/vega_store/src/context_compaction/prepared_tests.rs` | `74799bd4336e9298ed0252ea59648c27aa3857914c7a1318b6f947c1e9c3992a` |
| `crates/vega_conversation/src/agent/subagents/stage_recovery.rs` | `3dccd3203556590473f640d48f120f404a3c3e7bba834714b656c58645214d01` |
| `crates/vega_conversation/src/agent/subagents/stage_recovery/prepared_tests.rs` | `2ce9b87d799352258159639b152a2cca951a465d6cbe2bacc0126f57c3cdf4e6` |
| `crates/vega_conversation/src/agent/compaction/precompute.rs` | `2d50695b75c6f6e8249fc6c072dba4fbc7ec20834a6aaddfcd4e3b1be060c3bd` |

Stop writing after final static/identity verification; hand back the scope for central execution and development-test cleanup. Implemented tests are not executed results, and this is not the final whole-repository merge.

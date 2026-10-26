# Reset — initial service and conservative completion gate

This D4 increment connects the existing pause, journal, typed automation revocation and acknowledged memory invalidation. It is not the app-wide reset action, active Stop coordinator, native cleanup implementation or permission to downgrade. The full D4 acceptance contract remains unchanged.

## Service contract

- Prepare an opaque reset intent under short authorization coordination, capturing the exact database identity, ready epoch, canonical existing configuration path and raw default permission fields. Do not create configuration, pause work or mutate authority merely to prepare a confirmation. A live database pause or durable pending reset refuses a new intent.
- After explicit user confirmation, the future app coordinator retains its UI/scheduler gate and obtains the exact database admission pause. It requests Stop and waits without holding authorization coordination. The service accepts only that live, drained pause and the original intent; a different database, epoch, configuration source or permission revision refuses before journal/configuration mutation. Unrelated current preferences are preserved.
- The first transaction checks persisted unsettled work and atomically begins the existing reset journal/epoch rotation/durable invalidation together with typed automation revocation. Overflow, malformed automation and transaction failure must not publish a partial reset. An ambiguous commit returns outcome unknown, never completion.
- With pending durable, acknowledge ordinary-submission and replay invalidation before configuration publication or a completed receipt. Failure retains the same pending journal and revoked authority. Retrying an existing pending reset with the held pause does not prepare a new intent or rotate the epoch. Startup recovery uses the same ordinary-state check and invalidation; it has no replay registry because normal controllers are not initialized until recovery succeeds.
- Preserve coordination → ConfigEdit → database transaction ordering. Reuse the source-or-exact-target configuration protocol and final journal readback. Release coordination after the synchronous service call; do not wait for workers inside it. The caller owns the pause throughout success/failure and must keep it until UI/scheduler state is reconciled. Dropping a pause cannot clear durable pending.

## Evidence required before completion

Owner release, actor closure, pipeline cleanup, successful tool output and cancelled history are different facts. None alone certifies external-process cleanup.

- Inspect all retained ordinary attempts for the exact database, including retired/permission-invalidated submissions. Active owners/workers refuse with Busy; unsealed, abnormal, ambiguous commit/primary or incomplete/failed/unknown cleanup refuses with UnsettledWork. Preserve every fact and input; invalidation cannot erase a blocker. History handoff can retire its presentation, but must retain the registry evidence if any attempt still fails this check; a fully settled handoff keeps the existing removal behavior. A normal completed pipeline may satisfy this in-process check, but still needs the independent persisted-effects gate.
- Keep the existing streaming, unfinished automation, active/quarantined delegation, credential unknown/claimed and scope-unknown blockers. Do not invoke generic history/automation recovery to make this check pass.
- Until trustworthy typed executor cleanup receipts exist, a persisted successful external/unknown tool call also blocks reset. In particular, successful Bash may leave detached descendants; successful MCP/ACP/desktop calls do not supply general external cleanup receipts. The narrow known-local completed set is read/glob/grep, ask_user_question/checkpoint_stage and synchronous write/edit. Existing failed/cancelled/approved/running effect-capable calls remain blockers. Pending/rejected calls that were never executed do not themselves prove an effect occurred.
- This conservative stage may refuse reset for otherwise finished historical external work. There is no user override, status rewrite, evidence deletion or assumption that exit code zero means native cleanup. Durable typed cross-executor cleanup and a truthful recovery path are still required for full D4; this stage does not reduce that obligation.

## Development verification

Use real temporary-file configuration, Store transactions, service decisions and ordinary worker/receipt APIs. No local Vega execution, subprocess/network experiments or actual user configuration/credentials. Required regressions:

1. A confirmed unchanged-source intent plus drained exact pause completes a quiescent reset; thread/default/automation grants are cleared, epoch/revisions change once, unrelated preferences/input/history survive, old ordinary retry is revoked, same pause stays held and a new post-release submission is possible.
2. Wrong/live-owner pause, competing permission edits/epoch/database, missing or malformed configuration refuse without journal/config mutation. A prepared intent cannot be replayed after success.
3. Initial automation failure rolls back the journal/epoch and preserves config. Pending retry keeps reset identity/epoch and uses the existing source-or-target recovery protocol.
4. Lost/unknown ordinary attempts block both initial reset and pending recovery even if admission is drained or the submission is already invalidated. Other databases do not block this database.
5. Successful Bash/unknown external calls and existing unknown effects block both initial reset and startup recovery without normalizing history; a successful local write and a rejected Bash call are positive controls, not a blanket prohibition on all history.

6. History handoff preserves pending/failed/unknown cleanup as a reset blocker even after permission invalidation; genuinely successful cleanup permits the existing registry removal. Replay-registry poison after durable pending refuses both initial completion and retry, preserves original configuration bytes and does not rotate the journal again.

## Implementation status

The service, ordinary-state gate, retained incomplete history-handoff evidence and conservative external-success predicate are implemented. Nine new development cases cover the above boundaries: seven initial-service cases, one ordinary history-handoff case and one replay-unavailable service case. The ordinary fixture shares its original real commit/receipt setup; previous assertions remain. Local pinned-toolchain formatting and diff-whitespace checks passed. Fresh exact-head cloud compilation/execution and independent non-writer delta review are pending; no native process or UI acceptance is claimed. No schema, dependency or workflow changes are included. This remains a service increment, not a complete reset coordinator.

Keep existing barrier, recovery, updater, startup and real GPUI navigation regressions. Run development tests only through existing Actions, obtain a non-writer delta review, preserve full source SHAs and actual cloud results, then remove all Rust development tests and exclusive support before eventual merge. Final required CI must explicitly report no Rust tests (0 discovered / 0 executed).

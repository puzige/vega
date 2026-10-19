# Subagent startup receipts and safe runtime diagnostics

Issue: [#487](https://github.com/puzige/vega/issues/487). Baseline:
`ae35310fe7e1ddac2b79d7ec7bc55bcca2d6af2a` (v0.1.170).
Status: acceptance frozen v1.1, 2026-10-19 (Asia/Shanghai); causal reproduction
and effective RED await Actions. Production implementation is not authorized
until those results are reviewed.

## User request and evidence

The user authorizes continued diagnosis, a fix after root confirmation, and
specific error diagnostics. Codex coordinates one dedicated implementation
executor and owns review, Git, PR and CI. This request authorizes continuing
the same issue through implementation, rather than stopping after a handoff.

The reported run received a successful HTTP 200 primary-model tool response
at 10:17:20.333. A spawn tool entered its lifecycle at 10:17:20.356; its child
user row was created at .368 and assistant row at .380. The child run started
at 10:17:22.441 and was cancelled after 5 ms, without a provider request or
tool execution. The parent failed at .453 with only `runtime`, `unknown` and
`unknown_safe_failure`. The spawn receipt was not persisted. Recovery at
10:20:22 belonged to a later user continuation and is not the original error.

The old diagnostic cannot recover an unrecorded original error. Do not claim
that these historical rows prove a particular SQLite error code. The narrow
source-supported mechanism to confirm in Actions is concurrent child
preparation and parent receipt persistence: the lifecycle replies before child
preparation; preparation owns an immediate transaction while loading credentials
and history; the parent independently persists the spawn result. Critical
persistence errors cancel the parent and its children. The two-second timing
matches the busy timeout but is only supporting evidence until reproduced.

## Acceptance before implementation

All automated entries use fresh owned file databases and production conversation,
runtime, lifecycle and persistence functions. Substitute the external provider
and UI host at their existing boundaries only. No external processes, network,
application launch, restored historical suite, source-string tests, arbitrary
sleep collision, or mirrored persistence implementation.

| ID | State and operation | Observable result / evidence |
| --- | --- | --- |
| R1 | Gate the existing credential reader during real delegated preparation; while it waits, persist an actual parent's valid spawn terminal result using production persistence | Parent persistence succeeds while the external credential callback is pending, then child preparation succeeds after explicit release. On the unchanged baseline, capture the actual SQLite primary/extended error codes from the same real writer boundary before releasing the reader; do not decide the result from a timing estimate. This confirms the causal transaction defect without inventing the unrecoverable historical error code. |
| R2 | A real parent spawns a child; inspect storage when the child begins preparation/execution | Parent spawn success is durable first, with the same call, parent and child identity. Parent and child then complete normally with one child execution and valid stored results. |
| R3 | A real database trigger rejects the parent's spawn terminal update | Parent fails closed; child preparation/provider/tool side effects have not started. Delegation becomes terminal without orphan execution or duplicate result. The original SQLite classification survives the acknowledgement path. |
| R4 | Parent cancellation or an admission rejection occurs before the durable receipt | Cancellation drains the admitted child without hanging or starting a provider. An ordinary rejected admission/tool business failure remains a tool result that the parent can handle. |
| R5 | Inject a real strict-audit/state violation and a real SQLite failure with a sensitive sentinel in the error text | Live failure, failed tool/root diagnostic events, reopened history and JSON export retain the correct safe stage/reason and applicable SQLite numeric codes; no sentinel, path, SQL, tool input/output, provider body or credential reaches diagnostics or user errors. |
| R6 | Migrate an existing schema-27 database with legacy unknown diagnostics; reopen/export it, and validate invalid new detail values | Existing rows remain genuinely unknown, no retrospective lock diagnosis is invented, new nullable fields are validated, and unrelated data/migrations remain intact. |

Write all required cases and exclusive supports before implementation. An API
absence may be separately identified as interface RED, but behavioral root
confirmation must execute the relevant production functions. Test-side wiring
errors are not effective RED. A case that needs a new acceptance boundary
returns to Codex before implementation; it is not appended after the fix.

## Minimal implementation after reviewed RED

### Durable receipt ownership

Use conversation-private supervisor/registry state for a two-phase child launch.
Admission may reserve/create a child and update the UI. It cannot start the
child run's preparation, provider or tools until the parent's successful spawn
terminal receipt has passed the existing persistence boundary.

The runtime event processor releases only that same supervisor's call identity
after `PersistenceActor::event(ToolCallFinished Success)` succeeds, before
returning its ACK. The child waits on release versus its inherited cancellation
token; parent failure/stop closes the pending launch and drains it. Reused tool
receipts do not start another child. Admission rejection, stale call identity,
replayed results, ordinary tool failure and terminal delegation ownership retain
their existing semantics. Prefer this private observer over new public runtime
tools, polling, timing delays, a global task registry or a new scheduler.

Do not increase busy timeouts, retry mutations, swallow persistence errors,
relax approval validation, or disable cancellation. Do not broadly refactor
SQLite transactions or move credential security decisions. R1 specifically
requires reading the owner credential values before acquiring the preparation
write transaction: the callback can open other database connections and inspect
files, and cannot run while holding the exclusive writer. Keep legacy-content
redaction/validation in its original transaction and preserve the final provider
credential reread/guard. The receipt gate alone is insufficient: an already
launched child could otherwise block the parent's next tool proposal or another
spawn in the same batch.

### Structured safe runtime failures

Retain a typed, content-free failure at critical persistence/acknowledgement
boundaries. Details describe a closed stage and reason plus applicable numeric
SQLite primary/extended code or a closed IO category. Stages distinguish run
preparation, snapshot, tool proposal/approval/running/terminal, and event pipeline;
reasons distinguish database failure, strict approval audit, lifecycle/state
conflict, IO and pipeline/worker failure. Unknown errors stay unknown. Arbitrary
`Tool { message }` strings must not be parsed to manufacture a diagnosis.

The nullable JSON/storage keys are `runtime_stage`, `runtime_reason`,
`sqlite_code`, `sqlite_extended_code` and `io_kind`. The terminal tool examples
are stage `tool_terminal`, reason `database` with primary/extended codes
`19`/`1811`, or reason `approval_audit` with no SQLite fields. Other reasons
are `state`, `io` and `pipeline`; `run_preparation` is a separate stage.
The complete implementation uses closed enums/allowlists for supported stages
and IO categories. SQLite code pairs must be consistent and belong only to a
database reason; arbitrary strings and incompatible metadata are rejected.

Add nullable validated diagnostic detail fields through one new append-only
migration; do not change an old migration or overload token/budget/HTTP fields.
The existing failure-code/kind/scope SQL CHECK vocabularies remain compatible.
The detailed stage/reason and safe numeric codes are carried through runtime
metrics, conversation conversion, store insert/read/validation, history failure
ownership and JSON export. Old records have null details. Migration numbering
must be reconciled with the current base at integration, including other active
branches; do not modify their worktrees.

When a critical tool event cannot be persisted, close its diagnostic attempt as
failed with the same content-free reason and keep the parent failure truthful.
Do not fabricate a successful tool terminal row or replace the actual cause with
later cancellation/recovery. Recording diagnostic metadata is best effort on the
existing persistence connection and cannot block or reverse the run result.

The ordinary failed-run message should explain the relevant failure and next
action concisely, using the current UI's error surface. SQLite/code/stage details
belong in the existing diagnostics/export. Raw errors remain excluded from
display, Debug/export and provider-facing projections. Preserve current provider,
context, credential and cancellation classifications and legacy fallback.

### Scope and ownership

The dedicated executor owns only the relevant files in:

- `crates/vega_conversation/src/agent/` (supervisor, event/actor/entry boundaries,
  diagnostic conversion and focused new development tests/supports).
- `crates/vega_conversation/src/types/events.rs` and minimal history projection
  changes needed for the new failure detail.
- `crates/vega_runtime/src/error.rs`, diagnostic metric declarations and any
  narrowly required public exports; no unrelated loop/tool changes.
- `crates/vega_store/src/run_diagnostics.rs`, its failure-ownership reader,
  migration registration and one new migration.
- Test-only dependency wiring using already approved workspace dependencies,
  and `docs/vega-issue-487-subagent-startup-diagnostics-delivery.md`.

No change to sibling tasks, runtime databases, credentials, user configuration,
delegation strategy, application installation, dependency versions, CI workflows
or required-check policy. No new dependencies or code comments. Codex owns this
specification, acceptance changes, integration, commits and external updates.

## Verification and delivery

Local checks are limited to fast static/diff/format checks; no Vega execution,
compilation, Clippy or tests. Existing draft-PR Actions run workspace fmt,
Clippy, the preserved Python checks, and Nextest. Poll at five-minute intervals.

Record source baseline, complete test commit and file hashes, causal-reproduction
evidence and effective RED before implementation. Freeze cases/expectations,
then record the same-test GREEN with exact run/job/counts. Only after GREEN is
fixed in the PR, delete this issue's Rust tests and exclusive supports in an
independent commit, review production equivalence, and verify the latest head's
full gate and explicit zero discovered/executed Rust tests. Keep recoverable
test sources and RED/GREEN links in the PR after squash.

Only the verified final head can become a regular PR and merge. Keep the issue
open and branch/worktree retained at In review until user manual acceptance.
Rollback is a reviewed revert PR; additive nullable diagnostic columns may remain.

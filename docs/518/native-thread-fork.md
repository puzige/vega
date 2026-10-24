# Native project thread fork — specification review

## Stage and scope

- Issue: #518; parent: #498.
- Claim: `vega-fork-518-20261024T182917Z`.
- Inspected baseline: `5db92c12cfca933674164c9485c7836e6201912d`.
- Stage: specification review; **not implementation-ready**. No production changes.
- Data layer only: Native project chats, fresh identities, source unchanged, atomic prefix clone. No Fork UI, ACP replay, run start, application installation or runtime data mutation.
- Current repository policy supersedes the issue's historical mandatory TDD requirement. Local work is static only; development Rust tests, if used, execute in Actions and must be preserved as accessible evidence then removed before merge. Independent implementation review and final required CI remain mandatory.

## Confirmed current APIs and migration correction

- `vega_conversation::threads` owns public thread orchestration. Its `Thread` and `vega_store::threads::ThreadRow` need distinct fork provenance fields, never reuse `parent_thread_id`.
- `Store::immediate_transaction` returns a coordinated `WriteTransaction` dereferencing to a SQLite transaction. Eligibility, prefix reads and destination writes must share that snapshot; preflight reads alone are insufficient.
- `context_compaction::load_source_in_transaction` and `agent::history_from_context_source` provide existing context projection and terminal-tool validation seams. Validation must cover the selected prefix, not only its last message. Avoid loading/rejecting unrelated suffix history as a substitute for prefix validation.
- Migration `0031` in the issue is occupied by sidebar ordering. Latest migration is `0034`; the next candidate is `0035_thread_fork.sql`, subject to rechecking the integration head. Append registration in `vega_store/src/lib.rs`.
- Conversation code intentionally has no direct rusqlite dependency. Keep SQL cloning and its database error types inside the store layer.

## Stable acceptance inherited from the issue

1. New Native project thread is top-level and sidebar-visible, with its project workspace; no inherited subagent or workspace owner identity.
2. Record direct source thread/message provenance. Original thread and dependent records remain unchanged.
3. Copy only messages through the selected complete assistant boundary. Allocate a fresh thread and fresh message/tool call identities; regenerate chronological thread-local sequences.
4. Preserve content, plan history, images, tool rounds and trustworthy Final offsets. Consistent remapping of relational and recognized structured artifact references is required.
5. No new usage/cost, compacted checkpoint, queue/credential authority, delegation owner or execution is created. No agent run is triggered.
6. Failures leave no partial destination. Independent repeated successful forks have disjoint identities; this API does not promise request-id idempotency.
7. Codex/ACP and standalone sources return typed refusal, not replay or fallback.

## Required copy graph

| Table | Historical data treatment |
|---|---|
| threads | Fresh id; project/native binding; direct provenance; null parent/workspace owner. Explicit fresh lifecycle/title/selection policy still to freeze. |
| messages | Fresh id/thread/ordered seq; preserve role, kind, exact content/status/timestamp and plan fields. |
| tool_calls | Fresh id/thread/message ownership; preserve relative ordering, offsets and historical fields. Structured reference policy below; external output-file ownership still to freeze. |
| tool_call_rounds | Remap message and call; preserve coherent response grouping. |
| assistant_responses | Remap message; preserve coherent response identities and exact UTF-8 byte ranges/kind. |
| image_attachments | Remap message; preserve ordinal and encoded bytes. |
| computer_screenshot_artifacts | Remap capture call and attachment owner; preserve ordinal. |
| assistant_run_durations | Remap message; historical duration only. |
| builtin_skill_activations | Remap message; preserve historical name/version/hash, not a new activation grant. |

Do not blindly use live-write APIs requiring streaming/running owners to copy terminal history. Use validated historical inserts inside one transaction.

## Unresolved acceptance conflicts — resolve before implementation

### A. Delivered input is also historical metadata

`0034_run_continuations.sql` extends `pending_messages`; there is no separate run-continuations table.

- `messages/mod.rs` derives historical continuation edges from delivered pending rows and `accepted_after_assistant_id`.
- `context_compaction.rs` substitutes delivered `model_content` for visible user `messages.content` when rebuilding model history.

Thus the issue's blanket exclusion of `pending_messages` is not equivalent to excluding live queue state: it can lose historical grouping and change actual model input. Copying model text into visible transcript content is not an equivalent correction. Options are a dedicated inert historical representation retaining these facts, or explicit refusal of affected prefixes in a deliberately smaller first release. Blindly cloning queue rows is not accepted.

### B. Completed versus interrupted cutoff

The issue's API section permits `done` or `interrupted`, ambiguously discusses excluding the last terminal assistant, while acceptance says any completed assistant. Current `done` can also be an intermediate steering segment; status alone does not establish a trusted Final. Freeze whether interrupted cutoffs are allowed and how legacy done-without-Final is treated. Latest valid completion should not be arbitrarily excluded.

### C. Structured screenshot and delegation references

- Screenshot receipts can embed `artifact_id: screenshot:<call-id>`. Unchanged captures can refer to an earlier capture, not the current call. Remap recognized structured receipts using the actual referenced capture; never replace arbitrary text globally. Reject unresolved/out-of-prefix artifact references or provide a specified faithful representation.
- Delegation input/output JSON can embed child/parent/message identities, checkpoints and recovery evidence. Omitting delegation tables does not itself define the copied receipt semantics. Choose an inert historical projection retaining readable history without execution authority, or explicitly refuse affected prefixes. Never mint working child/resume authority by cloning old identities.

### D. Remaining implementation choices

Freeze thread title/model/mode/permission defaults, source subagent eligibility, `output_full_path` ownership (SQLite cannot atomically clone external files), provenance deletion semantics, and caller busy fencing. Persisted validation cannot replace an in-memory execution owner contract; the later UI caller must guard activity without running synchronous clone work on the draw path.

## Proposed risk-based cloud verification

Use migrated in-memory Stores with real storage readers, no provider/desktop/process E2E:

- Source immutability; Unicode prefix and tool/response grouping; later suffix excluded; provenance and sidebar/workspace identity.
- Image and screenshot graph, including unchanged capture aliases and exact bytes.
- Malformed/incomplete tool groups and offsets refuse atomically.
- Delivered-model-content/continuation and delegation cases according to the chosen policy.
- Typed eligibility failures and late-insert failure rollback; no runtime/usage/checkpoint clones; independent repeated forks.
- Migration compatibility and foreign-key integrity.

Run via the existing PR Actions only. Preserve development test source/full SHA/patch and actual results before cleanup; independently review the implementation, remove Rust tests and exclusive support, then obtain required CI on the final head with `no Rust tests` (0 discovered/0 executed). CI is not user acceptance. Do not close the issue or mark Done before user hand testing.

## Evidence and next step

Parent static inspection and independent read-only schema investigation agree on the delivered-input conflict. Investigation made no writes and stopped at the baseline. This document is a review artifact, not an implemented feature or passed check. Resolve A/B/C, then freeze the minimal spec and delegate the single implementation writer. No local Vega code, build, test or UI execution was performed.

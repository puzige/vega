# Native project thread fork — implementation contract v2

## Stage, decisions and scope

- Issue #518; parent #498; takeover `vega-518-takeover-20261025T021710Z` replaces the prior paused claim by explicit user approval.
- Integrated baseline `927032ed2a8f177ebc8d72dc9a10d1c50c1e4a40`, including master `420a0eb9a06594173b658f7e3a4437e3d2b5432d` and original investigation `82cc7d6a8a3ea916c24e3698e3b9d1f629e4b842`.
- User decisions, 2026-10-25: preserve complete durable historical semantics, including delivered model input, continuation, tool/screenshot/delegation history, without copying execution or recovery authority. Cutoff is ONLY a normally completed assistant with a complete trusted Final. Interrupted cutoffs and partial replies are excluded.
- Data layer only: Native project chats, fresh identities, unchanged source, atomic prefix clone. No Fork button/event wiring, ACP replay, run start, installation or user-runtime mutation. #519 consumes the API and inert history projection later.
- Stage: implementation contract; no implementation or behavior validation claimed. Local work is static only. Development Rust tests execute only in existing Actions, are preserved as evidence, then removed before merge. Independent implementation review and final-head required CI remain mandatory.

## Acceptance

1. `vega_conversation::threads::fork_thread(store, source_thread_id, through_message_id)` returns a new top-level Native project thread. Never use `parent_thread_id` or `workspace_owner_thread_id` for fork provenance. Sidebar visibility and project workspace resolution must hold.
2. Eligibility, reads and writes share one coordinated immediate transaction. Source must exist, use Native, have a real project, and have no persisted streaming message or nonterminal tool. Reject subagent/workspace-owner sources: their actual execution workspace need not be the project directory. The future caller must also fence in-memory active startup/execution; database checks cannot certify that state. No synchronous clone on the UI draw path.
3. The selected message belongs to the source, has role assistant and status done, and passes the existing `messages::completed_response_text` trusted-Final contract. Latest valid completion is allowed. Prefix is all source messages with seq <= selected seq, not a replay of visible text.
4. Preserve exact visible content, role/kind/status/timestamps/plan review fields, image bytes/ordinals, tool status/input/output/audit metadata, tool rounds, response byte boundaries and historical durations. Allocate fresh thread/message/call/response identities, with consistent references and chronological thread-local sequences. Do not rewrite arbitrary prose or arbitrary JSON string values.
5. Preserve actual delivered model input and continuation relationships without creating pending/claimed/delivered queue rows in the destination. Forking an already forked thread must preserve the same facts. No hidden fallback to visible text when an override exists.
6. Preserve screenshot receipt references including unchanged aliases to earlier capture calls. Preserve delegation receipts and evidence as inert history, not as live children, checkpoints, Read authority or resume capability. No retrospective enrichment from a child's current state after the cutoff.
7. Source and every dependent source record remain unchanged. Failed validation/insertion leaves no destination residue. Independent successful forks have disjoint identities; request-id idempotency is not promised.
8. Destination has no token usage, context checkpoints/status, pending messages, credential continuation/exceptions, delegated runs/executions/recovery/checkpoints/Read receipts, skill execution snapshots or thread pins. Historical builtin activation facts do not grant new activation. No agent or composer execution is started by this API.
9. Typed refusals distinguish missing source, unsupported backend/binding, invalid cutoff, active/corrupt history; store failures remain real failures. No silent reduced-history success.

## Concrete implementation plan

### Store schema and APIs

- Add `0035_thread_fork.sql` after rechecking latest migration at integration; `0031` is occupied. Register in `vega_store/src/lib.rs`.
- Add nullable `threads.forked_from_thread_id` (FK source thread ON DELETE SET NULL), nullable `forked_from_message_id`, and source-thread index. Source-message provenance is a historical identity, not a live FK; deleting the source does not delete a fork.
- Append columns/mappings to `vega_store::threads::{COLUMNS,ThreadRow,thread_from_row}` and `vega_conversation::types::Thread`; initialize None in ordinary constructors/literals and map in conversation row conversion. Do not alter existing create behavior.
- Add a focused store fork module, owning SQL, maps, validation and historical inserts. Conversation has no direct rusqlite dependency. Prefer an explicit typed storage error/refusal and public orchestration bridge rather than SQL in conversation.
- Add an inert per-message historical delivery table keyed by destination message/thread with nullable actual model content and mapped predecessor assistant. This is not a queue: no behavior, generation, claim, payload, execution state or delivery APIs. FK/cascade ownership must follow destination messages.
- Add a small inert tool-history reference/archive representation only where structural receipt projection changes identity or needs distinguishing from live delegation. Preserve original bytes and explicit reference provenance; no live execution tables refer to it. Use typed readers, not executable JSON deserialization into authority objects. Avoid a generic history engine.

### Transaction and prefix validation

- `Store::immediate_transaction` is the existing coordinated write boundary. Pass its underlying transaction to store APIs; commit only after conversation history validation. Do not call streaming-only insert APIs to clone terminal rows.
- Validate source eligibility and selected trusted Final inside the snapshot. Validate the entire selected prefix's terminal tool groups, response ranges, image ownership and references, not unrelated suffix content. Add a bounded prefix-source reader or load the cloned destination inside the uncommitted transaction; reuse `agent::pipeline::history_from_context_source` for real canonical grouping checks through an appropriate crate-private bridge. A corrupt suffix must not invalidate an otherwise closed prefix, except persisted activity correctly blocks forking the source.
- Copy tables in FK-safe order: new thread, messages, tool calls, rounds/responses, attachments, screenshot graph, historical durations/builtin facts, inert delivery/archive references. Keep message/call ordering and response grouping coherent; all IDs are allocated and mapped once.
- New thread retains source title/model/mode, but uses `confirm` permission mode (never inherit auto/full_access), active lifecycle, unpinned/unread=false, current creation/update time, null parent/workspace owner. Project-level permission rules continue to be governed by existing project policy, not cloned grants.

### Actual input and continuation

- Source facts come from delivered `pending_messages.model_content` and `accepted_after_assistant_id`, OR existing inert historical delivery on a forked source. Do not clone live queue state or generation tokens.
- `context_compaction::{load_messages,...}` must read inert actual model text with existing delivered fallback and finally visible content, including byte-count guards and fingerprinting. Existing non-fork history remains unchanged.
- `messages::page_continuations` must union historical predecessor facts with existing delivered facts while preserving same-thread, role, terminal user and chronological successor checks. No duplicate edges.
- Extend `credential_privacy::{fields,clean/source projection}` for inert model/archive text, redaction and fingerprints so hidden model content cannot bypass credential review. Preserve source ownership and invalidation behavior. Do not copy credential exceptions.

### Screenshots and delegation

- Copy image BLOBs directly with mapped message and capture-call owners. Historical inserts cannot use APIs requiring streaming/running state.
- Recognize the runtime's first-line screenshot receipt JSON and map only recognized `artifact_id: screenshot:<capture-call-id>` to the copied capture, preserving remainder and unchanged status. Do not substitute the current alias call for the original capture. Missing/cross-thread/out-of-prefix captures refuse atomically; preserve original receipt bytes when projected output changes.
- Delegation input/output is historical data. Preserve receipt/task/result/evidence bytes and explicit mappings, including source parent/message/request call and external child/execution/checkpoint/read references. External references remain historical identities, never new working threads or checkpoint rows; no global recursive substitution.
- Existing successful delegation card projection deserializes `SubagentSnapshot` and offers open-child behavior. Provide a distinct typed historical/inert result projection at the headless history boundary so downstream consumers cannot accidentally treat it as a live snapshot. Do not claim current renderer is already wired: #519 must consume this historical projection before exposing Fork. No UI edits or inert-fork buttons in this card. If an existing UI consumer cannot accept the new type without a renderer change, report the exact seam before changing UI scope rather than emit a fake live agent.
- Do not clone supervisor registry, delegated ownership, recovery or execution rows even if terminal. `stage_recovery::load` consumes those rows, so terminal does not mean inert.

### External output metadata and limits

- Preserve `output_full_path` only as exact historical metadata; do not move/read/delete/open its file or claim to snapshot external bytes. Current code has no writer/ownership/retention contract for that field. Durable bounded `output_text` remains exact. Do not advertise preserved full external output if bytes were never in storage.
- Complete history means complete persisted semantics, not reconstruction of missing live provider-wire or ephemeral desktop state. Persisted assistant screenshots remain readable history, not current observations.
- Existing source size/image/UTF-8 constraints remain; data corruption, unresolved required references or storage limits fail explicitly. Do not reject merely because valid history includes steering or delegation.

## Risk-based cloud verification

Use migrated in-memory Stores, real storage/context/history readers, no provider/desktop/process execution. This data-integrity feature warrants a small focused development suite:

- Complete Unicode prefix, trusted Final, comments/tools/response ordering, plan fields and suffix exclusion; source immutability, direct provenance, sidebar/workspace identity and independent repeated fork maps.
- Delivered model override differs from visible text; continuation crosses page edges; fork-of-fork; queue and live ownership remain empty; canonical source/destination history equivalent modulo declared identity mapping.
- Image exact bytes; screenshot unchanged alias points to original copied capture; first-line receipt; broken/out-of-prefix capture fails atomically.
- Delegation inputs/receipts/checkpoint/evidence retained as inert history; archive identities cannot resolve through live child/checkpoint/Read APIs; later child changes do not enrich cutoff history.
- Typed eligibility and cutoff failures (standalone, non-Native, subagent workspace, active source, interrupted/no-Final), incomplete tool group, malformed UTF-8 ranges, late SQL failure rollback and foreign-key integrity.
- Hidden model/archive text participates in privacy scanning and redaction; no exceptions/authority copied; historical output path metadata is not read as a file.

Use existing PR Actions only. Record actual tests discovered/executed/passed/failed/skipped and immutable source SHA/patch before cleaning Rust tests and exclusive support. Never remove a failing test to obtain green. Independently review implementation and test cleanup; run full required CI on final no-Rust-tests head. CI/merge is not user acceptance; keep Issue open and move In review only after merge.

## Evidence and change record

- v1 `82cc7d6a8a3ea916c24e3698e3b9d1f629e4b842` discovered delivered-input, cutoff and structured-reference conflicts; preserved in Git history.
- v2 closes product choices with explicit user answers. Parent statically confirmed delivered-input/continuation consumers unchanged through current master. Independent read-only investigation at `927032ed2a8f177ebc8d72dc9a10d1c50c1e4a40` verified screenshot aliases, live delegation loader, missing inert UI route and external-output evidence limitations; no writes or execution.
- Code implementation, independent implementation review, cloud validation and user acceptance remain separate pending stages.

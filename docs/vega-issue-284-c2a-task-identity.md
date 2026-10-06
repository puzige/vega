# A4-C2A: Persist task backend and session identity

Issue: [#284](https://github.com/puzige/vega/issues/284)
Contract: [A4 Codex task specification](vega-acp-codex-v1-spec.md)
Base: `d81a1f3b2007d3402782952a8e745d4e0c587f1c` (A4-C1 merged)

## Goal

Give every Vega task a durable backend identity, preserve the selected Codex profile and workspace as an immutable execution snapshot, and record session creation so later integration can safely decide whether a prompt may be sent. This card does not start an ACP process or send prompts.

## Boundaries

- Legacy task rows read as `Native`; their existing project, model, permission, message, and usage data keep their current meaning.
- A Codex task snapshot identifies the profile and adapter, executable and argument list, adapter/version identity, allowlisted non-secret launch settings, selected project/worktree identity, canonical absolute working directory, and explicitly selected additional directories.
- Persisted fields are typed business data. Do not persist ACP frames, prompt text, process environment values, API keys, OAuth tokens, or credentials. Do not accept an untyped raw configuration blob as the task snapshot.
- A submitted task keeps its Vega thread ID as its only task identity. Saving the task identity and immutable snapshot must not create an alias or a second thread.
- Session creation follows `absent → intent → confirmed | uncertain | definitively_failed`. The intent is durable before external creation begins. `confirmed` requires a non-empty returned ACP session ID committed durably. Only that state can produce a prompt-eligible binding.
- An uncertain result is fail-closed: it cannot be retried into a fresh creation intent or treated as prompt-eligible by this card. Any deliberate reconciliation belongs to later recovery work.
- Reject duplicate active intents, duplicate external session bindings, and all invalid transitions. Deleting a Vega task must not leave orphaned identity or creation records.
- Native task creation, listing, draft materialization, and updates retain existing behavior.

## Implementation plan

1. Add a typed shared projection for backend, profile reference, execution snapshot, session creation state, and confirmed session binding. Keep database row types in `vega_store` and map invalid persisted vocabulary to a fail-closed conversation error.
2. Add an additive migration from schema version 15. Preserve old task rows as Native, add suitable constraints and indexes, and retain foreign-key/cascade integrity.
3. Add store operations for task identity/snapshot persistence and conditional session-state transitions. Keep identity plus snapshot tied to the existing thread ID; make transitions transactional and make the prompt-eligibility read depend on a committed confirmed binding.
4. Add conversation-layer APIs that validate task/profile/workspace data and expose the typed projection without leaking SQL rows or ACP wire types.
5. Add focused migration, store, and conversation tests for the matrix below. Update this document with actual implementation paths, commands, and results before opening the PR.

## Acceptance matrix

| Case | Evidence required | Status |
|---|---|---|
| Upgrade a version-15 database | Migration is additive; legacy thread fields and child rows survive; legacy task backend reads Native | Pass: `version_fifteen_upgrade_preserves_native_thread_and_children` |
| Native regression | Existing create/list/draft/materialize/update path remains unchanged and reads Native | Pass: six focused thread regression tests; creation and list assert Native |
| Codex task identity round trip | Profile reference, immutable launch snapshot, selected workspace, and canonical cwd survive close/reopen | Pass: `codex_task_identity_uses_the_materialized_thread_and_immutable_snapshot` |
| Stable Vega task identity | Snapshot and session state use the already-materialized thread ID; binding does not create a second task | Pass: identity test asserts same thread ID, one listed task, and rejected duplicate binding |
| Session creation intent | Intent is durable before the external session creation call can be attempted | Pass: `codex_session_intent_is_durable_and_confirmed_binding_alone_is_prompt_eligible` closes and reopens after intent, before confirmation |
| Confirmed session binding | Non-empty ACP session ID is committed; prompt-eligible lookup succeeds only after that commit and returns the persisted binding | Pass: identity test verifies no binding before confirmation and exact binding after reopen |
| Uncertain creation | Structured uncertainty survives reopen; duplicate retry, transition to confirmed, and prompt-eligible lookup all fail closed | Pass: `uncertain_and_definitively_failed_session_creations_cannot_be_retried` |
| Invalid transitions and duplicates | Duplicate active intent, duplicate session binding, empty IDs, and unsupported state changes are rejected without partial writes | Pass: session-intent, confirmed-binding, uncertain/failure, and unique-session assertions |
| Privacy boundary | Persistence APIs accept only allowlisted typed fields; no prompt, environment, credential, or raw ACP frame is stored | Pass: typed column API; no raw config/environment/prompt/frame fields; adapter arguments reject API-key, token-value, and free-text inputs |
| Delete and rollback integrity | Deleting a task removes owned identity/session records; failed multi-row persistence leaves no partial Codex identity | Pass: `codex_session_id_is_unique_and_task_deletion_cascades_identity` and `codex_binding_rolls_back_backend_if_snapshot_insert_fails` |

## Verification record

Implementation paths: `crates/vega_conversation/src/types/codex_task.rs`, `crates/vega_conversation/src/codex_tasks.rs`, `crates/vega_conversation/src/threads.rs`, `crates/vega_store/migrations/0016_codex_task_identity.sql`, and `crates/vega_store/src/codex_tasks.rs`. Existing `Thread` test literals in `vega`, `vega_ui`, and `vega_conversation` were updated only to specify the preserved Native backend.

Focused evidence:

- `CARGO_NET_OFFLINE=true cargo nextest run -p vega_store -p vega_conversation -E 'test(version_fifteen_upgrade) | test(migrate_creates_exactly_the_twenty_eight_tables) | test(codex_task_identity) | test(codex_session_intent) | test(uncertain_and_definitively_failed) | test(codex_session_id_is_unique) | test(codex_binding_rolls_back) | test(create_thread_uses_ddl_defaults)'` — 8 passed, 711 skipped.
- `CARGO_NET_OFFLINE=true cargo nextest run -p vega_conversation -E 'test(create_thread_uses_ddl_defaults) | test(draft_thread_matches_create_thread_fields) | test(materialize_draft_reuses_the_draft_id) | test(list_threads_orders_by_updated_at) | test(update_thread_applies_the_field_set) | test(typed_modes_and_permissions_survive_restart)'` — 6 passed, 560 skipped.
- `cargo fmt --all -- --check` and `git diff --check` — passed.
- `CARGO_NET_OFFLINE=true cargo clippy -p vega_store -p vega_conversation --all-targets -- -D warnings` — passed.
- `CARGO_NET_OFFLINE=true cargo check -p vega_ui -p vega --tests` — passed; Cargo reported the existing future-incompatibility warning for dependency `block v0.1.6`.

No full-workspace tests or live Codex/ACP process were run; execution and recovery remain outside this card.

## Out of scope

New Task UI, Agent settings UI/profile management, ACP process routing, prompt dispatch, session restart recovery, activity and approval projection, usage reporting, and changes to Native execution.

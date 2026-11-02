# Fork mutation provenance and compact pending feedback

## Scope and baseline

Issue #851; baseline `ec8b3f8bbd2493690032ceb0e662f46d9f1f1bda`. User authorized confirming the repair, creating the card, then implementing. Only Fork mutation-history continuation and its pending icon are in scope. No user database edits, checkpoint-file rewrites, historical tool execution, dependencies, or unrelated #519 changes.

## Root cause

Fork allocates fresh thread/message/tool identities but copies successful write/edit output verbatim. The output contains the real original `checkpoint_ref`. Run preparation validates terminal mutations against the new identities and rejects them as corrupt. SQLite structure checks do not diagnose this application-level mismatch. The original diagnostic lacks a thread ID, so attribution of a specific submission remains conditional; the copied identity conflict itself is confirmed.

## Repair contract

1. Keep original mutation audit/output and checkpoint reference unchanged. Add an owner-cascading mutation-origin table storing original project/thread/call identity for copied write/edit calls. This is historical provenance only, not execution, recovery, rollback or permission authority. Do not create or rename checkpoint files.
2. Resolve provenance in the Store from durable records. New clones inherit the original origin, including repeated forks; original source deletion must not break newly recorded provenance. Origin project must equal the current thread's project. Do not infer identity merely by decoding an arbitrary checkpoint string.
3. Support pre-fix forks without rewriting their records: walk the actual fork ancestry and completed cutoff. Only a call in the copied message prefix with equal message sequence/content/role/kind/status and equal tool sequence/name/input/output/status/approval/offset/timing metadata may inherit the source origin. Calls after the cutoff use their own identity. Missing source, mismatch or cycles fail closed; an old fork whose source has already been deleted cannot be reconstructed safely.
4. Share terminal-tool reconstruction between run preparation and Fork pre-commit validation. Mutation calls use validated origin identities; all other tools use current identities. Retain existing input, output, approval, exit/duration and safe projection validation. New calls in a fork and ordinary threads continue to require their own identity.
5. Validate the complete newly cloned terminal history before commit; corruption rolls back the destination. The source stays unchanged. No schema deletion or alteration of existing constraints; add a numbered migration.

## UI contract

Replace the pending Fork action with a same-size centered indicator, using the existing Spinner in active windows when reduced motion is off. Remove visible `Creating…` text and horizontal padding/gap that widen the action. Preserve the localized accessible pending name, inactive/reduced-motion static fallback, single-flight/disabled behavior, focus/keyboard discoverability, failure recovery and route fences. No new persistent explanatory copy.

User acceptance revision (2026-11-02): while conversation work makes Fork unavailable, omit the footer Fork action entirely, like Copy, rather than rendering a gray disabled icon or reserving its horizontal slot. Restore the normal hover/focus action when idle. A pending Fork creation keeps its own same-size spinner; other footer Fork actions are omitted until it settles. Keep context-menu disabling and activation/backend guards unchanged. Verify idle, running (including shared runtime ownership), pending, completion/failure, keyboard focus and unchanged Copy/timestamp/footer geometry by static branch review plus required CI; real visual acceptance remains user manual testing.

## Verification

Development-only Rust regressions, run exclusively by Actions, cover real migrated Stores: ordinary mutation validation, success write/edit Fork continuation, repeated forks, legacy ancestry recovery, new calls using current identity, missing/mismatched origins and copied rows rejected, new provenance surviving source deletion, atomic failure, and original checkpoint/output unchanged. No real files or external services are required. Tests and exclusive support must be preserved at a full remote SHA and removed before final merge, with final-head required CI confirming no Rust tests (0 discovered/executed). Local checks are format/diff/static only, never Vega execution/build/tests. Independent non-writer review and latest-head required CI precede merge. UI animation, navigation, restart and real sending remain post-merge user manual acceptance; do not close the Issue or mark Done before that acceptance.

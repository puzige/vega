# Fork title numbering

## User acceptance revision

Forked conversations must be distinguishable by a trailing sequence number. The latest user revision starts Fork numbering at one, e.g. `四步法多Agent (1)`, superseding the original example's starting number. This extends #851; it does not change the completed history fixes.

## Contract

- Keep the source title unchanged. A first fork of `Title` is `Title (1)`; later forks of that title in the same project use the next number above existing numbered titles (including archived conversations).
- A fork of `Title (1)` continues as `Title (2)` or the next unused higher number, never `Title (1) (1)`. Only a source that is itself a fork has its final canonical numeric suffix treated as a generated suffix. A non-fork title such as `Report (2026)` remains meaningful text and forks as `Report (2026) (1)`. Existing numbered titles, including those created under the previous starting-at-two rule, remain unchanged and continue above the existing maximum rather than filling gaps.
- Use English half-width parentheses `(` and `)` as explicitly requested by the user. Canonical suffixes are ASCII decimal integers >= 1 without leading zeros, preceded by a space and enclosed in parentheses. Other parentheses, Unicode text, `%`, `_` and brackets remain literal title content.
- Collisions are checked against top-level conversations in the same project, including archived ones. Other projects and child agents do not affect numbering. Existing conversations are not renamed or backfilled.
- Allocate and insert the new title inside the existing immediate Fork transaction. Concurrent forks serialize through that boundary; failure rolls back without changing the source. No schema changes or changes to permission/history validation.
- Exhausted integer numbering fails explicitly rather than overflowing or fabricating a duplicate.

## Sidebar entry (additional user request)

- Add Fork to the shared conversation-row context menu (projects, pinned, recents and archived rows), including conversations that are not currently open. Reuse `ui.branch_in_new_chat`; no explanatory text is added to the normal menu.
- Fork through the latest completed assistant message with a trusted Final, selected inside the same immediate transaction. Trailing user/interrupted messages are not copied. The per-message Fork cutoff remains unchanged.
- Only Native top-level project conversations support Fork. Keep the item disabled while the target is running, another task mutation is pending, or no trusted completed reply exists; expose the existing busy reason, or a localized no-completed-reply reason. Recheck eligibility on activation and persisted activity under the worker transaction/admission lock.
- Run copying off the UI thread, prevent duplicate submissions, preserve drafts, and open the new numbered fork only if the originating navigation route remains current. If navigation moved, retain the successfully created fork in the list without switching the user back. Surface failures without changing the source. Clear pending state on success or failure and preserve existing menu keyboard indexing and organization actions.

## Pending message footer (additional user request)

While the message's own Fork is creating, keep its entire existing footer visible after the pointer leaves: Copy, the same-size pending indicator and timestamp. Do not force other messages' footers visible. Completion/failure clears pending through the existing lifecycle and restores normal hover/focus visibility. Keep Copy behavior, action guards, geometry, accessibility, reduced-motion fallback and running-state hiding unchanged. No new visible text is required.

## Implementation and validation

Allocate the stored title at `vega_store::thread_fork::clone_prefix`, with a private title allocator. All UI projections already consume the persisted thread title. Add an atomic latest-Final conversation Fork entry and a sidebar background action that reuses the existing validated clone boundary.

Actions-only development regressions use migrated in-memory stores and the real clone transaction: first/repeated/nested forks, archived collision and project isolation, meaningful numeric titles, literal punctuation, rollback and source preservation. Preserve tested source/results before removing temporary Rust tests. Required final-head CI and independent non-writer review precede merge. Local validation is static format/diff only, never Vega execution, compilation or tests. Actual sidebar/header display remains user manual acceptance after release.

### Starting-at-one revision validation

The revision only lowers canonical suffix recognition to one and the unnumbered-source baseline to zero. No schema, collision query, transaction, overflow rejection, or historical identity changes are needed. Verify first fork `(1)`, nested/repeated `(2)` and higher, existing `(2)` continuing to `(3)`, canonical `(1)` collision, meaningful ordinary numeric titles, and zero/leading-zero rejection by static boundary review. This low-risk constant adjustment does not restore development Rust tests; final required Actions must still pass with 0 Rust tests discovered/executed. Independent non-writer review and user title acceptance remain separate requirements.

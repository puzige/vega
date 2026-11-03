# #859 · Skills directory enable recovery

## Confirmed defect

A persisted project Skills root can retain its canonical path and inode while its
filesystem device number changes. `SkillSettingsService::rebind` correctly rejects
that identity. The UI nevertheless retries `SetSource`, and confirming a fresh
project preview fails against the existing project-source unique index.

## Scope and acceptance

- Preserve exact path/device/inode binding, per-Skill SHA approval, session receipts,
  selected-project isolation, and transaction-local consent-generation CAS.
- For a disabled source diagnosed `changed_review_required`, replace the Enable
  switch with a “Review directory again” action. It opens a fresh preview of that
  source's configured directory rather than enabling stale consent.
  Project/global previews use their authoritative roots; imports use the exact
  already-linked configured path, never neighbouring directories.
- Cancel leaves all persistent state untouched. Confirm rechecks the preview's
  identity and generation and atomically replaces the old association for the same
  project, Vega-global slot, or imported configured path. Replacement revokes old
  authority and cascades deletion of old approvals. New sources remain disabled
  and non-automatic, requiring explicit enable and per-Skill review.
- Already linked identical identities remain idempotent. Unrelated sources and
  source files remain untouched. Failed insertion rolls back removal.
- No new dependency, schema migration, automatic rebinding, or runtime database fix.

## Minimal implementation

1. Add a scoped source-preview service entry point and matching UI action.
2. Offer review instead of stale-source Enable; retain normal `SetSource` for valid roots.
3. In the existing link transaction, remove only the association replaced by the
   explicitly confirmed root before inserting its fresh disabled association.
   Existing deletion triggers and foreign keys revoke authority/clear approvals.

## Validation

Local: focused diff/format/static contract inspection only; no Vega execution,
compilation or tests. Final head: independent review plus required Actions.
No new Rust tests are introduced under current delivery policy. CI's empty Rust
suite is `no Rust tests`, not behavioral regression evidence.

User acceptance: stale directory offers review and opens preview; cancel preserves old state; confirm
replaces the card without a uniqueness error; enable succeeds for the current
identity; Skills require fresh individual approval. Changed-again roots or stale
consent are still rejected. Verify source files remain intact.

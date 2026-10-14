# Credential privacy controls delivery

Implements Credential Broker design sections 7–9. Raw user records stay unchanged until an explicit permanent-cleanup confirmation. A shared safe text projection covers hydrated history and its copy/selection sources, task search labels, and on-demand credential review. Existing application exports are audited; this change does not introduce an export product.

Review is on demand. Findings show source IDs/field/rule/byte range and a masked preview, never a matched secret or its prefix. False-positive exceptions bind thread, rule, and keyed content digest; exact owner matches cannot be exempted. Changes to rule or content require review again.

Cleanup previews affected sources and requires a second explicit action. The service fences the source fingerprint and current detection before an atomic cleanup, invalidates summaries/continuations, preserves tool pairing and offsets, and does not modify external files or backups. Database logical cleanup does not promise WAL/backup physical erasure.

## Test matrix

| Area | Required behavior |
| --- | --- |
| Safe projection | synthetic known and high-confidence values masked; raw user data unchanged; copy shares safe text |
| Findings | metadata has no secret, byte ranges valid, low-confidence review remains available |
| Exceptions | thread/rule/content bound; exact values never bypass; altered content invalidates |
| Cleanup | explicit confirmation, stale source rejected, transaction rollback, JSON remains valid, unrelated thread untouched |
| Recovery | destructive cleanup invalidates stale continuation capability and summaries |
| Product | review action reachable, cancel does not write, loading/error/confirmation states, keyboard button labels |

Local checks are rustfmt and diff/static review only. Compilation, Clippy, and tests run on cloud PR Check. Native UI, TLS, and OS integration remain user hand-test evidence.

## Implemented integration and evidence

- Startup registers each database's explicit configuration root. History hydration refreshes known values; the UI has only safe strings. A root-scoped keystore revision invalidates live echo caches after application set/delete. Unknown/stale caches show an unavailable marker; the normal accepted-turn composer-history refresh replaces the latest deferred user echo with the safe persisted projection. External credential-file edits bypass this in-process counter; fresh request/attempt readers remain the sending boundary.
- Ring HMAC-SHA256 fingerprints never persist raw matched values. Each projection/run snapshots exceptions once. A keyed owner-state epoch invalidates prior exceptions on owner changes; old run filters check the current epoch in memory.
- The on-demand panel locates messages, supports adding/revoking unknown-pattern exceptions, and requires explicit confirmation before permanent cleanup. Low-confidence findings remain reviewable; only values requiring masking are offered for cleanup.
- Tool offsets and assistant response ranges are remapped with safe text; permanent cleanup invalidates summary caches and continuation receipts. Images/binary attachments are outside text scanning. No conversation export feature was found or introduced.
- Added synthetic service tests for non-mutating safe history, exact-match refusal, thread/content scope, revocation, owner reclassification, stale cleanup rejection, and unrelated-thread preservation. Tests and compilation are pending cloud CI. Local rustfmt and git diff whitespace checks passed.

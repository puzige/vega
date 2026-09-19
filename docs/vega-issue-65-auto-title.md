# Issue 65 · A1-05 automatic conversation titles

Freeze: 2026-09-19, Asia/Shanghai. User decision on GitHub #65:
one extra model request per new conversation; failure falls back to its first
message. This supersedes R60's deferred automatic-title scope, not its header layout.

## Contract

R1. Start naming only after the first user turn is durably accepted. An empty
draft, failed preflight, or opening/switching a conversation triggers no request.
New threads and existing empty, untitled threads are eligible. Previously
populated conversations and existing nonempty legacy titles are not backfilled.
Persist a provisional first-message title immediately with the accepted turn,
without delaying the conversation run on the naming network request.

R2. Use the original composer text, never expanded @file contents, image bytes,
tool output, history, credentials, or system context. Collapse whitespace and
trim; bound input to 2,000 Unicode scalar values. Fallback is the first 40
Unicode scalar values of normalized original text (no byte slicing); for
image-only turns use 图片对话. Model title uses the user's language, plain text,
at most 40 scalar values, with whitespace collapsed and surrounding quotes
removed. Blank/unusable output retains fallback. No markdown title rendering.

R3. Make one additional, non-tool model request using the provider/model and
credential authority frozen for the accepted first turn; never switch providers
or models. Request only a concise title from the bounded text (or a neutral
image-only description), no tools or extra file access. Cap output at 512 tokens,
wall time at 15 seconds, and disable automatic retries for this request.
The local title text accumulator is bounded to 8 KiB even if the peer ignores
max_tokens; ignore reasoning content and cancel on text overflow, keeping fallback.
Failure, timeout, cancellation or unsupported provider output retains fallback
and must not fail or block the primary conversation. Do not expose raw provider
errors or request content in logs. Extra usage, when returned, goes through
existing token/cost accounting without fake visible conversation messages;
unknown usage or price must not be fabricated as zero-cost success.

R4. Persist an atomic at-most-once claim before network dispatch. This is not an
exactly-once guarantee across a crash: a claimed request interrupted by process
exit keeps its fallback and is not retried automatically on restart. Multiple
submission/ACK paths and later user turns cannot request naming again. Add an
additive numbered migration 0008 for minimal provenance/claim metadata; leave
existing migrations untouched. Exact schema-version/inventory guards may be
updated only for this migration while retaining exact assertions.
The existing Issue 63 version-six upgrade fixture may be corrected to build
the actual first six migrations instead of relabeling the newest schema as
version six; retain all its attachment/message/foreign-key assertions.

R5. Manual titles always win, including a rename to the same text as fallback
or a rename away and back. Protect both rename_thread and update_thread(title).
Generated completion is a conditional title-only update of the originating
thread, never a stale full Thread write; cannot overwrite model, project,
permissions, archive/pin state or resurrect a deleted thread. Automatic updates
do not bump user-activity updated_at or reorder Recents.

R6. Sidebar and current header refresh after provisional/generated title updates.
Late results for another thread must never navigate, switch selection, overwrite
the current header, clear the composer or interrupt another run. Persisted title
survives route changes and restart. Keep current title typography and layout.
UI uses conversation facade, never SQLite directly; runtime stays headless.

## Ownership and non-scope

Main owns this spec, handoff, review, full gates, native acceptance and integration.
Implementation executor owns the narrowly needed changes in vega_store,
vega_conversation, vega_runtime, vega application and vega_ui, with its delivery
report. Architect approves `tracing.workspace = true` in vega_conversation,
using the existing locked/allowlisted version for fixed, content-free persistence
diagnostics. No other new dependency, settings toggle, title regeneration button, bulk
backfill, provider configuration fix, pricing-policy change (#60), image changes,
remote push or unapproved deployment. Do not weaken existing tests.

## Acceptance

- Production controller/provider/store chain with owned temporary DB and bounded
  mock network: first durable turn creates fallback, exactly one naming dispatch,
  successful result persists; subsequent turns and restart do not repeat it.
- Empty draft/preflight rejection, pure image input, @file text boundary,
  timeout/error/blank output, manual rename (including ABA/same text), concurrent
  title writes, deleted thread and route switch preserve R1–R6. Observe actual
  production wiring, not only isolated helper tests.
- Check request body contains only bounded permitted naming text, no images,
  tools or expanded file contents; capture title usage without visible fake turns.
- Native app through UI: new text task obtains an automatic title, manual rename
  is retained, and leaving/reopening retains titles. No manual config edits.
- Serial locked workspace tests, fmt, strict workspace/all-target clippy, build
  and package. Retain first failures and exact log paths/hashes in delivery.

Implementation uncertainty or conflict with existing API/spec must be reported
before inventing a substitute contract. Feature completion requires review and
native evidence, not only an executor's success report.

## Existing fixture adaptation (2026-09-19)

Full-workspace execution exposed 14 existing app fixtures whose single scripted
MockProvider shares request recording/round consumption between primary and the
new auxiliary request. Authorize a test-only provider wrapper at those fixtures'
injection sites: primary requests delegate unchanged to the existing mock;
chat_stream_once delegates to a separate recorded auxiliary mock, returning an
empty completed answer by default so fallback is exercised. Preserve every
existing primary request-count, message-content, permission, stop, model, image
and reference assertion. Add a wrapper regression proving one auxiliary request
cannot consume primary rounds. Keep dedicated #65 success/HTTP tests unwrapped.
Do not disable naming in production/test code or weaken request-count assertions.
The single old r69 first-submit empty-title assertion must instead equal that
case's normalized first-message fallback; all its identity/row-count/draft
assertions remain. This is a new product-contract expectation, not an exception
for arbitrary test failures.

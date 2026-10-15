# Issue 393 delivery

## Contract

See [thinking stream and diagnostics specification](vega-issue-393-thinking-stream-diagnostics.md). User authorized removing fixed reasoning byte aborts and completing failure diagnostics; PR status polling is every five minutes per AGENTS.md.

## Changes

- Runtime no longer rejects valid thinking/summary/replay due to delta, response, historical projection or aggregate byte/item quotas. Required replay is preserved for the next model request; display duplicates use an independent UTF-8-safe 256 KiB retention bound.
- Responses removes the equivalent adapter gates and whole-stream 4 MiB abort. Existing SHA-256 supports incremental prefix verification without retaining duplicate full text. Generic 4 MiB SSE frame protection and 256 KiB tool-argument validation remain; framing accepts LF, CR and CRLF across network chunks.
- Failure details persist safe source/kind/scope, observed/limit/units and applicable item counts. Nullable legacy fields preserve unknowns. Root and primary failures, restored notices and diagnostic export retain known causes without raw model/provider payloads. Credential-specific failure data remains classified.

## Verification

| Evidence | Result |
|---|---|
| Runtime `cargo fmt --all -- --check` and `git diff --check` | PASS |
| Python SQLite in-memory execution of all migration SQL through 0025, plus populated prior-schema upgrade | PASS; legacy reasoning_limit detail stays NULL; no live database accessed |
| Local compilation, Clippy, Rust/GPUI tests, app packaging | NOT RUN per user local CPU restriction |
| Cloud PR Check | Pending |
| Real-model user acceptance / installed application | Pending; no app update or restart performed |

Regression coverage: single 1 MiB UTF-8 thinking/summary delta completes; >256 KiB reasoning and 300 replay items reach next tool continuation intact; cancellation prevents tool execution; runtime display duplicate retention remains bounded; done-only/partial/duplicate/mismatched Responses snapshots; >6 MiB cumulative stream; generic oversized frame diagnosis; LF/CR/CRLF and multiline framing; diagnostic storage/reopen/redaction and owner-specific restored failures. Tests are cloud-only until PR Check completes.

## Residuals

Required model-visible replay remains resident until normal safe compaction and is governed by actual request context accounting. This change does not promise unlimited model output or bypass genuine protocol/parser, context, transport or tool-input failures. Public implementation comparison and its limits are recorded in the specification. User hand-test follows green merge, issue remains open/In review until accepted.

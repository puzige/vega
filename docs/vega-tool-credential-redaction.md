# Tool-output credential redaction and safe conversation recovery

## Contract

This document specifies the authorized DraftIssue for local tool-output credential redaction and safe recovery of previously persisted conversations. It does not reopen or duplicate a closed issue.

The application must redact credentials it currently holds, plus common credential formats, before tool output can enter runtime events, the next provider request, or durable conversation state. A match is replaced with `[REDACTED:provider_credential]`. The boundary applies to ordinary and MCP tools, including structured output and tool errors. Runtime receives a complete tool result after each adapter has assembled its output, so fragmented stdout/stderr is scanned after assembly and before the result is emitted.

The current application-held values are supplied to the runtime as an opaque reader. Credential values must not enter logs, errors, events, tool schemas, or persisted configuration. The runtime also scans common provider prefixes, bearer values, JWTs, and credential assignments when no exact held value is available.

When a conversation is prepared for a new turn, existing assistant text, tool results, and checkpoint summaries for that thread are scanned and locally rewritten in the accepted SQLite transaction. If any legacy content is changed, the run ends with a local `CredentialExposureBlocked` error before a provider request is sent. The UI-facing failure kind gives a local recovery action and must not be classified as a provider transport/network failure. Persisted user-authored messages and tool arguments are not rewritten; the final outbound request guard blocks a contaminated projection before network dispatch.

Automatic and manual context summaries use the same held-credential reader. Summary requests are checked before provider dispatch, and returned summary text is redacted before it enters a checkpoint or the projected continuation context.

No schema migration is required. Tool output full-file spill is not part of the current bounded output contract; `output_full_path` is required to remain absent, and the redactor must not open external paths.

## Acceptance matrix

| Surface | Required behavior | Evidence |
| --- | --- | --- |
| Runtime output scanner | Exact canary values are removed from stdout, stderr, structured JSON, error text, assembled fragments, and large strings; safe neighboring text remains | Runtime unit tests |
| Tool lifecycle | Redaction precedes tool-output events, persistence, and follow-up model context | Temporary project/store fixture with a test Bash executor and `MockProvider` |
| MCP structured output | Text and structured result are combined and redacted before the result is returned | MCP registry test |
| Legacy restore | Existing assistant output, tool output, and context summary are redacted locally; the first resumed attempt produces the typed local block and zero provider requests | Temporary SQLite fixture and `MockProvider` |
| Context summary | Held credentials in a summary response are redacted before checkpoint installation | Compaction fixture with `MockProvider` |
| Outbound guard | A contaminated historical request is blocked with `CredentialExposureBlocked`, not a status-less provider transport error | Provider wrapper regression |

All credential fixtures are synthetic canaries. Tests must not read user SQLite, local provider configuration, or real credentials and must not send network requests.

## Delivery evidence

The regression was first run against the old behavior and failed because the contaminated historical request was classified as a provider transport failure. After implementation:

```text
cargo nextest run -p vega_runtime issue170_ --lib
5 tests run: 5 passed, 212 skipped

cargo nextest run -p vega_conversation issue170_ --lib
5 tests run: 5 passed, 484 skipped

cargo nextest run -p vega_runtime issue73_final_result_join_redacts_owner_secret --lib
1 test run: 1 passed, 216 skipped

cargo fmt --all -- --check
exit 0

git diff --check
exit 0

cargo check -p vega
Finished `dev` profile
```

The checks use `MockProvider`, temporary directories, an owned temporary SQLite database, and a test-only Bash executor. No full workspace tests, real provider calls, user database, or real credential reads were used.

## Accepted limits

- Exact redaction covers credentials returned by the current application credential reader. An arbitrary opaque value no longer held by the app and lacking a recognized format cannot be identified from content alone.
- Common-format detection is intentionally heuristic and may redact credential-shaped values in ordinary tool output.
- Existing user-authored messages and raw tool arguments are not rewritten. They are blocked by the final outbound request scanner if they contain a held credential or recognized format.
- The current store/runtime contract has no full-output spill file; external output files are not opened or modified.
- Real provider and native UI acceptance remain for the user on the merged build.

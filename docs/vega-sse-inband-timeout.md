# Chat Completions in-band timeout recovery

## Scope and evidence

2026-10-27: CPA requests 4ed46240, b3c5cba9, 50ec543a and b8e712eb returned HTTP 200 SSE followed by an error object with type `server_error`, code `request_timeout`, and an upstream stream-closed message. Vega currently maps every top-level error to nonretryable Protocol.

## Contract

- Parse top-level error objects separately from malformed envelopes. Only the exact structured pair `server_error` / `request_timeout` is classified as retryable Timeout. Do not infer retryability from arbitrary provider message text.
- Preserve HTTP response metadata through existing recovery. Emit only a fixed sanitized diagnostic, never the upstream message or arbitrary code.
- Unknown codes, invalid error shapes and mixed error/choices envelopes remain nonretryable Protocol. No broad server-error retries or parser relaxation.
- Existing retry budget, cancellation and terminal Done boundary remain authoritative. Each retry builds a fresh assembler. Pending tools and partial text are discarded by the existing runtime RetryScheduled handling.
- No tool from an incomplete attempt executes. A completed tool from an earlier round is not re-executed when a later model request retries; call-id/result replay protections remain unchanged.
- No service restart, credential/config change or local build/test execution.

## Verified development evidence

- Test-bearing commit: `0b03fbc190882698f07b7cb439d2ff0a328ffeee` ([source](https://github.com/puzige/vega/blob/0b03fbc190882698f07b7cb439d2ff0a328ffeee/crates/vega_runtime/tests/sse_timeout.rs)).
- [Actions run 40371921889](https://github.com/puzige/vega/actions/runs/40371921889): Clippy passed; Nextest executed 5 tests, 5 passed, 0 skipped. Covers partial-argument recovery, uncommitted-tool discard, completed-tool result reuse, retry exhaustion, and unknown/malformed/mixed error rejection.
- Independent static review of that SHA: no remaining blocking findings after adding lifecycle order, successful fixture output and reused-result assertions.
- Initial run 40370980439 failed to compile the test fixture (`Ulid::new` unavailable); corrected to existing `Ulid::generate`, without weakening assertions.
- Development tests are removed from the delivery head under repository policy; linked commit retains their original source. Production code is unchanged by test removal. Final head CI is a separate required gate, not evidence that those removed tests ran again.
- No local compilation/tests or service restart. Live service behavior remains for user verification.

## Implementation and regression acceptance

Change `openai/sse.rs` at `SseAssembler::absorb`, with a narrowly scoped structured-error classifier. Add development tests for classification and real loopback HTTP/SSE recovery through the provider and agent runtime: timeout then success, exhaustion, unknown error, partial tool arguments then timeout, complete tool proposal then timeout, and a completed prior tool followed by a retry. Assert actual tool execution counts and recorded tool lifecycle, not just parser strings. Run tests only in GitHub Actions. Independent review is required. Per repository delivery policy, preserve test-bearing commit and CI evidence before removing Rust test-only code for any master merge.

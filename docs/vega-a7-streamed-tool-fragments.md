# A7-03 · OpenAI-compatible streamed tool fragment completion

Status: SPEC FROZEN · 2026-09-17 · Owner: E2E tool-call subagent

## Evidence and defect

During a native read-only `README.md` request, CPA/hy3 emitted nine apparent
tool attempts that Vega stored with an empty tool name and `{}` safe input.
The permission gate correctly rejected them as unavailable tools, but this was
downstream of SSE assembly. A content-free structural capture of the same
provider/model showed one `delta.tool_calls[index=0]` start fragment with a
nonempty ID and `function.name = "read"`, followed by argument fragments whose
`function.name` is the empty string. The current assembler replaces the saved
name with that empty continuation value.

This is a bug against S4-T19 and tech spec §4.1: `ToolUse` is emitted only
after the fragments are completely aggregated. An empty continuation field
does not revoke the initial field.

## Contract

1. For a given tool-call index, a nonempty `id` or `function.name` updates the
   saved value; an absent or empty value leaves a previously saved value intact.
   `function.arguments` remains append-only, including empty fragments.
2. At the terminal tool flush, every emitted `ToolUse` has a nonempty ID and
   name. An incomplete call is a sanitized, non-retryable provider protocol
   error, and no calls from that flush reach the tool/permission layer. A
   genuinely unknown but nonempty tool name still follows the existing
   unavailable-tool permission path.
3. Keep index ordering, usage handling, cancellation, and follow-up message
   wire shape unchanged. Do not change the tool definitions, run-mode matrix,
   database schema, credentials, or user project files.

## Acceptance

- Unit replay of the observed start/continuation shape emits exactly one
  `ToolUse` with the original `read` name, original ID, and complete arguments.
- Missing ID/name at flush yields a provider protocol error rather than a
  nameless `ToolUse`; multiple calls flush atomically.
- Loopback HTTP/SSE test traverses the production provider parser; an owned
  temporary repository test traverses the agent's real `read` tool and proves
  a successful one-call observe round without a permission prompt.
- Reverse the nonempty-name guard once and show that the regression fails,
  then restore it. Run focused runtime tests, workspace tests, format, and
  strict clippy under the repository's cargo lock.

## Residual

This fix makes CPA/hy3's observed tool-fragment wire shape executable. Native
installation and live account/network acceptance are separate coordinator
tasks; no live credential is retained in this test suite.

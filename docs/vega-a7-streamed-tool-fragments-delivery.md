# A7-03 · Streamed tool-fragment delivery

## Freeze

- Date: 2026-09-17 (Asia/Shanghai)
- Branch: `feat/a7-hy3-tool-calls`
- Contract: [`vega-a7-streamed-tool-fragments.md`](vega-a7-streamed-tool-fragments.md)
- Source files: `vega_runtime/openai/sse.rs` and provider tests only. No
  provider configuration, keystore, permission matrix, schema, or user data
  was changed.

## Cause and result

The CPA/hy3 start fragment named `read`; later argument fragments carried
`function.name = ""`. Vega's assembler overwrote `read` with the empty
continuation value. The runtime then safely denied an unknown/empty tool,
which produced repeated failed cards instead of reading the requested file.

The assembler now ignores empty ID/name continuation values and checks that
the whole terminal batch has an ID and name before emitting any `ToolUse`.
Unknown **nonempty** tools retain their existing deny behavior. The added
loopback test sends the observed fragmented shape through the real HTTP/SSE
provider and agent, runs the real `read` tool against an owned temporary
`README.md`, then proves the second provider round observes that result and
finishes with one successful tool call.

## Verification

| Evidence | Exact command | Result |
|---|---|---|
| Unit and loopback provider/agent | `scripts/cargo-lock.sh test -p vega_runtime` | 99 passed, 4 pre-existing load-sensitive ignored; doctest 1 passed |
| Workspace | `scripts/cargo-lock.sh test --workspace --quiet` | Exit 0; every suite completed with 0 failures, including the 335-test suite and doctests |
| Format | `cargo fmt --all -- --check` | Exit 0 |
| Strict lint | `scripts/cargo-lock.sh clippy --workspace --all-targets -- -D warnings` | Exit 0; only an upstream future-incompatibility notice for `block` was printed |

Mutation proof: removing the new nonempty-name guard caused
`openai::tests::empty_tool_identity_continuations_preserve_start_fragment`
to fail (0 passed, 1 failed). The guard was restored, and the later full
workspace test passed.

## Residual

This branch does not install a native build. The nine previously persisted
failed calls remain historical records; new native read-only tool runs require
integration, packaging, and fresh UI E2E acceptance by the coordinator.

# A4-C1：bounded ACP v1 runtime

Date: 2026-10-06. Status: [Issue #281](https://github.com/puzige/vega/issues/281), In progress.

This card provides Vega's headless ACP v1 transport/runtime for a user-configured `codex-acp` executable. It is a foundation for the complete A4 task flow; it does not make A4 complete by itself. The end-to-end product contract remains in [the A4 v1 specification](vega-acp-codex-v1-spec.md).

## User-visible goal

Vega can start a configured ACP agent as a subprocess, negotiate stable protocol v1, create or restore a session, send a prompt, receive updates and permission requests, answer the exact offered permission option, cancel a running prompt, and observe the prompt's actual terminal response. The caller owns the child and session lifecycle.

The card does not add Vega task persistence, New Task UI, settings, or app-level routing. A4 is not complete until later cards connect this runtime to the task lifecycle and pass the full product acceptance matrix.

## Frozen implementation boundary

The new `vega_acp` crate is headless and owns ACP JSON-RPC types, stdio framing, child-process lifecycle, request correlation, negotiated capabilities, session operations, update events, permission request IDs and option IDs, and protocol errors. ACP wire JSON does not cross into `vega_conversation` or `vega_ui` in later cards; those crates receive business projections.

The crate exposes one owned `Connection` per launched adapter process. Its public operations cover `initialize`, `session/new`, `session/load`, `session/resume`, `session/set_mode`, `session/prompt`, `session/cancel`, exact permission responses, event receipt, and orderly shutdown. `session/prompt` returns a completion handle whose result is the protocol terminal response. Sending `session/cancel` is not itself completion.

The caller provides an absolute executable path, argument vector, absolute launch directory, and a minimal inherited environment. The runtime rejects a relative executable or directory before spawning. It inherits only `HOME`, `PATH`, `LANG`, `TMPDIR`, and `USER` when present; it never injects Vega provider credentials, MCP servers, Skills, or `APP_SERVER_LOGS`. The runtime does not download or install an adapter. The user-configured adapter is responsible for Codex authentication.

Vega sends only explicitly implemented client capabilities. For this card that means no filesystem, terminal, elicitation, or image capability. Text prompt content is supported. Client permission answers preserve ACP's original option ID, title, and order; no ACP choice is mapped to Native `Always` permissions.

## Transport and resource contract

ACP v1 stdio is UTF-8 JSON-RPC 2.0 with one frame per newline-delimited line. A frame is one JSON-RPC object or one non-empty JSON-RPC batch array, and MUST NOT contain embedded newlines. The runtime preserves a batch as one frame, validates its complete shape and element count before dispatch, and emits one response array for response-bearing batch entries; it never flattens, reorders, or partially dispatches a batch. Client-originated requests and notifications remain individual messages.

| Resource | Frozen limit | Overflow behavior |
|---|---:|---|
| Incoming or outgoing JSON-RPC line, excluding LF | 1,048,576 bytes | Fail the connection, resolve pending operations as transport failure, and terminate the owned child |
| JSON-RPC batch elements per line | 16 | Fail the connection before dispatching any element |
| Read scratch buffer | 8,192 bytes | Reused; no unbounded `read_until` or `read_to_end` |
| Adapter argument count / aggregate encoded argument bytes | 128 / 65,536 bytes | Reject configuration before process creation |
| Outbound requests awaiting responses | 16 per connection | Reject the next request before writing it |
| Unanswered inbound requests | 16 per connection | Fail the connection; do not silently drop a permission request |
| Seen inbound JSON-RPC request IDs | 4,096 IDs and 1,048,576 UTF-8 identifier bytes per connection lifetime | Never evict completed IDs; fail the connection with `RequestIdHistoryFull` before admitting a new ID beyond either limit |
| Buffered events | 128 events and 4,194,304 serialized bytes per connection | Fail the connection; do not drop text, permission, update, or terminal events |
| Weight per queued event | 4,096 serialized bytes per permit, rounded up | Event admission fails closed if the byte budget is exhausted |
| Retained stderr | 0 bytes | Drain with the fixed 8,192-byte scratch buffer and discard; never log its contents |
| Retained raw protocol frame after dispatch | 0 bytes | Decode one bounded frame, then release it after dispatch |

The frame reader checks the limit before appending bytes. The event queue is bounded by both item count and byte permits held by each queued event. Full queues cause a typed visible connection failure and owned-child cleanup; they never drop an event and continue. Only one frame is decoded at a time per connection. JSON parsing uses `serde_json`'s default recursion limit. No unbounded channel is permitted in this crate.

Unknown, duplicate, or already-completed wire request IDs fail closed. The runtime retains inbound wire IDs for the connection lifetime within the count and byte budgets above; once either budget is full, the next unique inbound request terminates the connection rather than evicting an ID. Replaying an active or completed inbound wire ID fails with `DuplicateRequestId`; replying again through an already-completed local permission responder returns `PermissionRequestClosed`; replaying a response for a completed outbound request fails with `UnknownRequestId`. Connection termination completes all outbound request waiters and permission responders with a sanitized typed error. Error values exposed outside the crate contain a category and safe display text only, never raw prompt, workspace contents, environment values, stderr, or credentials.

## Acceptance matrix

| ID | Requirement | In-process test evidence | Status |
|---|---|---|---|
| C1-01 | Invalid executable/cwd and over-limit launch arguments are rejected before process creation | Launch validation covers the exact argument-count and aggregate-byte limits plus a spawn observer | PASS |
| C1-02 | `initialize` requests stable v1; an unsupported protocol version prevents session creation; returned capabilities are represented exactly | Duplex protocol tests | PASS |
| C1-03 | Session new/load/resume and workspace-write mode preserve exact paths, session ID, and response values | Scripted in-memory peer | PASS |
| C1-04 | Prompt updates are delivered in order and completion reports the returned stop reason | Multi-message peer script and completion handle | PASS |
| C1-05 | Cancel sends the protocol notification but remains stopping until the original prompt completion arrives | Cancel/prompt lifecycle test | PASS |
| C1-06 | Permission labels, order, and raw option IDs survive projection; one request receives at most one response | Exact-option, completed wire-ID replay, and stale responder tests | PASS |
| C1-07 | Concurrent request responses may arrive out of order and resolve only their matching waiters | Correlation test and 16-pending boundary | PASS |
| C1-08 | Malformed UTF-8/JSON, missing LF, oversized frame, empty/over-limit/malformed batch, unknown/completed IDs, EOF, closed peer, and bounded ID-history overflow fail visibly without panic or partial dispatch | Incoming/outgoing frame boundaries, duplicate IDs, 4,096-ID and 1 MiB history limits, and transport tests | PASS |
| C1-09 | Item-count, event-byte-budget, pending-permission, and batch-element limits fail closed without dropping an event or growing a queue | 128/129 events, four/five large queued events, 16/17 permissions, 16-entry response-array ordering, and 17-entry rejection | PASS |
| C1-10 | Stderr is continuously drained without retaining/logging its contents | Injected reader test asserting zero retained bytes | PASS |
| C1-11 | Shutdown affects only this connection's child and resolves pending work as interrupted/unknown | Owned-child lifecycle tests using the in-process process seam | PASS |

All automated tests are in-process and use a scripted ACP peer over Tokio duplex I/O or an injected process seam. Do not add a test that launches a real external executable or uses network access. This follows the repository's #149 test policy; a real `codex-acp` run is reserved for the later user-authorized integration acceptance.

## Implementation plan and ownership

1. Add `crates/vega_acp` and register it in the Cargo workspace. Use only existing approved dependencies: `serde`, `serde_json`, `thiserror`, `tokio`, `tokio-util`, and `tracing`; do not add `agent-client-protocol` or any other dependency.
2. Implement bounded newline framing and JSON-RPC validation over generic Tokio I/O, then request correlation, inbound request handling, and the byte/item-bounded event stream.
3. Add the owned subprocess launcher, strict environment allowlist, stderr drain/discard, and cancellation-safe shutdown.
4. Add ACP v1 initialization/session/prompt/mode/cancel/permission operations and the focused in-process acceptance tests above.
5. Add a short delivery record with exact targeted nextest command/output and any unrun acceptance rows.

Do not edit `vega_conversation`, `vega_store`, `vega`, or `vega_ui` in this card. Do not add settings or assume how an existing Vega task stores ACP identity; those shared business contracts belong to the integration card and must have a single owner.

## Local verification and handoff

Run only the card's targeted tests, for example `cargo nextest run -p vega_acp`; do not run workspace-wide tests locally. The cloud `check (fmt, clippy, test)` remains the full gate. The real Codex adapter, sign-in, workspace edits, Git Diff, and native UI remain NOT RUN by this foundation card.

Official transport sources: [ACP v1 transports](https://agentclientprotocol.com/protocol/v1/transports) and the [ACP Rust SDK transport framing contract](https://github.com/agentclientprotocol/rust-sdk/blob/v2.2.0/md/transport-architecture.md). Official session lifecycle sources: [session setup](https://agentclientprotocol.com/protocol/v1/session-setup), [prompt turn](https://agentclientprotocol.com/protocol/v1/prompt-turn), and [cancellation](https://agentclientprotocol.com/protocol/v1/cancellation).

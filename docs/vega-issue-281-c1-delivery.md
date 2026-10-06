# Issue #281 / A4-C1 delivery record

## Freeze

- Contract: [A4-C1 bounded ACP v1 runtime](vega-acp-codex-c1-runtime.md)
- Product scope: [A4 Codex task v1](vega-acp-codex-v1-spec.md)
- Evidence class: in-process scripted peer / Tokio duplex transport
- Status: implemented; ready for main-agent review
- Spec corrections: corrected the stale batch-array rejection statement to match the frozen <=16 element batch contract; updated the feature list and A4 module description to say Vega uses its own bounded headless stdio runtime without the ACP Rust SDK; added fixed inbound request-ID history limits. The local PRD copy was left unchanged because the repository workflow makes Notion authoritative.

## Acceptance cases

| ID | Case | Evidence | State |
|---|---|---|---|
| C1-01 | Launch validation | Exact argument count/byte boundaries and a spawn observer | PASS |
| C1-02 | Stable v1 initialization | Handshake, capabilities, and unsupported version scripted-peer tests | PASS |
| C1-03 | Session operations | New/load/resume, paths, IDs, and mode request/response tests | PASS |
| C1-04 | Prompt lifecycle | Ordered updates and terminal stop reason test | PASS |
| C1-05 | Cancel lifecycle | Cancel notification remains distinct from prompt completion | PASS |
| C1-06 | Permission projection | Original options, exactly-once response, stale responder, and reused wire ID tests | PASS |
| C1-07 | Correlation and pending limit | Out-of-order response mapping and 16/17 outbound request boundary | PASS |
| C1-08 | Framing and IDs | UTF-8/JSON/EOF/batch failures, exact inbound/outbound 1 MiB line limits, completed IDs, and 4,096-ID/1 MiB history limits | PASS |
| C1-09 | Bounded queues and batches | 128/129 events, byte budget, 16/17 permission requests, batch boundary and response-array order | PASS |
| C1-10 | Stderr | Fixed-buffer drain/discard seam retains zero bytes | PASS |
| C1-11 | Shutdown | In-process owned-child seam and pending request completion | PASS |

## Results

Commands are package-scoped; no workspace-wide test, format, or clippy command was run.

| Command | Exit | Result |
|---|---:|---|
| `CARGO_NET_OFFLINE=true cargo nextest run -p vega_acp` | 0 | 25 passed, 0 failed |
| Debug-only `cargo test -p vega_acp c1_09_event_byte_budget_overflow_fails_without_dropping_queued_events -- --nocapture` | 101, then 0 | First run exposed a timing assumption in the test; it now waits for the terminal signal before checking retained permits. Final acceptance uses the Nextest command above. |
| `cargo fmt -p vega_acp` | 0 | Formatted package sources |
| `cargo fmt -p vega_acp -- --check` | 0 | Clean |
| `CARGO_NET_OFFLINE=true cargo clippy -p vega_acp --all-targets -- -D warnings` | 0 | Clean with warnings denied |
| `git diff --check` | 0 | Clean |

Branch: `feat/281-acp-runtime`. Base: `origin/master` at `d9dceccd287fce0e28566e349fb89377143ad43a`. Final HEAD is recorded in the handoff message.

Changed files:

- `Cargo.toml`, `Cargo.lock`
- `crates/vega_acp/Cargo.toml`
- `crates/vega_acp/src/lib.rs`, `error.rs`, `framing.rs`, `protocol.rs`, `connection.rs`, `tests.rs`
- `docs/vega-acp-codex-v1-spec.md`, `docs/vega-acp-codex-c1-runtime.md`, `docs/vega-acp-codex-research.md`
- `docs/vega-features.md`
- `docs/vega-issue-281-c1-delivery.md`

## Residuals and risks

- No real `codex-acp` process, network access, login, workspace edit, native UI, or full A4 task flow was exercised. The acceptance evidence uses only scripted in-process peers and an injected child-control seam. The integration acceptance must pin official `codex-acp` v2.0.0 and its bundled Codex 0.158.0; no compatibility claim is made for the local CLI 0.157.0.
- The runtime retains inbound wire request IDs exactly so it can reject reuse after completion. To keep this history bounded, the connection fails closed after 4,096 unique IDs or 1 MiB of UTF-8 ID bytes; this lifecycle limit is part of the updated C1 contract.
- Conversation/store/UI integration, task persistence, settings, and end-to-end acceptance remain for later A4 cards.
- No contract deviation remains known.

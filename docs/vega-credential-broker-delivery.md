# Credential Broker implementation — Issue #378

## Contract and evidence

Implements `vega-credential-broker-design.md` under the user's authorization for the complete design. Local verification is restricted to formatting and static review. All Rust compilation and tests are NOT RUN locally and must execute in PR CI. Real TLS, Seatbelt, SDK and UI integration remains user acceptance; in-process tests do not prove operating-system isolation.

## Acceptance matrix (defined before implementation)

| ID | Requirement / risk | Setup and operation | Observable result | Evidence | Status |
| --- | --- | --- | --- | --- | --- |
| CB01 | Scoped aliases | Register synthetic secret in two environments; use foreign alias | Only owning environment injects | Unit | Pending |
| CB02 | Target authorization | Vary scheme, host, port, path, header, redirect | Exact allowed HTTPS target only; no automatic auth | Unit | Pending |
| CB03 | Rotation/revocation | Rotate reader value or revoke lease before injection | Old lease cannot inject | Unit | Pending |
| CB04 | Controlled environment | Build shell environment with secret and startup variables | Clean allowlist, only dummy credentials, no login startup | Unit + user OS acceptance | Pending |
| CB05 | HTTPS proxy | CONNECT then authenticated request | Local CA only, upstream TLS checked, auth injected only at exit | In-process parsing/security + user TLS acceptance | Pending |
| CB06 | Isolation | Read credential root, direct network, process inspection | Restricted execution rejects; Full access makes no isolation claim | Policy unit + user Seatbelt acceptance | Pending |
| CB07 | Echo / streaming | Split known secret across response chunks | Secret never returned across chunk boundaries | Unit | Pending |
| CB08 | Model projection | User/tool/reasoning/schema with known secrets | Valid safe request, original unchanged, tool IDs paired | Provider boundary | Pending |
| CB09 | False positives | Rust password field, ordinary literals | Low confidence text preserved; precise secrets still filtered | Unit | Pending |
| CB10 | Legacy recovery | Completed tool, contaminated history, continue | Same turn continues with persisted result, no duplicate side effect | Runtime/store | Pending |
| CB11 | Unavailable authority | Credential reader fails | Distinct actionable failure, no dispatch | Provider boundary | Pending |
| CB12 | Retry race | Rotate secret after initial projection | Retry rechecks, no unsafe request | Provider boundary | Pending |
| CB13 | Trusted clients | Provider/remote MCP credential path | Real value confined to authenticated transport | Static chain + existing regression | Pending |
| CB14 | Persistence | Restart with old alias/config | No grant restored by alias alone; original records retained | Unit/store | Pending |
| CB15 | UI recovery | Blocked check/config unavailable | Reason-specific notice and actionable existing recovery | UI/unit + user acceptance | Pending |

## Implementation plan

1. Separate model projection from source records and remove historical-cleanup forced abort; classify unknown formats and preserve low-confidence source code.
2. Add a headless Broker with trusted bindings, environment leases, alias ownership, scoped injection, revoke/rotation checks and bounded response filtering.
3. Add local authenticated CONNECT/TLS proxy and bind it to tool process lifetime, controlled environment, scoped CA trust and Seatbelt policy. Fail closed without real-value fallback.
4. Wire owner configuration, Provider/MCP trusted boundaries, file path guards and failure reporting; retain existing permission decisions.
5. Add in-process regression cases listed above. Run `cargo fmt --all -- --check` and `git diff --check` locally. CI runs `cargo clippy --workspace --all-targets -- -D warnings` and `cargo nextest run --workspace`.

## Compatibility and rollback

Bindings contain key references and destinations only, never secret values. Existing owner credential storage remains unchanged. No source history is replayed by projection. Rollback reverts this issue's changes; no credential values or CA private keys are persisted by Broker.

## Results

Implementation in progress. No compilation or tests have run. Dependencies for TLS require architect approval before addition.

## First implementation milestone

Architect approved `vega_credentials`, rustls `=0.23.43`, tokio-rustls `=0.26.4`, rcgen `=0.14.7` (ring/pem, default features off), httparse `=1.10.1`; existing reqwest/base64/ring/serde/tokio reused. Lockfile resolved incrementally; existing package versions retained.

Production paths now wired: owner `credential_bindings` → `app_agent` → `ToolCredentialPolicy` → Bash lease/child environment/HTTPS proxy; local MCP settings slots → explicitly matching binding → protected stdio subprocess; Provider boundary → safe request copy → fresh final reader check. Remote MCP and model API authentication retain trusted clients.

Proxy compatibility: HTTPS HTTP/1 header authentication; one request per connection; 32 KiB headers, 8 MiB request/response body, 16 concurrent connections, 120-second connection lifetime. Responses are buffered before returning, including chunked upstream responses, so secrets crossing chunks are filtered. This is not incremental SSE streaming. Compression other than identity, upgrades, chunked request bodies and non-HTTP protocols fail closed. Redirects are returned to the client and never followed by the proxy. Only explicitly bound public HTTPS destinations are reachable while a credential lease is active. Real TLS/Seatbelt compatibility remains NOT RUN.

Credential-enabled shell uses clean environment and temporary HOME; ordinary shell without bindings retains its existing environment/startup compatibility. Local MCP with credentials now requires an explicit destination binding rather than silently forwarding its stored secret. Its error identifies missing binding; local MCP without secret slots keeps ordinary network access but its process cannot read/write the owner configuration root.

`cargo fmt --all -- --check`: exit 0. `git diff --check`: exit 0. All Rust compilation, Clippy and nextest: NOT RUN locally. Durable continuation and UI recovery are being implemented in a separately owned worktree and are not included in this milestone. First milestone is not a completion or merge claim.

Trusted configuration example (synthetic reference; never put the secret here):

```toml
[[credential_bindings]]
key_ref = "github-tools"
env = "GITHUB_TOKEN"
origin = "https://api.github.com"
header = "authorization"
prefix = "Bearer "
path = "/repos"
```

Bindings permit all HTTP methods within the configured path boundary; tool permission approval remains independent. Rotating/deleting the referenced secret or changing the binding configuration invalidates existing leases. Stored MCP slot references must match both `key_ref` and `env` in the trusted binding. The tool cannot register a binding through its own environment.

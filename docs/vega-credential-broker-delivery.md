# Credential Broker implementation — Issue #378

## Contract and evidence

Implements `vega-credential-broker-design.md` under the user's authorization for the complete design. Local verification is restricted to formatting and static review. All Rust compilation and tests are NOT RUN locally and must execute in PR CI. Real TLS, Seatbelt, SDK and UI integration remains user acceptance; in-process tests do not prove operating-system isolation.

## Acceptance matrix (implementation and verification status)

| ID | Requirement / risk | Setup and operation | Observable result | Evidence | Status |
| --- | --- | --- | --- | --- | --- |
| CB01 | Scoped aliases | Register synthetic secret in two environments; use foreign alias | Only owning environment injects | Broker/redaction unit regression | Implemented; CI pending |
| CB02 | Target authorization | Vary scheme, host, port, path, header, redirect | Exact allowed HTTPS target only; no automatic auth | Broker/redaction unit regression | Implemented; CI pending |
| CB03 | Rotation/revocation | Rotate reader value or revoke lease before injection | Old lease cannot inject | Broker/redaction unit regression | Implemented; CI pending |
| CB04 | Controlled environment | Build shell environment with secret and startup variables | Clean allowlist, only dummy credentials, no login startup | Unit + user OS acceptance | Implemented; CI pending |
| CB05 | HTTPS proxy | CONNECT then authenticated request | Local CA only, upstream TLS checked, auth injected only at exit | In-process parsing/security + user TLS acceptance | Implemented; CI pending |
| CB06 | Isolation | Read credential root, direct network, process inspection | Restricted execution rejects; Full access makes no isolation claim | Policy unit + user Seatbelt acceptance | Implemented; CI pending |
| CB07 | Echo / streaming | Split known secret across response chunks | Secret never returned across chunk boundaries | Broker/redaction unit regression | Implemented; CI pending |
| CB08 | Model projection | User/tool/reasoning/schema with known secrets | Valid safe request, original unchanged, tool IDs paired | Provider boundary | Implemented; CI pending |
| CB09 | False positives | Rust password field, ordinary literals; explicit rule-scoped exception | Low confidence text preserved; known values never exempt | Detector/policy and privacy service regression | Implemented; CI pending |
| CB10 | Legacy recovery | Completed tool, contaminated history, continue | Same turn continues with persisted result, no duplicate side effect | Runtime/store | Implemented; CI pending |
| CB11 | Unavailable authority | Credential reader fails | Distinct actionable failure, no dispatch | Provider boundary | Implemented; CI pending |
| CB12 | Retry race | Rotate secret after initial projection | Retry rechecks, no unsafe request | Provider boundary | Implemented; CI pending |
| CB13 | Trusted clients | Provider/remote MCP credential path | Real value confined to authenticated transport | Static chain + existing regression | Implemented; CI pending |
| CB14 | Persistence | Restart with old alias/config | No grant restored by alias alone; original records retained | Unit/store | Implemented; CI pending |
| CB15 | UI recovery | Blocked check/config unavailable | Reason-specific notice and actionable existing recovery | UI/unit + user acceptance | Implemented; CI pending |

## Implementation plan

1. Separate model projection from source records and remove historical-cleanup forced abort; classify unknown formats and preserve low-confidence source code.
2. Add a headless Broker with trusted bindings, environment leases, alias ownership, scoped injection, revoke/rotation checks and bounded response filtering.
3. Add local authenticated CONNECT/TLS proxy and bind it to tool process lifetime, controlled environment, scoped CA trust and Seatbelt policy. Fail closed without real-value fallback.
4. Wire owner configuration, Provider/MCP trusted boundaries, file path guards and failure reporting; retain existing permission decisions.
5. Add in-process regression cases listed above. Run `cargo fmt --all -- --check` and `git diff --check` locally. CI runs `cargo clippy --workspace --all-targets -- -D warnings` and `cargo nextest run --workspace`.

## Compatibility and rollback

Bindings contain key references and destinations only, never secret values. Existing owner credential storage remains unchanged. No source history is replayed by projection. Rollback reverts this issue's changes; no credential values or CA private keys are persisted by Broker.

## Results

Core Broker, model boundary, protected tool/stdin MCP execution and durable continuation are implemented. The privacy review/exception/display controls are integrated from the dedicated privacy worktree. Dependencies are approved below. No local compilation or tests have run; PR #380 CI is the compilation/test authority.

## First implementation milestone

Architect approved `vega_credentials`, rustls `=0.23.43`, tokio-rustls `=0.26.4`, rcgen `=0.14.7` (ring/pem, default features off), httparse `=1.10.1`; existing reqwest/base64/ring/serde/tokio reused. Lockfile resolved incrementally; existing package versions retained.

Production paths now wired: owner `credential_bindings` → `app_agent` → `ToolCredentialPolicy` → Bash lease/child environment/HTTPS proxy; local MCP settings slots → explicitly matching binding → protected stdio subprocess; Provider boundary → safe request copy → fresh final reader check. Remote MCP and model API authentication retain trusted clients.

Proxy compatibility: HTTPS HTTP/1 header authentication; one request per connection; 32 KiB headers, 8 MiB request/response body, 16 concurrent connections, 120-second connection lifetime. Responses are buffered before returning, including chunked upstream responses, so secrets crossing chunks are filtered. This is not incremental SSE streaming. Compression other than identity, upgrades, chunked request bodies and non-HTTP protocols fail closed. Redirects are returned to the client and never followed by the proxy. Only explicitly bound public HTTPS destinations are reachable while a credential lease is active. Real TLS/Seatbelt compatibility remains NOT RUN.

Credential-enabled shell uses clean environment and temporary HOME; ordinary shell without bindings retains its existing environment/startup compatibility. Local MCP with credentials now requires an explicit destination binding rather than silently forwarding its stored secret. Its error identifies missing binding; local MCP without secret slots keeps ordinary network access but its process cannot read/write the owner configuration root.

`cargo fmt --all -- --check`: exit 0. `git diff --check`: exit 0. All Rust compilation, Clippy and nextest: NOT RUN locally. Durable continuation and UI recovery were subsequently integrated; see the integration update and `vega-credential-continuation-delivery.md`. No completion or merge claim follows from formatting alone.

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

## Integration update

Durable continuation work is integrated separately (receipt claim/CAS, source fingerprint, original user association, terminal tool outcome checks, recovery/settings actions). The application worker now routes that action through the dedicated resume entry; it does not submit a new user instruction.

Full access policy: a configured credential execution requires protected mode and returns `credential_protection_required` before starting a lease in Full access. Full access without a credential lease retains ordinary network and external-project file access; a minimal internal-resource Seatbelt policy still denies access to Vega's owner configuration root and process inspection. This is not a claim that Full access is a fully isolated environment. Owner configuration protection prevents agent tools from rewriting a destination grant and obtaining broader authority on the next run.

Known-secret response filtering now covers raw, JSON-escaped and percent-encoded forms, longest first. Arbitrary encodings, external user backups, terminal plugins and user-installed host processes remain outside that guarantee. File reads and grep use descriptor-relative no-follow traversal, while atomic writes use a captured directory descriptor for creation and rename.


## Current production/test mapping

- `vega_credentials::CredentialBroker` is invoked by `vega_tools::ToolCredentialPolicy` and `vega_mcp::stdio::StdioClient`, not an isolated fixture. Tests `scoped_aliases_targets_rotation_and_revocation`, `no_alias_does_not_add_or_override_authentication`, and `only_active_registered_aliases_are_recognized` exercise scope, explicit header selection and invalidation.
- `vega_credentials::proxy` terminates local TLS and uses a verified, DNS-pinned upstream reqwest client. `ambiguous_framing_and_unsupported_protocols_are_rejected` and `authority_and_path_confusion_are_rejected` cover parser gates. These are not a live TLS/SDK or macOS sandbox test.
- `OwnerCredentialProvider` is installed by `app_agent` for normal, approved-plan and credential-continuation runs. It projects safe request copies; transport retry guards reread owner values. `credential_guard` regressions cover schema semantic rejection, original-request immutability, authority failure and rotated-value checks. Model Text/Thinking/Summary streams retain a bounded suffix across chunks before publication; `split_owner_value_never_reaches_visible_events` covers interleaved channels.
- `RuntimeToolConfig` filters tool results; summary requests/results and legacy context cleanup share the policy. Detector metadata reports rule, byte range and confidence without values. `low_assignment_does_not_hide_nested_high_token` and `exceptions_never_release_owner_values_and_ranges_are_utf8_boundaries` cover nested formats and non-overridable known values.
- `credential_continuation` migration/store claims, context-prefix fence, actor durable ACKs and the dedicated resume entry are integrated. Restart/claim/source-fence regressions reside in store and conversation continuation tests. Running/unknown tool outcomes do not resume automatically. The UI exposes Continue and credential-settings actions with accessible labels.

Remaining verification is cloud compilation/Clippy/nextest plus macOS Seatbelt and real synthetic TLS/SDK/UI acceptance. Privacy UI/history/search/copy/export and explicit cleanup integration is detailed in `vega-credential-privacy-delivery.md`. Transaction CAS, authority changes and exception revocation invalidate stale review/filter state. No real credential, user database or installed application was inspected by this implementation.


MCP settings exposes **Copy API authorization configuration template** for saved local servers. The trusted service fills the actual opaque key references and environment names without reading secret values into the template; the API origin is deliberately empty and must be filled by the user before a binding can validate. Missing-binding errors point to this action. Model/runtime exceptions are loaded per run; every owner authority check refreshes the epoch so rotation invalidates a stale exception snapshot. Exact known secrets remain non-overridable.

Manual context compaction now uses the same complete owner authority and protected model stream wrapper, including rule exceptions; queued steering messages retain the same policy. Diagnostic response request IDs are projected before leaving the Provider boundary.

# Issue #171 — Run diagnostics delivery record

**Status:** implementation in progress; Issue remains OPEN  
**Branch:** `feat/171-run-diagnostics-implementation`  
**Base:** current `origin/master` (includes #146)  
**Spec:** [Run diagnostics implementation contract](vega-issue-171-run-diagnostics-spec.md)  
**Implementation commits:** pending  
**Pull request:** pending

## Scope

- Add append-only, content-free run diagnostic lifecycle records with `run_id`, `attempt_id`, parent linkage, phase, state, UTC timestamps, duration and safe provider/tool metadata.
- Split summary failure types without changing the existing coarse status UI contract.
- Persist via an independent bounded best-effort writer. A dropped/failed diagnostic write cannot change the agent's normal result or weaken required transcript/tool/usage persistence.
- Add typed local read APIs and explicitly invoked allowlist export API; no viewer UI, automatic export, real provider, user DB or credential access.
- Request IDs are accepted only from `x-request-id`, `request-id`, `openai-request-id` and only when the value matches the frozen ASCII/length constraints.

## Acceptance matrix

| ID | Requirement / risk | Test setup and operation | Observable result | Evidence | Status |
|---|---|---|---|---|---|
| D171-01 | Schema migration and constrained event writes | Temporary SQLite; migrate v14→v15; append valid and invalid lifecycle events | Valid closed-vocabulary rows persist; bad enum/identity/count rejected; old migrations unchanged | Pending | NOT RUN |
| D171-02 | Ordered reads and restart recovery | Append start/terminal and an incomplete start; close and reopen temp DB | Same events returned by thread/run in append order; unfinished attempt is explicitly incomplete | Pending | NOT RUN |
| D171-03 | Provider metadata and retry count | MockProvider/in-process metadata fixture returns safe status, request ID and retries | Typed metadata recorded; invalid header/value becomes NULL; unknown retry count remains NULL | Pending | NOT RUN |
| D171-04 | Provider failure classification | Inject HTTP, transport and malformed-stream typed errors | Closed categories differ without inspecting raw error text | Pending | NOT RUN |
| D171-05 | Summary outcomes | Mock Summary stream for timeout, length, empty, malformed framing, invalid projection and provider failures | Each outcome maps to its distinct diagnostic code; existing UI status remains compatible | Pending | NOT RUN |
| D171-06 | Tool and later-stage correlation | Mock one successful tool followed by a failing model attempt under one run | Tool success and later failure are ordered and share run ID; tool call ID joins existing record | Pending | NOT RUN |
| D171-07 | Diagnostic write isolation | Inject full/closed writer and a diagnostic-only DB write failure while required store is healthy | Agent outcome and required persistence are unchanged; no diagnostic error reaches user-facing run failure | Pending | NOT RUN |
| D171-08 | Privacy canaries | Put fake key, Authorization, prompt, response, reasoning, summary and tool-output canaries in test fixtures | No canary appears in diagnostic DB projection, Debug/Display, tracing fields or export JSON | Pending | NOT RUN |
| D171-09 | Export boundary | Explicitly call export API for one run and inspect serialized fields | Versioned allowlist JSON only; no transcript/table join or automatic file/network action | Pending | NOT RUN |
| D171-10 | Existing critical persistence contract | Inject an existing required `PersistenceActor` failure | Run still reports the established required-persistence error; best-effort diagnostics do not mask it | Pending | NOT RUN |

## Implementation plan

1. Freeze this spec and matrix on the implementation branch before changing Rust production files.
2. Add `vega_store` migration/API and store-level validation/read/reopen tests.
3. Add typed runtime provider failure/attempt metadata and distinct summary error types; preserve raw payload redaction in Debug/Display and existing coarse UI status mappings.
4. Add conversation-layer diagnostic lifecycle IDs and an independent bounded best-effort SQLite writer; project run/provider/summary/tool lifecycle without passing raw content.
5. Add typed read/export APIs and cross-crate integration tests for correlation, failure isolation and canary absence.
6. Run only affected-crate targeted Nextest filters, `cargo fmt --all -- --check`, affected-crate Clippy with `-D warnings`, and `git diff --check`. Do not run workspace-wide local tests.
7. Record exact commands, exit codes, concise raw output, diff hash, deviations and residuals here; commit in small stages (at most three implementation commits), push branch and open a PR. Do not merge or close the Issue; retain worktree for desktop acceptance.

## Verification evidence

No production changes or tests have run yet. The spec/discovery phase only checked repository sources and used `git diff --check`.

| Command | Exit | Result / raw output excerpt |
|---|---:|---|
| `cargo nextest run -p vega_store ...` | Pending | NOT RUN |
| `cargo nextest run -p vega_runtime ...` | Pending | NOT RUN |
| `cargo nextest run -p vega_conversation ...` | Pending | NOT RUN |
| affected-crate `cargo clippy --all-targets -- -D warnings` | Pending | NOT RUN |
| `cargo fmt --all -- --check` | Pending | NOT RUN |
| `git diff --check` | Pending | NOT RUN |

## Privacy boundary and residuals

- Tests use MockProvider, in-process metadata fixtures, temporary SQLite, deterministic identifiers, fake credentials and canary strings only.
- Diagnostics never store API keys, Authorization values, request/response bodies, prompt/history, visible or reasoning response text, summary text, complete tool input/output, filesystem contents, endpoint URLs or arbitrary headers.
- No user database, real credentials, external provider, live HTTP service, app install or native UI was accessed for implementation verification.
- Real desktop/UI acceptance is **NOT RUN** and remains for product acceptance after the PR is reviewed.
- Check results and PR URL: pending.

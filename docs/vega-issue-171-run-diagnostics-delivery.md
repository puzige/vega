# Issue #171 — Run diagnostics delivery record

**Status:** implementation and local verification complete; Issue remains OPEN
**Branch:** `feat/171-run-diagnostics-implementation`
**Base:** `db97c5d` (`origin/master`, includes #146 and #64)
**Spec:** [Run diagnostics implementation contract](vega-issue-171-run-diagnostics-spec.md)
**Implementation commit:** `d136bc7 feat(#171): persist safe run diagnostics`
**Pull request:** [#217](https://github.com/puzige/vega/pull/217) — OPEN, not merged
**PR base:** `master` (`db97c5d2d5961c33a52df276f157be4836c2c9f9`)
**PR head:** `feat/171-run-diagnostics-implementation` (this documentation-only follow-up advances the branch head)
**Local verification:** affected-crate targeted Nextest, fmt, `cargo check`, Clippy with `-D warnings`, and `git diff --check` passed; no Rust files changed in this follow-up.

## Scope

- Add append-only, content-free run diagnostic lifecycle records with `run_id`, `attempt_id`, parent linkage, phase, state, UTC timestamps, duration and safe provider/tool metadata.
- Split summary failure types without changing the existing coarse status UI contract.
- Persist via an independent bounded best-effort writer. A dropped/failed diagnostic write cannot change the agent's normal result or weaken required transcript/tool/usage persistence.
- Add typed local read APIs and explicitly invoked allowlist export API; no viewer UI, automatic export, real provider, user DB or credential access.
- Request IDs are accepted only from `x-request-id`, `request-id`, `openai-request-id` and only when the value matches the frozen ASCII/length constraints.

## Acceptance matrix

| ID | Requirement / risk | Test setup and operation | Observable result | Evidence | Status |
|---|---|---|---|---|---|
| D171-01 | Schema migration and constrained event writes | Temporary SQLite; migrate v14→v15; append valid and invalid lifecycle events | Valid closed-vocabulary rows persist; unsafe IDs and invalid terminal shape rejected; current schema has 26 tables | `cargo nextest run -p vega_store run_diagnostics` (5/5); migration table-count filters (1/1 each) | PASS |
| D171-02 | Ordered reads and restart recovery | Append start/terminal and incomplete start; close/reopen temp DB; close writer sender and wait for drain | Ordered events survive reopen; incomplete attempts remain visible; graceful last-sender close drains terminal record | Store reopen test (1/1); conversation writer drain/reopen test (1/1) | PASS |
| D171-03 | Provider metadata and retry count | In-process HTTP fixtures return allowlisted and invalid response IDs and actual retry counts | Typed status/request ID/retry count are projected; invalid IDs are omitted; unknown metadata remains absent | `cargo nextest run -p vega_runtime openai::tests::` (41/41), including allowlist and retry metadata cases | PASS |
| D171-04 | Provider failure classification | Inject typed HTTP, transport and protocol failures, plus rejected response | Closed categories differ without parsing diagnostic text; Debug/Display hide error body and request-ID values | Runtime error/provider/OpenAI filters (5/5, 5/5, 41/41); primary metadata-retention test (1/1) | PASS |
| D171-05 | Summary outcomes | Mock summary stream, expired deadline, oversized input, empty/malformed output and typed provider failures | Timeout, truncation, empty, framing, projection/source and known preflight failures map to safe typed codes; over-budget preflight never calls provider | `cargo nextest run -p vega_conversation agent::compaction::` (19/19); preflight provider-call assertion and timeout persistence cases included | PASS |
| D171-06 | Tool and later-stage correlation | Mock successful tool then failing model attempt under one run | Ordered records share run ID and validated tool call ID; provider failure receives its typed code | Conversation correlation integration test (1/1); normal tool lifecycle test (1/1) | PASS |
| D171-07 | Diagnostic write isolation | Fill/close queue; install diagnostic-only SQLite ABORT trigger while required store remains healthy | Calls remain non-blocking/error-free; run result and required assistant message persist despite diagnostic INSERT failure | Queue isolation (1/1); SQL trigger isolation integration test (1/1) | PASS |
| D171-08 | Privacy canaries | Use fake key, Authorization, prompt, response, reasoning, summary, provider error and tool-output canaries | Canaries absent from diagnostic export and custom error/metadata Debug/Display; only safe counts/status/request ID allowlist survive | Runtime OpenAI redaction suite (41/41), summary truncation/export test (1/1), run/tool export assertions (1/1), provider format canaries (2/2) | PASS |
| D171-09 | Export boundary | Explicitly call export API for one run and inspect serialized fields | Versioned typed allowlist only; no transcript/tool-table join and no automatic filesystem/network action | Store export/cascade test (1/1), normal run export assertions (1/1) | PASS |
| D171-10 | Existing critical persistence contract | Inject existing required `PersistenceActor` running/terminal failure | Existing run error and stop-before-next-stage behavior remain intact; best-effort writer does not mask required failures | Running and terminal persistence failure tests (1/1 each) | PASS |

## Implementation plan

1. Freeze this spec and matrix on the implementation branch before changing Rust production files.
2. Add `vega_store` migration/API and store-level validation/read/reopen tests.
3. Add typed runtime provider failure/attempt metadata and distinct summary error types; preserve raw payload redaction in Debug/Display and existing coarse UI status mappings.
4. Add conversation-layer diagnostic lifecycle IDs and an independent bounded best-effort SQLite writer; project run/provider/summary/tool lifecycle without passing raw content.
5. Add typed read/export APIs and cross-crate integration tests for correlation, failure isolation and canary absence.
6. Run only affected-crate targeted Nextest filters, `cargo fmt --all -- --check`, affected-crate Clippy with `-D warnings`, and `git diff --check`. Do not run workspace-wide local tests.
7. Record exact commands, exit codes, concise raw output, diff hash, deviations and residuals here; commit in small stages (at most three implementation commits), push branch and open a PR. Do not merge or close the Issue; retain worktree for desktop acceptance.

## Verification evidence

| Command | Exit | Result / raw output excerpt |
|---|---:|---|
| `cargo nextest run -p vega_store run_diagnostics` | 0 | `5 tests run: 5 passed, 145 skipped` |
| Store migration/table-count filters | 0 | Two targeted schema tests: `1 test run: 1 passed` each; user_version 15, 26 tables |
| `cargo nextest run -p vega_runtime error::tests::` | 0 | `5 tests run: 5 passed, 207 skipped` |
| `cargo nextest run -p vega_runtime provider::tests::` | 0 | `5 tests run: 5 passed, 207 skipped` |
| `cargo nextest run -p vega_runtime openai::tests::` | 0 | `41 tests run: 41 passed, 171 skipped` |
| `cargo nextest run -p vega_runtime stream_failure_preserves_success_response_metadata` | 0 | `1 test run: 1 passed, 211 skipped` |
| `cargo nextest run -p vega_conversation agent::compaction::` | 0 | `19 tests run: 19 passed, 515 skipped` |
| Conversation writer, correlation, SQL failure and required persistence filters | 0 | Seven targeted cases: `1 test run: 1 passed` each; exact test names are recorded in the matrix above |
| `cargo check -p vega_runtime -p vega_store -p vega_conversation` | 0 | `Finished dev profile` |
| `cargo clippy -p vega_runtime -p vega_store -p vega_conversation --all-targets -- -D warnings` | 0 | `Finished dev profile` |
| `cargo fmt --all -- --check` | 0 | No output |
| `git diff --check` | 0 | No output |
| Implementation diff SHA-256 (excluding this delivery record) | — | `e41516cf0d72d87ed420d2b5b752a7d30a892613c108b79e20eb3331994d4c2b` |
| GitHub PR Clippy | IN PROGRESS | Check is running for the pushed PR revision; this documentation-only revision also needs its PR workflow checks to finish |
| GitHub PR Nextest | IN PROGRESS | Check is running for the pushed PR revision; this documentation-only revision also needs its PR workflow checks to finish |

## Privacy boundary and residuals

- Tests use MockProvider, in-process metadata fixtures, temporary SQLite, deterministic identifiers, fake credentials and canary strings only.
- Diagnostics never store API keys, Authorization values, request/response bodies, prompt/history, visible or reasoning response text, summary text, complete tool input/output, filesystem contents, endpoint URLs or arbitrary headers.
- No user database, real credentials, external provider, live HTTP service, app install or native UI was accessed for implementation verification.
- Graceful last-sender shutdown is tested to drain the bounded queue before the writer exits. Abrupt process termination (`SIGKILL`/power loss) can still lose queued best-effort diagnostics and is not claimed to flush.
- Real desktop/UI acceptance is **NOT RUN** and remains for product acceptance after the PR is reviewed.
- This follow-up changes documentation only; no Rust production code or tests were changed.
- PR URL: [https://github.com/puzige/vega/pull/217](https://github.com/puzige/vega/pull/217); base `master` at `db97c5d2d5961c33a52df276f157be4836c2c9f9`, head ref `feat/171-run-diagnostics-implementation`.

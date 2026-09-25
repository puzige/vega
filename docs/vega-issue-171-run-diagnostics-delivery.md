# Issue #171 — Run diagnostics delivery record

**Status:** implementation and local verification complete; Issue remains OPEN
**Branch:** `feat/171-run-diagnostics-implementation`
**Base:** `db97c5d` (`origin/master`, includes #146 and #64)
**Spec:** [Run diagnostics implementation contract](vega-issue-171-run-diagnostics-spec.md)
**Implementation commit:** `d136bc7 feat(#171): persist safe run diagnostics`
**Pull request:** [#217](https://github.com/puzige/vega/pull/217) — OPEN, not merged
**PR base:** `master` (`db97c5d2d5961c33a52df276f157be4836c2c9f9`)
**PR head:** `feat/171-run-diagnostics-implementation`
**Local verification:** affected-crate targeted Nextest (including the CI expectation fixes), fmt, `cargo check`, Clippy with `-D warnings`, and `git diff --check` passed on the current PR worktree.

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
| D171-04 | Provider failure classification | Inject typed HTTP, transport and protocol failures, plus rejected response | Closed categories differ without parsing diagnostic text; Debug/Display hide error body and request-ID values; rejected duplicate or post-terminal usage cannot overwrite accepted diagnostic metrics | Runtime error/provider/OpenAI filters; usage protocol filter (5/5) asserts retained accepted token metrics and one `UsageUpdated` event | PASS |
| D171-05 | Summary outcomes | Mock summary stream, expired deadline, oversized input, empty/malformed output and typed provider failures | Timeout, truncation, empty, framing, projection/source and known preflight failures map to safe typed codes; over-budget preflight never calls provider; output-limit failure counts the whole observed chunk without retaining its body | `cargo nextest run -p vega_conversation agent::compaction::` (19/19), including overflow count/export canary regression; preflight and timeout cases included | PASS |
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
| `cargo nextest run -p vega_runtime usage_limits::` | 0 | `5 tests run: 5 passed, 207 skipped`; duplicate and post-terminal events preserve accepted diagnostic metrics and usage event count |
| `cargo nextest run -p vega_runtime stream_failure_preserves_success_response_metadata` | 0 | `1 test run: 1 passed, 211 skipped` |
| `cargo nextest run -p vega_conversation agent::compaction::` | 0 | `19 tests run: 19 passed, 515 skipped`; `summary_stage_records_overflow_bytes_without_persisting_summary_text` asserts complete observed byte count and export canary exclusion |
| PR CI regression remediation — conversation targeted filters | 0 | Three stale #171 expectations pass: `3 tests run: 3 passed` (summary failure classification, migration version/table list, seeded todo schema list) |
| PR CI regression remediation — runtime targeted filters | 0 | Three event-sequence expectations pass: `3 tests run: 3 passed` after asserting user-visible semantics while filtering internal `DiagnosticAttempt` events |
| PR CI regression remediation — store targeted filters | 0 | Two migration table-count expectations pass: `2 tests run: 2 passed` |
| `cargo nextest run -p vega_conversation issue74_imported_global_auto_respects_ui_switches_without_ambient_scan` | 0 | `1 test run: 1 passed` on PR branch and on base `db97c5d`; left unchanged because the single CI failure did not reproduce in isolation on either revision |
| Conversation writer, correlation, SQL failure and required persistence filters | 0 | Seven targeted cases: `1 test run: 1 passed` each; exact test names are recorded in the matrix above |
| `cargo check -p vega_runtime -p vega_store -p vega_conversation` | 0 | `Finished dev profile` after CI expectation updates |
| `cargo clippy -p vega_runtime -p vega_store -p vega_conversation --all-targets -- -D warnings` | 0 | `Finished dev profile` after CI expectation updates |
| `cargo fmt --all -- --check` | 0 | No output |
| `git diff --check` | 0 | No output |
| Implementation diff SHA-256 (excluding this delivery record) | — | `d923b99237dbb033985d52f13fb27ce08f69c5fccca0e9712c352215801cbb98` |
| GitHub PR Clippy (run `36170726674`, head `458afdac8b6c51de6bfac1a9794f9a5290d41fef`) | PASS | Clippy passed; Nextest reported 1880 passed, 9 failed, 5 skipped. Eight failures were stale assertions for #171's schema, internal events, and precise summary error type; all eight now pass in the targeted reruns above. |
| GitHub PR Nextest remaining skills case (same run) | NON-REPRODUCIBLE | The one `issue74_imported_global_auto_respects_ui_switches_without_ambient_scan` CI failure passes in isolation on both PR head and base `db97c5d`; no skills code or tests were changed. |
| GitHub PR checks for remediation head | PENDING | Await the workflow result for the new PR head. |

## PR CI regression review

Run `36170726674` was triggered for PR head `458afdac8b6c51de6bfac1a9794f9a5290d41fef`. Its eight reproducible #171-related failures came from tests that still expected the pre-diagnostics contract: schema version 14 and 25 tables, user-visible event sequences without internal `DiagnosticAttempt`, and the old generic `InvalidSummary` category. The test-only fixes update migration expectations and assert user-visible event order after filtering internal diagnostic events; production behavior is unchanged. The eight affected tests pass in three focused reruns (conversation 3/3, runtime 3/3, store 2/2).

The remaining skills test failed once in that full CI run, then passed when run alone on both the PR revision and base `db97c5d`. This does not establish a PR regression, so it remains unchanged and is recorded as a non-reproducible suite-level failure.

The CI run URL is [36170726674](https://github.com/puzige/vega/actions/runs/36170726674). Its Clippy job passed; the failed Nextest result is superseded locally by the focused passing reruns above. The workflow for the remediation head is pending.

## Privacy boundary and residuals

- Tests use MockProvider, in-process metadata fixtures, temporary SQLite, deterministic identifiers, fake credentials and canary strings only.
- Diagnostics never store API keys, Authorization values, request/response bodies, prompt/history, visible or reasoning response text, summary text, complete tool input/output, filesystem contents, endpoint URLs or arbitrary headers.
- No user database, real credentials, external provider, live HTTP service, app install or native UI was accessed for implementation verification.
- Graceful last-sender shutdown is tested to drain the bounded queue before the writer exits. Abrupt process termination (`SIGKILL`/power loss) can still lose queued best-effort diagnostics and is not claimed to flush.
- Real desktop/UI acceptance is **NOT RUN** and remains for product acceptance after the PR is reviewed.
- The review follow-up updates summary overflow byte counts without appending over-limit text. Runtime token metrics are updated only after usage events pass protocol checks.
- The CI remediation commit updates tests only; it does not widen production scope.
- PR URL: [https://github.com/puzige/vega/pull/217](https://github.com/puzige/vega/pull/217); base `master` at `db97c5d2d5961c33a52df276f157be4836c2c9f9`, head ref `feat/171-run-diagnostics-implementation`.

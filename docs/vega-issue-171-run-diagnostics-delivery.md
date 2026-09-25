# Issue #171 — Run diagnostics delivery record

**Status:** implementation, local verification, cloud checks, main-agent review and squash merge complete; Issue remains OPEN for desktop acceptance
**Branch:** `feat/171-run-diagnostics-implementation`
**Base:** `db97c5d` (`origin/master`, includes #146 and #64)
**Spec:** [Run diagnostics implementation contract](vega-issue-171-run-diagnostics-spec.md)
**Implementation commits:** `d136bc7 feat(#171): persist safe run diagnostics`; `fix(#171): serialize diagnostic writes`
**Pull request:** [#217](https://github.com/puzige/vega/pull/217) — MERGED by squash
**PR base:** `master` (`db97c5d2d5961c33a52df276f157be4836c2c9f9`)
**PR head:** `9a2f15cce460d020474370a14d6f50933bf140eb` (delivery evidence update; implementation code head `414bc94d0c3edb7cd61505137552b9e36e26a351`)
**Squash merge:** `b21ee4be7df0fdcbc3fcadd90aed4cc438a65756`
**Local verification:** targeted conversation Nextest filters (7/7 regression/failure-isolation cases and 19/19 compaction cases), affected-crate Clippy with `-D warnings`, `cargo check`, fmt, and `git diff --check` passed after the serialization follow-up.

## Scope

- Add append-only, content-free run diagnostic lifecycle records with `run_id`, `attempt_id`, parent linkage, phase, state, UTC timestamps, duration and safe provider/tool metadata.
- Split summary failure types without changing the existing coarse status UI contract.
- Ordinary runs enqueue diagnostics on the existing bounded `PersistenceActor` channel and insert them on its Store connection. Manual compaction buffers at most 64 events in memory and best-effort inserts through the caller's Store connection after the terminal stage. Neither production path opens a diagnostics-only SQLite connection.
- Add typed local read APIs and explicitly invoked allowlist export API; no viewer UI, automatic export, real provider, user DB or credential access.
- Request IDs are accepted only from `x-request-id`, `request-id`, `openai-request-id` and only when the value matches the frozen ASCII/length constraints.

## Acceptance matrix

| ID | Requirement / risk | Test setup and operation | Observable result | Evidence | Status |
|---|---|---|---|---|---|
| D171-01 | Schema migration and constrained event writes | Temporary SQLite; migrate v14→v15; append valid and invalid lifecycle events | Valid closed-vocabulary rows persist; unsafe IDs and invalid terminal shape rejected; current schema has 26 tables | `cargo nextest run -p vega_store run_diagnostics` (5/5); migration table-count filters (1/1 each) | PASS |
| D171-02 | Ordered reads and restart recovery | Append start/terminal and incomplete start; close/reopen temp DB; complete normal run and manual compaction | Ordered events survive reopen; normal run returns only after actor queue drains; manual events flush in order through caller Store; incomplete attempts remain visible | Store reopen test (1/1); conversation writer drain/reopen (1/1); normal run immediate read assertion (1/1); bounded manual buffer order and compaction terminal assertions (1/1 each) | PASS |
| D171-03 | Provider metadata and retry count | In-process HTTP fixtures return allowlisted and invalid response IDs and actual retry counts | Typed status/request ID/retry count are projected; invalid IDs are omitted; unknown metadata remains absent | `cargo nextest run -p vega_runtime openai::tests::` (41/41), including allowlist and retry metadata cases | PASS |
| D171-04 | Provider failure classification | Inject typed HTTP, transport and protocol failures, plus rejected response | Closed categories differ without parsing diagnostic text; Debug/Display hide error body and request-ID values; rejected duplicate or post-terminal usage cannot overwrite accepted diagnostic metrics | Runtime error/provider/OpenAI filters; usage protocol filter (5/5) asserts retained accepted token metrics and one `UsageUpdated` event | PASS |
| D171-05 | Summary outcomes | Mock summary stream, expired deadline, oversized input, empty/malformed output and typed provider failures | Timeout, truncation, empty, framing, projection/source and known preflight failures map to safe typed codes; over-budget preflight never calls provider; output-limit failure counts the whole observed chunk without retaining its body | `cargo nextest run -p vega_conversation agent::compaction::` (19/19), including overflow count/export canary regression; preflight and timeout cases included | PASS |
| D171-06 | Tool and later-stage correlation | Mock successful tool then failing model attempt under one run | Ordered records share run ID and validated tool call ID; provider failure receives its typed code | Conversation correlation integration test (1/1); normal tool lifecycle test (1/1) | PASS |
| D171-07 | Diagnostic write isolation | Fill/close actor queue; install diagnostic-only SQLite ABORT trigger for actor and caller-store buffered inserts | Queue overflow/closed and insert errors are dropped; run result and required assistant message persist despite diagnostic INSERT failure | Actor sink isolation (1/1); actor SQL trigger integration test (1/1); buffered flush trigger test (1/1) | PASS |
| D171-08 | Privacy canaries | Use fake key, Authorization, prompt, response, reasoning, summary, provider error and tool-output canaries | Canaries absent from diagnostic export and custom error/metadata Debug/Display; only safe counts/status/request ID allowlist survive | Runtime OpenAI redaction suite (41/41), summary truncation/export test (1/1), run/tool export assertions (1/1), provider format canaries (2/2) | PASS |
| D171-09 | Export boundary | Explicitly call export API for one run and inspect serialized fields | Versioned typed allowlist only; no transcript/tool-table join and no automatic filesystem/network action | Store export/cascade test (1/1), normal run export assertions (1/1) | PASS |
| D171-10 | Existing critical persistence contract | Inject existing required `PersistenceActor` running/terminal failure | Existing run error and stop-before-next-stage behavior remain intact; best-effort writer does not mask required failures | Running and terminal persistence failure tests (1/1 each) | PASS |

## Implementation plan

1. Freeze this spec and matrix on the implementation branch before changing Rust production files.
2. Add `vega_store` migration/API and store-level validation/read/reopen tests.
3. Add typed runtime provider failure/attempt metadata and distinct summary error types; preserve raw payload redaction in Debug/Display and existing coarse UI status mappings.
4. Add conversation-layer diagnostic lifecycle IDs. Use the existing PersistenceActor connection for ordinary runs and a bounded in-memory event sink plus caller Store connection for manual compaction; project run/provider/summary/tool lifecycle without passing raw content.
5. Add typed read/export APIs and cross-crate integration tests for correlation, failure isolation and canary absence.
6. Run only affected-crate targeted Nextest filters, `cargo fmt --all -- --check`, affected-crate Clippy with `-D warnings`, and `git diff --check`. Do not run workspace-wide local tests.
7. Record exact commands, exit codes, concise raw output, diff hash, deviations and residuals here; commit in small stages (at most three implementation commits), push the branch and open a PR. After cloud `pr-check` passes and the main-agent review is complete, Agent squash-merges to master and moves the Issue to In review. Keep the Issue open and retain the worktree for user desktop acceptance; only close the Issue after that acceptance.

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
| `cargo nextest run -p vega_conversation issue74_imported_global_auto_respects_ui_switches_without_ambient_scan` | 0 | The prior PR package suite exposed an #171 regression: a deferred settings read→write raced the independent diagnostics connection. After serialization, `1 test run: 1 passed`; it asserts the first run terminal is readable before immediately calling `set_global_settings`. |
| Conversation actor/in-memory writer, correlation, SQL failure and required persistence filters | 0 | Seven targeted cases: `7 tests run: 7 passed`; normal run terminal records are immediately readable, actor SQL failure is swallowed without changing result, and buffered flush is bounded/ordered |
| `cargo nextest run -p vega_conversation agent::compaction::` | 0 | `19 tests run: 19 passed, 517 skipped` after manual compaction switched to the caller-Store buffer path |
| `cargo nextest run -p vega_conversation -E 'test(persists_messages_tool_lifecycle_and_zero_cost_usage) | test(diagnostics_keep_tool_success_before_later_provider_failure) | test(diagnostic_sql_write_failure_does_not_change_required_run_result) | test(issue74_imported_global_auto_respects_ui_switches_without_ambient_scan) | test(closing_last_sender_drains_terminal_event_and_allows_reopen) | test(persistence_sink_drops_full_and_closed_events_without_returning_errors) | test(buffered_writer_is_bounded_and_flushes_in_order_on_the_caller_store)'` | 0 | `7 tests run: 7 passed, 529 skipped`; verifies actor queue overflow/closed behavior, caller Store buffer capacity/order/insert failure swallowing, immediate terminal visibility, required outcome preservation, and the reproduced skills sequence |
| `cargo check -p vega_runtime -p vega_store -p vega_conversation` | 0 | `Finished dev profile` after CI expectation updates; current code also compiled under final all-target Clippy |
| `cargo check -p vega_conversation` | 0 | `Finished dev profile` with the same-connection actor and buffered manual sink |
| `cargo clippy -p vega_runtime -p vega_store -p vega_conversation --all-targets -- -D warnings` | 0 | `Finished dev profile` after serialization follow-up |
| `cargo fmt --all -- --check` | 0 | No output after the final code and test edits |
| `git diff --check` | 0 | No output after the final code and documentation edits |
| Implementation diff SHA-256 (excluding this delivery record) | — | `a851a33098517a9ea237111a2c2de3f2c52516d7b7560c369446c726ba830614` |
| GitHub PR Clippy (run `36170726674`, head `458afdac8b6c51de6bfac1a9794f9a5290d41fef`) | PASS | Clippy passed; Nextest reported 1880 passed, 9 failed, 5 skipped. Eight failures were stale assertions for #171's schema, internal events, and precise summary error type; all eight now pass in the targeted reruns above. |
| GitHub PR Nextest skills case (`36173109636`, prior head `f40872a`) | FAIL — #171 regression | It was the only failed case (1888/1889 passed). A local conversation suite also reproduced it (529/530); base `db97c5d` passed 520/520. |
| GitHub PR checks for serialization fix | [PASS](https://github.com/puzige/vega/actions/runs/36177941984) | At head `414bc94d0c3edb7cd61505137552b9e36e26a351`: Clippy, Nextest and required `check (fmt, clippy, test)` all passed. |
| Latest full GitHub PR checks | [PASS](https://github.com/puzige/vega/actions/runs/36179505067) | At PR head `9a2f15cce460d020474370a14d6f50933bf140eb`: Clippy, Nextest and required `check (fmt, clippy, test)` all passed. |

## PR CI regression review

Run `36170726674` on head `458afdac8b6c51de6bfac1a9794f9a5290d41fef` had eight stale expectations from the pre-diagnostics contract; those remain fixed by test-only updates. A subsequent full CI run, [36173109636](https://github.com/puzige/vega/actions/runs/36173109636), passed 1888 tests and failed only `issue74_imported_global_auto_respects_ui_switches_without_ambient_scan` with `DatabaseBusy: database is locked` in `skills::set_global_settings`. This was not a baseline flake: local PR package Nextest reproduced at 529/530, while a fresh detached base `db97c5d` run passed 520/520.

The failure came from a separate diagnostics SQLite connection committing while the existing settings code held a deferred read snapshot; WAL rejected the later read-to-write upgrade. Ordinary run diagnostics now use the existing PersistenceActor Store connection and bounded channel. Manual compaction uses a bounded in-memory event buffer and the supplied Store connection for sequential best-effort inserts at completion. The focused reproducer now passes and asserts terminal visibility before the immediate settings update. This removes diagnostics-only SQLite writers from production paths while preserving no-ack/drop-on-full/error-swallow behavior.

The actor worker returns an operational error after successful startup only through an unexpected task panic/join failure; required event write errors still use the existing acknowledgements and reach the run before terminal recording. If the actor panics during final drain, the run call reports the join failure, while any root terminal already persisted reflects the runtime/processor outcome; queued diagnostics can be missing. Fresh cloud checks for the serialization fix passed in [run 36177941984](https://github.com/puzige/vega/actions/runs/36177941984), and the latest full PR checks also passed in [run 36179505067](https://github.com/puzige/vega/actions/runs/36179505067).

## Privacy boundary and residuals

- Tests use MockProvider, in-process metadata fixtures, temporary SQLite, deterministic identifiers, fake credentials and canary strings only.
- Diagnostics never store API keys, Authorization values, request/response bodies, prompt/history, visible or reasoning response text, summary text, complete tool input/output, filesystem contents, endpoint URLs or arbitrary headers.
- No user database, real credentials, external provider, live HTTP service, app install or native UI was accessed for implementation verification.
- Ordinary run shutdown is tested to drain the actor queue before return; manual compaction flushes its bounded in-memory queue through the caller's existing Store. Abrupt process termination (`SIGKILL`/power loss) can still lose queued best-effort diagnostics and is not claimed to flush.
- Real desktop/UI acceptance is **NOT RUN** and remains for product acceptance after the PR is reviewed.
- The review follow-up updates summary overflow byte counts without appending over-limit text. Runtime token metrics are updated only after usage events pass protocol checks.
- The CI remediation updates test expectations; the follow-up also serializes diagnostics to remove the reproduced independent-writer regression.
- PR URL: [https://github.com/puzige/vega/pull/217](https://github.com/puzige/vega/pull/217); base `master` at `db97c5d2d5961c33a52df276f157be4836c2c9f9`, head ref `feat/171-run-diagnostics-implementation`.

# Issue #168 — compaction followed by provider failure

## Scope

This is a test-only regression slice for the reported sequence: automatic context
compaction succeeds, then the next primary-provider request fails. It does not
change production behavior and does not establish the cause of the historical
native failure. The behavior contract is tracked in [Issue #168](https://github.com/puzige/vega/issues/168), with compaction mechanics described in
[`vega-issue-76-context-compaction-delivery.md`](vega-issue-76-context-compaction-delivery.md).

## Coverage and results

| Case | Evidence | Command | Result |
|---|---|---|---|
| Conversation preserves successful compaction, checkpoint, and failed assistant state after a mocked HTTP 503 on the next primary request | In-process `MockProvider` regression | `cargo nextest run -p vega_conversation issue168_auto_compaction_then_primary_failure_keeps_checkpoint_and_fails_run` | Exit 0; 1 passed, 542 skipped; run `88d9f3c1-90a3-40f3-b33d-f3c1b05f5deb` |
| UI keeps the compaction row and provider-failure projection visible for the active run | GPUI production-stream test with injected events | `cargo nextest run -p vega_ui issue168_compaction_row_remains_visible_after_primary_provider_failure` | Exit 0; 1 passed, 531 skipped; run `28549f94-e2f2-435a-8458-bf8700c27fd5` |
| Rust formatting | Repository formatter check | `cargo fmt --all -- --check` | Exit 0 |
| Patch whitespace | Git diff check | `git diff --check` | Exit 0 |

The conversation case asserts that compaction success precedes the provider
error, the compacted history is sent on the next request, the checkpoint and
successful terminal status remain stored, and the assistant turn ends failed.
The UI case separately injects the successful-compaction status and provider
error into the production stream projection, then checks that both rows and the
failed-run state remain visible. These are two process-local regression seams;
the UI test does not invoke a provider or replay the original native session.

## Residuals

- **LIMIT:** Only the mocked post-compaction HTTP 503 path is covered. No
  production code changed.
- **NOT RUN:** The historical turn predates durable run diagnostics and has no
  retained provider category or HTTP status. Its original native cause cannot
  be reconstructed from the available record.
- **NOT RUN:** No real provider, network, or external-process E2E was run. Such
  integration remains for user acceptance under the repository's #149 testing
  decision.
- **ACCEPTED:** Keep Issue #168 open and its Project card `In progress` until a
  current diagnostic reproduction or safe evidence of the original failure is
  available. This slice is not a fix or final acceptance of the issue.

## Verification context

- Branch: `fix/168-compaction-disconnect`
- Base refreshed from `origin/master` before verification
- Toolchain: Rust/Cargo 1.98.0; cargo-nextest 0.9.146; macOS arm64
- Spec deviation: none

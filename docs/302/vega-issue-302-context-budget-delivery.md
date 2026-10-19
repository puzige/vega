# Issue #302 delivery evidence

## Freeze

- Contract: [bounded Shell model context](vega-issue-302-context-budget.md); deviations: none.
- Verified at: 2026-10-11T15:16:42Z / 2026-10-11 23:16:42 Asia/Shanghai.
- Branch: `feat/302-context-budget`; baseline: fetched `master`, `7d1f262`.
- Tracked implementation/test diff SHA-256: `519741544b7873893a7b0f1a343a11e1e14b5f24c7aada58c96e0ec0be027b9c`.
- Frozen contract SHA-256: `849f86a74f3a77904fe1ba821f2eb61212fb541b4e97b3262cbc38aefa9fa6cf`.
- Platform: Darwin arm64; `rustc 1.98.0 (88d9e12ae 2026-08-18) (Homebrew)`; `cargo 1.98.0 (797e8a9bc 2026-08-05) (Homebrew)`; `git version 2.55.0`.
- Used the worktree's default Cargo target. No shared target configuration, dependency changes, migrations, application installation, or access to user data was required for implementation/tests.

## Implementation

`ChatMessage::projected_tool_result` applies the same deterministic head/omission/tail projection to `bash` output at the runtime result boundary and the rich persisted-history boundary. The projected output is at most 65,536 UTF-8 bytes and retains character boundaries. At/below the ceiling and for other tools, text is unchanged. Existing historical-data framing remains intact.

The live loop projects only after credential redaction. Fresh results, completed-call reuse, validation-result reuse and other terminal message paths use the helper. Runtime output events, terminal records and stored output retain the complete collected/redacted text and execution metadata. Manual and automatic compaction therefore reconstruct the same bounded history, while existing source identity and complete tool grouping checks remain in force.

Manual/hook history preparation checks eligible raw Shell results for owner/pattern credentials before projection. It respects the same valid-checkpoint filter as history reconstruction and excludes covered history. Reader rotation is checked again by the hook. Existing assignment redaction markers are consumed completely, preserving idempotence without hiding a later actual credential.

The three typed local budget errors (`OverLimit`, `SummaryInputOverLimit`, `ResultOverLimit`) now retain `ContextOverLimit` in run diagnostics and safe local-budget UI copy. Unrelated failures retain their existing classification. Budget enforcement and user message contents are unchanged.

## Results

| Requirement | Evidence class | Result |
| --- | --- | --- |
| B302-1 | In-process production loop with scripted provider and Shell executor | RED before production changes; GREEN after projection. About 2.6 MB across 69 long JSON lines, 300k input budget, fresh plus reused call, one execution, complete output events. |
| B302-2 | Unit/property | PASS: empty/below/at/above limit, large single line, multibyte head/tail, explicit omission, idempotence. |
| B302-3 | Runtime and SQLite integration | PASS: completed-call and validation-result reuse; persisted full results reopened with bounded rich/primary reconstruction; full source rows unchanged. |
| B302-4 | Conversation compaction integration | PASS: manual checkpoint/reload equality and automatic live-owner source-fence equality; unchanged newest user/suffix and paired live tool calls; unrelated source mutation still rejected. |
| B302-5 | Production conversation run plus unit classification | PASS: each of the three genuine typed budget paths rejects before any provider request, persists a failed run with `ContextOverLimit`, preserves user input and returns local-budget copy. |
| B302-6 | Focused existing regressions and unit cases | PASS: small read/write/bash reuse; non-Shell output unchanged; genuinely oversized read tail still rejected; existing redaction/offset/JSON contracts and full event/store output preserved. |
| B302-8 | Credential unit and conversation integration | PASS: owner-only secret crossing either cut is rejected before any summary request; both immediate and rotated reader cases retain raw source and install no checkpoint. Safe assignment markers and checkpoint-covered raw rows do not block compaction. |
| B302-7 | Owner manual integration | NOT RUN: resume the original conversation using the merged application. |

### Exact focused commands

Initial red regression (exit 100):

```sh
cargo nextest run -p vega_runtime issue302_large_bash_output_is_bounded_for_fresh_and_reused_model_results
```

Final runtime (exit 0, 14 passed, test duration 3.363 s; build 9.80 s):

```sh
cargo nextest run -p vega_runtime -E 'test(issue302) or test(agent::credential_redaction) or test(issue91_usage_anchor_still_rejects_a_genuinely_large_tool_tail) or test(exact_same_turn_read_write_and_bash_calls_reuse_durable_results_once)'
```

Final conversation (exit 0, 8 passed, test duration 6.830 s; build 21.42 s):

```sh
cargo nextest run -p vega_conversation -E 'test(issue302) or test(issue170_bounded_bash_output) or test(issue76_auto_source_fence_rejects_unrelated_history)'
```

Only changed Rust files were formatted and checked (exit 0):

```sh
rustfmt --edition 2024 --config skip_children=true --check crates/vega_runtime/src/provider.rs crates/vega_runtime/src/agent/loop_.rs crates/vega_runtime/src/agent/credential_redaction.rs crates/vega_runtime/src/agent/tests/loop_tools.rs crates/vega_conversation/src/agent/compaction.rs crates/vega_conversation/src/agent/diagnostics.rs crates/vega_conversation/src/agent/entry.rs crates/vega_conversation/src/agent/pipeline.rs crates/vega_conversation/src/agent/tests/history_permissions.rs crates/vega_conversation/src/types/events.rs
git diff --check
```

No added code comments were found in the diff. No workspace-wide local test or Clippy run was performed.

### Initial red output

```text
────────────
 Nextest run ID 11f26d60-3d6d-4baf-8142-22be88f47660 with nextest profile: default
    Starting 1 test across 1 binary (265 tests skipped)
        FAIL [   1.569s] (1/1) vega_runtime agent::tests::loop_tools::issue302_large_bash_output_is_bounded_for_fresh_and_reused_model_results
  stdout ───

    running 1 test
    test agent::tests::loop_tools::issue302_large_bash_output_is_bounded_for_fresh_and_reused_model_results ... FAILED

    failures:

    failures:
        agent::tests::loop_tools::issue302_large_bash_output_is_bounded_for_fresh_and_reused_model_results

    test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 265 filtered out; finished in 1.56s

  stderr ───

    thread 'agent::tests::loop_tools::issue302_large_bash_output_is_bounded_for_fresh_and_reused_model_results' (6347853) panicked at crates/vega_runtime/src/agent/tests/loop_tools.rs:1006:10:
    large Shell display output must fit as a bounded model projection: Context(ResultOverLimit { estimated_tokens: 876198, target_tokens: 180000 })
    note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

────────────
     Summary [   1.570s] 1 test run: 0 passed, 1 failed, 265 skipped
        FAIL [   1.569s] (1/1) vega_runtime agent::tests::loop_tools::issue302_large_bash_output_is_bounded_for_fresh_and_reused_model_results
error: test run failed
```

### Final runtime output

```text
────────────
 Nextest run ID d09403fb-d259-45de-9f33-0f343f654f24 with nextest profile: default
    Starting 14 tests across 1 binary (255 tests skipped)
        PASS [   0.058s] ( 1/14) vega_runtime agent::credential_redaction::tests::issue170_common_provider_formats_are_redacted_without_owner_lookup
        PASS [   0.084s] ( 2/14) vega_runtime agent::credential_redaction::tests::issue170_exact_credentials_split_across_output_fragments_are_redacted
        PASS [   0.085s] ( 3/14) vega_runtime agent::credential_redaction::tests::issue170_common_provider_formats_are_redacted_without_owner_lookup_jwt_period
        PASS [   0.086s] ( 4/14) vega_runtime agent::credential_redaction::tests::issue302_redacted_assignments_are_idempotent_without_hiding_following_secrets
        PASS [   0.088s] ( 5/14) vega_runtime agent::credential_redaction::tests::issue170_exact_credentials_are_redacted_in_all_tool_output_shapes
        PASS [   0.089s] ( 6/14) vega_runtime agent::credential_redaction::tests::issue170_safe_text_and_short_assignments_are_preserved
        PASS [   0.089s] ( 7/14) vega_runtime agent::credential_redaction::tests::issue170_exact_redaction_maps_offsets_around_replaced_credentials
        PASS [   0.090s] ( 8/14) vega_runtime agent::credential_redaction::tests::issue170_json_escaped_held_credentials_are_redacted
        PASS [   0.051s] ( 9/14) vega_runtime provider::tests::issue302_shell_projection_bounds_utf8_and_preserves_other_tools
        PASS [   0.103s] (10/14) vega_runtime agent::tests::loop_tools::exact_same_turn_read_write_and_bash_calls_reuse_durable_results_once
        PASS [   0.174s] (11/14) vega_runtime agent::tests::issue91_usage_anchor_still_rejects_a_genuinely_large_tool_tail
        PASS [   0.214s] (12/14) vega_runtime agent::tests::loop_tools::issue302_reused_validation_result_redacts_before_shell_projection
        PASS [   0.341s] (13/14) vega_runtime agent::credential_redaction::tests::issue170_large_outputs_are_redacted_without_dropping_safe_neighbors
        PASS [   3.277s] (14/14) vega_runtime agent::tests::loop_tools::issue302_large_bash_output_is_bounded_for_fresh_and_reused_model_results
────────────
     Summary [   3.363s] 14 tests run: 14 passed, 255 skipped
```

### Final conversation output

```text
────────────
 Nextest run ID b7430cac-1838-4c18-9ae1-6b68973d0fc7 with nextest profile: default
    Starting 8 tests across 11 binaries (569 tests skipped)
        PASS [   0.013s] (1/8) vega_conversation types::events::run_failure_tests::issue302_local_context_failures_keep_actionable_content_free_copy
        PASS [   0.046s] (2/8) vega_conversation agent::tests::history_permissions::issue76_auto_source_fence_rejects_unrelated_history_before_summary_request
        PASS [   0.231s] (3/8) vega_conversation agent::tests::history_permissions::issue302_compaction_accepts_redacted_shell_markers_and_skips_covered_raw_rows
        PASS [   0.399s] (4/8) vega_conversation agent::tests::history_permissions::issue302_genuine_context_overflow_persists_typed_root_failure_without_sending
        PASS [   0.713s] (5/8) vega_conversation agent::tests::history_permissions::issue302_manual_compaction_rejects_raw_shell_credentials_across_projection_cuts
        PASS [   4.481s] (6/8) vega_conversation agent::tests::history_permissions::issue302_automatic_compaction_matches_bounded_live_suffix_and_source_fence
        PASS [   5.107s] (7/8) vega_conversation agent::tests::history_permissions::issue302_reopened_shell_projection_matches_compaction_and_preserves_storage
        PASS [   6.830s] (8/8) vega_conversation agent::tests::credential_redaction::issue170_bounded_bash_output_is_redacted_before_events_storage_and_followup_request
────────────
     Summary [   6.830s] 8 tests run: 8 passed, 569 skipped
```

## Intermediate failures retained

The initial incident regression failed before production changes with `ResultOverLimit`, demonstrating detection of the original problem. Two subsequent new-test fixture errors were corrected without weakening requirements or changing existing tests:

- `vega-302-green-conversation.log`, command `cargo nextest run -p vega_conversation issue302`, exit 100: 2 passed / 2 failed. The budget fixture omitted `FrozenReasoning` provider identity, so the intentionally unbudgeted direct API succeeded. The history fixture searched primary history for executable tool IDs, whereas existing primary history intentionally wraps them as untrusted assistant data. The corrected tests bind the model policy and separately assert rich pairing plus primary framing.
- `vega-302-runtime-targeted.log`, command below, exit 100: 4 passed / 1 failed. The validation-reuse fixture used the default Ask mode, so it took ordinary reuse rather than Execute validation. It now uses Execute and the existing normalized invalid-input audit; the original rejection/reuse/output assertions remain.

```sh
cargo nextest run -p vega_runtime -E 'test(issue302) or test(issue91_usage_anchor_still_rejects_a_genuinely_large_tool_tail) or test(exact_same_turn_read_write_and_bash_calls_reuse_durable_results_once)'
```

Bounded raw failure lines:

```text
called `Result::unwrap_err()` on an `Ok` value: ConversationRun { user_message_id_bytes: 26, assistant_message_id_bytes: 26, event_count: 2, content_bytes: 0, interrupted: false, failed: false }
called `Option::unwrap()` on a `None` value
Summary [   0.249s] 4 tests run: 2 passed, 2 failed, 571 skipped
assertion failed: outcome.events.iter().any(|event|
        matches!(event, RuntimeEvent::ToolCallValidationRejected
            { result, .. } if result.output == redacted && result.reused &&
            result.status == RuntimeToolStatus::Rejected))
Summary [   3.102s] 5 tests run: 4 passed, 1 failed, 263 skipped
```

The intervening successful commands were the same initial single-test command and conversation filter above. They were rerun only after new implementation or affected test changes, including checkpoint eligibility, typed-path coverage and reader rotation.

## Raw log hashes

Raw logs are retained locally under these basenames; only bounded output is included above.

| Log | SHA-256 |
| --- | --- |
| `vega-302-red.log` | `a537d873e8920265f62a62ea0be8b3392bb903b65b1649238f0853d2bc42e7be` |
| `vega-302-green-runtime.log` | `db9fb033d31401cca64a8a9e98bf1434b2e05741e9fd691376e1b7a20f8fa381` |
| `vega-302-green-conversation.log` | `b89d7b2c32f8ba785ea395b051d2f14372dae48ac8db42b2afce84ba337ca5ca` |
| `vega-302-runtime-targeted.log` | `a26f3959804090a6d13bf08b7fbd42f389d4680cb4ed72fb1eae3357c28008df` |
| `vega-302-conversation-targeted.log` | `cf9dd412b69e2f5a129f23a7917023d1ea5ddfd7eaabc5ea07c1bafca978fabc` |
| `vega-302-runtime-final.log` | `6fa676de6ecb7b4cc03026ea409acfba30d605a2c78a905dd17ac77cd730ff09` |
| `vega-302-conversation-final.log` | `2b098ad65dc33057954444760a266b2beb41e69333cd246a069969f5c49d3609` |
| `vega-302-conversation-verified.log` | `a7e699738c32d88f4ae8521d9f00dece47040dcfdfaee094ebeea119b62bf233` |

## Residuals

- LIMIT: The projection prevents a single large Shell result from producing the reported spike; multiple bounded results or intrinsically oversized user input can still exceed the configured budget and must fail closed.
- LIMIT: Tool-card/audit output remains the collector's complete retained result, not an unlimited process-output archive.
- NOT RUN: Cloud `pr-check`, merge, and owner manual application verification are integration steps owned by the main agent/user. This report is in-process evidence, not a real provider/network/UI acceptance claim.
- Rollback: revert this change. No stored output migration or restoration is required.

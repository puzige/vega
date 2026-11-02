# Validation and delivery evidence

## Development versions

- Baseline: `ec8b3f8bbd2493690032ceb0e662f46d9f1f1bda`.
- First implementation: `40a582ced6b215a0223fca280319ffba60700871`; [Actions 41405778141](https://github.com/puzige/vega/actions/runs/41405778141). Clippy succeeded; Nextest executed 8 tests, 3 passed, 5 failed, 0 skipped. Failures occurred during edit fixture setup: `edit_count: Some(1)` is invalid because this optional field is reserved for batches of at least two. Corrected the fixture to `None`, without changing production validation or weakening assertions. This failed run is not acceptance evidence.
- Verified development source: `38b44ac50088d1a9512a16481d0bb09ae4b0b2e5`; [complete source](https://github.com/puzige/vega/blob/38b44ac50088d1a9512a16481d0bb09ae4b0b2e5/crates/vega_conversation/src/agent/terminal_history_tests.rs), blob `b2af72aaf5dc5acab24da84ab28bb5cba91c6731`, SHA-256 `c0ca123cac97d4e7fa67dffef960da484438d8d3bef3616dd7759d0c6c079cc6`. Remote Contents API read-back confirmed this source is accessible (11,257 bytes).
- [Actions 41406783131](https://github.com/puzige/vega/actions/runs/41406783131) completed successfully on that exact SHA. Clippy job `125150509246`, Nextest job `125150509605`, required aggregation `125152050532` all succeeded. Nextest discovered/executed 9 tests: 9 passed, 0 skipped. Python groups executed 5 + 15 + 40 tests, all successful.
- Tests exercise real migrated in-memory Stores and the actual `fork_thread`/shared terminal recovery path: write and edit unchanged checkpoint/output; repeated forks; legacy forks including absent timestamp markers; saved provenance after source deletion and another fork; new mutations requiring own identity; mismatched/missing legacy records; invalid saved project/call/checkpoint; atomic failure; ordinary foreign-checkpoint rejection. No filesystem mutation, real services, runtime sends, or UI acceptance is claimed.

## Independent review

Non-implementation writer reviewed `40a582c` against baseline and identified missing early no-prefix legacy support and a write/edit schema-confounded identity test. Incremental review of `40a582c..38b44ac` confirmed both addressed with strict ancestry/cutoff checks and same-type write output, plus the independent fixture correction; no actionable findings remained. Reviews were static; cloud results were verified separately by the parent.

## Final delivery boundary

Per repository policy, after the successful development run the dedicated test file and its `cfg(test)` module wiring are removed. No test-only dependencies, features, fixtures or helpers were introduced outside that file. Production implementation remains identical to the successful development SHA. Existing #519 pending-feedback spec is synchronized to #851's icon-only user revision.

Local checks: `cargo fmt --all -- --check` and `git diff --check`; no local Vega execution, builds, compilation or tests. Final cleanup-head static test inventory, independent cleanup review and full required Actions remain to be verified. Final-head `no Rust tests` means 0 discovered/0 executed, not a functional test pass. Merge, installation and post-merge user manual acceptance remain distinct; Issue stays open until user acceptance.

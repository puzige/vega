# R15 HTTP fixture teardown delivery

## Freeze

- branch: `codex/r15-fixture-bounds`
- task_contract: `docs/vega-r15-macos-icon.md`, 验收补充：已有 HTTP 测试夹具的有界收尾 (`110807f`).
- Scope: only `crates/vega_conversation/src/provider_settings/tests.rs` and this report.

## Cause and change

The first root workspace gate hung in an existing provider test after production had returned and its assertions passed. The owned PID sample found the test thread synchronously joining the fixture thread while the fixture blocked writing the large response. The current-thread Tokio runtime could not drive connection cleanup during that join. The fixture had a read timeout but no write timeout.

The server now has a three-second write timeout. All nine async test joins delegate to one `spawn_blocking` helper and await its result. Both the blocking-task result and server-thread result are unwrapped, so a panic in either still fails the test. Existing request, response, persistence, redaction, and one-attempt assertions remain unchanged. Production deadlines, body bounds, transport, and native app are untouched. No mirror test or dependency was added.

## First failure evidence

- Root's first gate: `cargo test --workspace --locked --no-fail-fast`, raw log `vega-r15-root-workspace-tests.log` under `/private/tmp`; this incomplete/hung run is not a pass.
- Owned test PID was 71895; root alone terminated it after diagnosis. This executor sampled it read-only for one second and did not terminate it or rerun the old hang.
- Sample: `/private/tmp/vega-r15-provider-test-sample.txt`; SHA256 `8b024f419131d9c4488e75426e56874729894f6ee0b68cdda3919230a1e7972f`.
- Sampled source locations before fix: test `tests.rs:262` at `_pthread_join`, fixture `tests.rs:115` at `write_all` / `__sendto`. Owned loopback socket endpoints were still ESTABLISHED.

## Residuals

- Production behavior is unchanged; proof uses existing production-service tests and owned HTTP fixtures with synthetic credentials.
- Full workspace gates and native icon acceptance remain root-owned.
- Spec deviations: none.

## Results

- verified_at_utc: 2026-09-06T08:47:58Z; verified_at_local: 2026-09-06T16:47:58+08:00.
- git_head before fix commit: `110807f`; test-file diff SHA256: `5f4a70fa536d3092b8efb2f5575ec6de7c4e4ee96adbab317f7037946d6f9b91`.
- Commands ran from the dedicated fixture worktree; tests and clippy used the root-approved shared target after root confirmed it idle. All commands exited; no gate process remains owned by this executor.

| evidence | exact command | result | duration / bounded footer | log SHA256 |
|---|---|---|---|---|
| Existing production service + owned loopback E2E | `cargo test -p vega_conversation provider_settings --locked -- --nocapture` | PASS | 12.53s compile + 15.62s tests; `11 passed; 0 failed; 0 ignored; 289 filtered out` | `3d03b72a8f4cf8215799f5336cdcceea0bc9f6faddadf287fd1d6014fbf74bdc` |
| Formatting | `cargo fmt --all -- --check` | PASS | exit 0; empty output | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| Strict crate lint | `cargo clippy -p vega_conversation --all-targets --locked -- -D warnings` | PASS | 4.39s; `Finished dev profile` | `47682ba64afb7b8361600b6fead266ee84c91d4a44a8427bff25bac6cdf76915` |

Raw logs are `/private/tmp/vega-r15-fixture-tests.log`, `/private/tmp/vega-r15-fixture-fmt.log`, and `/private/tmp/vega-r15-fixture-clippy.log`. The first fixed-tree run passed, including the previously hung `production_failures_are_bounded_content_free_and_never_retry`; no retry-to-green loop was used.

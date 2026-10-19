# Issue #191 — Branch list capacity delivery

## Freeze

- verified_at_utc: 2026-09-25 15:26:12 UTC
- verified_at_local: 2026-09-25 23:26:12 Asia/Shanghai
- branch: `feat/191-composer-branch-menu-fix`
- base: latest `origin/master` after PR #204 merged; #191 commits were rebased without conflicts
- task_contract: [branch-list capacity specification](vega-issue-191-branch-list-capacity.md)
- pull_request: [#207](https://github.com/puzige/vega/pull/207), open and not merged
- implementation/spec diff SHA-256 excluding this delivery record: `6cac7e97c7f9498626c0b9c49b3be58cafb86f93964312de3d20627f54ffccee`
- os_arch: macOS 15.8, Darwin 24.6.0, arm64
- rustc: `rustc 1.98.0 (88d9e12ae 2026-08-18) (Homebrew)`
- cargo: `cargo 1.98.0 (797e8a9bc 2026-08-05) (Homebrew)`
- git: `git version 2.55.0`

## Results

The read-only project inspection recorded 14,449 tracked paths totaling 1,030,287 NUL-delimited bytes, 9 local heads, 4 remote refs, and zero bytes from the tracked-path `check-attr --all` query. The selector continues to show only local heads. The Issue #205 desktop report and the reopened [Issue #191](https://github.com/puzige/vega/issues/191) record the v0.1.13 failure and its user-project evidence.

| Requirement | Evidence class | Exact command | Result |
|---|---|---|---|
| Confirm the existing generic path guard | UNIT/PROPERTY | `cargo nextest run -p vega_conversation -E 'test(git_workspace_retained_budget_and_path_caps_are_inclusive)'` | `Starting 1 test across 11 binaries (518 tests skipped)`; `PASS ... git_workspace_retained_budget_and_path_caps_are_inclusive`; `Summary: 1 test run: 1 passed, 518 skipped` |
| List branches at the reported tracked-path count | FAULT-INJECTION | `cargo nextest run -p vega_conversation -E 'test(/refresh_accepts_14449_paths_and_nine_local_branch_refs/) | test(/refresh_reports_the_branch_path_limit_when_capacity_is_exceeded/) | test(/branch_list_path_limit_is_bounded_and_separate_from_workspace_limit/) | test(/path_counts_and_filter_values_are_inclusive_and_fail_closed/)'` | `Starting 4 tests across 11 binaries (518 tests skipped)`; all four named tests `PASS`; `Summary: 4 tests run: 4 passed, 518 skipped` |
| Show accurate selector errors and preserve Composer entry | UNIT/GPUI | `cargo nextest run -p vega_ui -E 'test(/branch_selector_limit_errors_name_the_limited_resource/) | test(/branch_selector_error_is_typed_and_clears_partial_state/) | test(/issue191_persisted_project_conversation_opens_the_footer_branch_selector/)'` | `Starting 3 tests across 1 binary (484 tests skipped)`; all three named tests `PASS`; `Summary: 3 tests run: 3 passed, 484 skipped` |
| Format | CHECK | `cargo fmt --all -- --check` | exit 0; no output |
| Whitespace and patch validation | CHECK | `git diff --check origin/master...HEAD` | exit 0; no output |
| Lint affected crates | CHECK | `cargo clippy -p vega_conversation -p vega_ui --all-targets -- -D warnings` | exit 0; finished in 7.11s. Bounded footer: `Finished dev profile`; dependency future-incompatibility notice for `block v0.1.6`; no lint warning. Captured log SHA-256: `8b37202c33e0ad3a4a9dc09162ddff3c3b8befb9358a00814c81bb5bd595881c`. |

The first UI test build after adding the typed error variants found an exhaustive-match compile error in the diff-view fallback. The new variants were added to its existing safe fallback group; the UI tests, formatting, and final Clippy run then passed, including after rebase to the latest `origin/master`.

The service tests use the repository's in-process Git command boundary. They cover 14,449 tracked paths with 9 local rows, the finite 25,000 tracked-path limit, unchanged workspace/switch-path guards, and the 10,000 local-ref limit. The synthetic test fixture does not claim to display remote refs.

The earlier PR check runs target the pre-rebase head. A fresh check run is expected after force-pushing the rebase; the PR remains open and unmerged.

## Residuals

- **NOT RUN** — Native desktop acceptance of the installed app against the reported project. Automated process-boundary and GPUI tests do not claim real desktop acceptance.
- **PENDING** — Cloud `pr-check` completion and review.
- **ACCEPTED** — Remote refs remain excluded; this preserves the existing local-head-only selector contract.
- **ACCEPTED** — Projects exceeding 25,000 tracked paths receive a specific actionable path-limit error. Existing byte, process-output, timeout, memory, and unrelated 10,000-path guards remain bounded.
- Spec deviations: none.

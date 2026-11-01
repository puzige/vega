# Five-dialect validation evidence

## Versions and scope

Issue #834, PR #835; baseline `1f93cbacbe6d87bd25111d6841477b571c852d50`. Contract: [mcp-five-dialects.md](mcp-five-dialects.md). No scope deviation. No runtime configuration/credential or protocol changes. Local verification is static only; no local Vega execution, compilation, build, packaging or tests.

## Development validation

- Initial source `ef550f4151210a597995f6b57e403384d8988f08`: [Actions 41315085891](https://github.com/puzige/vega/actions/runs/41315085891) passed full required checks, 6 discovered / 6 executed / 6 passed / 0 skipped. Independent static review found one P2: legacy dependencies under 2020-12 were compiled by the pinned dependency but skipped by preflight.
- Corrected source `057edba02badf47cdded4679216dd27571e2c2f9`: [recoverable original source](https://github.com/puzige/vega/blob/057edba02badf47cdded4679216dd27571e2c2f9/crates/vega_mcp/src/wire.rs), GitHub blob `f613266b040be44cfb8428694884d1f6b0079318`; [Actions 41316023736](https://github.com/puzige/vega/actions/runs/41316023736) passed full checks on 2026-11-01. Nextest summary: 7 tests run, 7 passed, 0 skipped; 7 discovered / 7 executed. Independent incremental static review cleared the P2 at this exact SHA, reusing the previous no-other-findings result.
- Actual Actions commands: `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `python3 scripts/release_tests.py`; `python3 scripts/prepare_computer_use_tests.py`; `python3 -m unittest discover -s scripts/tests -v`; `cargo nextest list --workspace --message-format json`; `cargo nextest run --workspace`. The aggregate `check (fmt, clippy, test)` succeeded.
- Seven behavior groups: five URI families/default and original schema preservation; boundaries/IDs/ref siblings; tuple/dependencies/recursive semantics; unknown/mixed/vocabulary declarations and annotation handling; default/explicit modern-root dependency safety; HTTP header extraction and unreachable markers; offline refs/resource budgets/diagnostics.

## Final cleanup and remaining gates

The successful source was verified accessible through GitHub and recorded/read back in #834 before cleanup. Delivery removes the complete private `dialect_tests` module and restores `validate_schema`'s unit success return, discarding the same validator after the same build/preflight. No fixture, helper, feature, dev dependency or separate test registration was introduced. Local final formatting and diff checks passed (exit 0); static search of the modified Rust file found no Rust tests, test-only wiring, unwrap or expect. No tests were weakened or removed to make a failing run pass.

Final cleanup-head independent incremental review and complete required CI are pending separately; development results do not substitute for that head. Final no-test CI must report `no Rust tests` (0 discovered / 0 executed), not 'tests passed'. Exact final head and run evidence will be recorded in #834/PR #835 before merge. User has not performed delivered-version integration acceptance. Actual Exa refresh, model API acceptance and tool calls remain user hand verification; no claim of installation or end-to-end verification is made. Keep the Issue open and move to In review only after merge.

# Code viewport development-test cleanup

Issue [#584](https://github.com/puzige/vega/issues/584), PR [#585](https://github.com/puzige/vega/pull/585). Cleanup starts from GREEN development commit `58f569ac3d516dacb8cfa28d589c9c0e4b4c2d9b`, with baseline `b2248996cc68497ef050539fd69f05cc723e4c0a`.

The root fixed and read back the recoverable source/patch/run evidence in [PR comment 6372320036](https://github.com/puzige/vega/pull/585#issuecomment-6372320036) before cleanup. [Actions 40005438002](https://github.com/puzige/vega/actions/runs/40005438002) discovered and executed 11 Rust tests: 11 passed, 0 failed, 0 skipped. Fmt, Clippy, 60 Python tests and the required check succeeded. This result belongs to the development commit.

| Preserved source | SHA256 |
| --- | --- |
| [code_viewport_tests.rs](https://github.com/puzige/vega/blob/58f569ac3d516dacb8cfa28d589c9c0e4b4c2d9b/crates/vega_ui/src/conversation_stream/code_viewport_tests.rs) — 6 tests | `60d3499062ed29d0f6f36213e39c1c1915f143089e6266108dba6a3e6b5ef03c` |
| [thinking_code_viewport_tests.rs](https://github.com/puzige/vega/blob/58f569ac3d516dacb8cfa28d589c9c0e4b4c2d9b/crates/vega_ui/src/conversation_stream/thinking_code_viewport_tests.rs) — 5 tests | `4f5ad35f93daa03190330b817c7e9feb01ba3d6048dff27682713b4581ecd12b` |

Removed both development test files, their `cfg(test)` module declarations, the viewport observation call, and the vega_ui dev-only GPUI `test-support` dependency block. `Cargo.lock` and `crates/vega_ui/Cargo.toml` are byte-identical to the baseline. All 10 development-only package identities and their exclusive edges are removed; baseline dependency versions remain intact. Production handler, viewport, scrollbar, fade, rendering, Python and CI logic were not changed by cleanup.

Static projection removes only the specified test blocks from each GREEN mixed file, then compares its bytes with the cleaned file:

| Production projection | Result / SHA256 |
| --- | --- |
| `code_viewport.rs` | Identical; `a433def896e24c659a0f785371f9b4d637bdc82d71e6125cc16b2137c53f4085` |
| `thinking_body.rs` | Identical; `b382e75c3fbae691fd37199b74d4e32868b7713571036da94c62bd36f8324d1e` |
| Baseline `Cargo.lock` | Identical; `a15fadafca5597ee96d3ad70a8d9013c44f9ebc71474ca85ca8ce1aa9527548f` |

Actual local checks:

- Scoped rustfmt and `rustfmt --edition 2024 --check --config skip_children=true` on the two mixed files: exit 0.
- `git diff --check`: exit 0.
- `cargo metadata --offline --locked --format-version 1`: exit 0; 914 resolved packages, no active GPUI `test-support`, none of the 10 development-only package identities remaining. Metadata only; no compilation or execution.
- Whole-repository static inventory of 453 Rust files and 16 Cargo manifests: 0 Rust test attributes, test conditional-compilation blocks, test modules/property macros, explicit Cargo test targets, or development-support/observer references. Both deleted source files are absent. The only rustdoc fence is an existing `text` diagram; no executable rustdoc fence was found. These are static counts, not test discovery/execution.

No local Vega execution, build, Clippy or test run occurred. Final cleanup-head Actions, including required checks and Rust 0 discovery/0 execution, remain pending; they must be recorded separately from the 11-test development result. READY: cleanup writer stopped all writes. Root owns commit, independent final review and final CI. Spec deviations: none.

# R41 Titlebar Search delivery

## Freeze

- verified_at_utc: `2026-09-11T12:25:07Z`
- verified_at_local: `2026-09-11T20:25:07+0800`
- git_head: implementation commit `b081b46`
- tracked_diff_sha256: `84e7f024da99cccecb8684f32578ef9f7cd1990772fb8f51883209b7e4185092`
- task_contract: [R41 Titlebar Search adjacency](vega-r41-titlebar-search.md)
- environment: macOS 15.7.9 arm64; Rust/Cargo 1.98.0; Git 2.55.0

## Results

| requirement | evidence class | exact command / action | result | bounded evidence |
|---|---|---|---|---|
| Shared order and behavior | E2E-REAL | targeted Vega palette, Sidebar organization and navigation-control tests | PASS | Sidebar → Search → Back → Forward at exact 4px gaps; visible/hidden shell, pointer, Space and Command-K covered |
| Formatting | BUILD | `cargo fmt --all -- --check` | PASS | exit 0 |
| Strict lint | BUILD | `cargo clippy --all-targets -- -D warnings` | PASS | exit 0; only existing dependency future-compatibility notice |
| Workspace regression | BUILD | `cargo test --workspace` | PASS | Vega 84, UI 186 and all remaining crate/integration/doc tests passed |
| Architecture boundary | BUILD | `cargo tree -p vega_runtime` UI dependency scan | PASS | no GPUI or `vega_ui` dependency found |
| Candidate package | BUILD | `cargo xtask package` | PASS | release bundle, Info.plist and signature validation passed |
| Installed candidate identity | E2E-REAL | strict codesign verification and SHA-256 comparison | PASS | installed and packaged executables both `2adb14774a02ebf69cdde577e941b1f927f910eabad956cbae0b11ce8ba2a82f` |
| Native titlebar walkthrough | E2E-REAL | launch installed Vega, inspect controls, activate Search | PASS | Search is visibly adjacent to Sidebar and opens the existing palette; screenshots `/tmp/vega-r41-titlebar-search.png` and `/tmp/vega-r41-search-open.png` |

The previously installed application is recoverable at
`/tmp/vega-r41-install.O4ySQy/Vega.app.previous`.

## Residuals

- ACCEPTED: Back, Forward, Sidebar, Search palette geometry and Command-K
  behavior are unchanged apart from the requested control order.
- Spec deviations: none.

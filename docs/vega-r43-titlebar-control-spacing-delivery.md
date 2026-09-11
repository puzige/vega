# R43 Titlebar control spacing delivery

## Freeze

- verified_at_utc: `2026-09-11T13:07:01Z`
- verified_at_local: `2026-09-11T21:07:01+0800`
- git_head: implementation commit `9b01d34`
- tracked_diff_sha256: `4959e901ae65735a23cc8d14ac693a70058ec9f262d1213aaaed8a34a257ee18`
- task_contract: [R43 Titlebar control spacing](vega-r43-titlebar-control-spacing.md)
- environment: macOS 15.7.9 arm64; Rust/Cargo 1.98.0; Git 2.55.0

## Results

| requirement | evidence class | exact command / action | result | bounded evidence |
|---|---|---|---|---|
| Shared geometry tokens | UNIT | `cargo test -p vega_theme r43_titlebar_control_geometry_is_frozen` | PASS | control size `28`, gap `4` |
| Mounted titlebar grid and actions | E2E-REAL | `cargo test -p vega production_root_palette_escape_preserves_composer_and_settings_action` | PASS | Sidebar, Search, Back and Forward are 28x28; 16x16 icons are centered; adjacent centers are 32 apart; disabled history slots and all four actions remain functional |
| Formatting | BUILD | `cargo fmt --all -- --check` | PASS | exit 0 |
| Strict lint | BUILD | `cargo clippy --all-targets -- -D warnings` | PASS | exit 0; existing dependency future-compatibility notice only |
| Workspace regression | BUILD | `cargo test --workspace` | PASS | final full run passed; two known asynchronous process tests failed once and then passed individually before the clean full rerun |
| Candidate package | BUILD | `cargo xtask package` | PASS | release bundle, Info.plist and signature validation passed |
| Installed candidate identity | E2E-REAL | strict codesign verification and SHA-256 comparison | PASS | installed and packaged executables both `bf9ecb7202fed04c833cd96b61d8eafeb9334e0cbf210a0413c1096f26cb5d22` |
| Native titlebar appearance | E2E-REAL | launch installed Vega and inspect Sidebar / Search / Back / Forward controls | PASS | all four controls share one visual grid and baseline; screenshot `/tmp/vega-r43-titlebar-spacing.png` |

The previously installed application is recoverable at
`/tmp/vega-r43-install.sX1tBF/Vega.app.previous`.

## Residuals

- ACCEPTED: disabled Back or Forward controls retain their slot so the other
  controls do not shift as navigation availability changes.
- Spec deviations: none.

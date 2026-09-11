# R40 Search palette geometry delivery

## Freeze

- verified_at_utc: `2026-09-11T10:53:48Z`
- verified_at_local: `2026-09-11T18:53:48+0800`
- git_head: implementation commit `dc3410d`
- tracked_diff_sha256: `a264f53f0137d27cea9a3391be49908b243f3c2bba9c331f2e4ecefcb29ca6d7`
- task_contract: [R40 Search palette geometry](vega-r40-search-palette-geometry.md)
- environment: macOS 15.7.9 arm64; Rust/Cargo 1.98.0; Git 2.55.0

## Results

| requirement | evidence class | exact command / action | result | bounded evidence |
|---|---|---|---|---|
| Typed geometry | UNIT | `cargo test -p vega_theme r40_command_palette_geometry_is_frozen` | PASS | 1 passed; width 520px, max height 480px, independent from compact menu width |
| Responsive mount and internal scrolling | E2E-REAL | `cargo test -p vega_ui production_palette_mounts_roomy_responsive_and_bounded_geometry` | PASS | 1 passed; production render at roomy, 960×600, narrow, short, and defensive-floor sizes; real wheel event moves results while input/footer remain fixed |
| Existing search interaction | E2E-REAL | `cargo test -p vega_ui command_palette::tests` | PASS | 2 passed; keyboard, scopes, activation, IME guard, geometry and scrolling |
| Formatting | BUILD | `cargo fmt --all -- --check` | PASS | exit 0 |
| Strict lint | BUILD | `cargo clippy --all-targets -- -D warnings` | PASS | exit 0; only dependency future-incompatibility notice for `block 0.1.6` |
| Workspace regression | BUILD | `cargo test --workspace` | PASS | Vega 84, conversation suites, UI 185, theme 12, xtask 36, remaining crates and doctests all passed |
| Candidate package | BUILD | `cargo xtask package` | PASS | release bundle, plist and signature validation passed |
| Installed candidate identity | E2E-REAL | strict codesign verification and SHA-256 comparison | PASS | installed and packaged executables both `0ff410ad2fb56c5d2eca0dd3eae8cf7c00944f40badce22a9cb1b43bee04ede7` |
| Native Search walkthrough | E2E-REAL | launch installed Vega, click Search / press Command-K, capture real screen | PASS | centered wide palette is visibly bounded; input, scopes, results and footer all remain inside the card; screenshot `/tmp/vega-r40-native-search.png` |

The previously installed application is recoverable at
`/tmp/vega-r40-install.jLwsJ6/Vega.app.previous`.

## Residuals

- ACCEPTED: the existing shared 18px floating-surface radius, palette top
  offset, colors, typography, ranking and commands were intentionally kept.
- ACCEPTED: `block 0.1.6` emits the repository's existing future-compatibility
  notice; strict Clippy still exits successfully.
- Spec deviations: none.

# R42 Single selection highlight delivery

## Freeze

- verified_at_utc: `2026-09-11T12:44:23Z`
- verified_at_local: `2026-09-11T20:44:23+0800`
- git_head: implementation commit `bf1ada9`
- tracked_diff_sha256: `decb1416060f396526d2b39bd59926823940f1ead92227b0c78bee21d95ab759`
- task_contract: [R42 Single selection highlight](vega-r42-single-selection-highlight.md)
- environment: macOS 15.7.9 arm64; Rust/Cargo 1.98.0; Git 2.55.0

## Results

| requirement | evidence class | exact command / action | result | bounded evidence |
|---|---|---|---|---|
| Specific selection precedence | E2E-REAL | `cargo test -p vega_ui r42_mounted_sidebar_prefers_the_active_task_surface_in_light_and_dark` | PASS | normal child and Pinned project task suppress ancestor project active; project-only remains active in Light/Dark |
| Organization regression | E2E-REAL | mounted Sidebar organization test group | PASS | 21 passed; hover, focus, collapse and expand preserved |
| Formatting | BUILD | `cargo fmt --all -- --check` | PASS | exit 0 |
| Strict lint | BUILD | `cargo clippy --all-targets -- -D warnings` | PASS | exit 0; existing dependency future-compatibility notice only |
| Workspace regression | BUILD | `cargo test --workspace` | PASS | all crate, integration and doc tests passed |
| Candidate package | BUILD | `cargo xtask package` | PASS | release bundle, Info.plist and signature validation passed |
| Installed candidate identity | E2E-REAL | strict codesign verification and SHA-256 comparison | PASS | installed and packaged executables both `acc5078622996a1ada4a01d9d2dfbf16d1c7f4d82708c811a6d1ee5245cbe41e` |
| Native reported scenario | E2E-REAL | launch installed Vega and open `R13 Beta` under `r13-beta-project` | PASS | only `R13 Beta` has the neutral active surface; its open Folder ancestor remains at rest; screenshot `/tmp/vega-r42-child-selected-4.png` |

The previously installed application is recoverable at
`/tmp/vega-r42-install.0Ki1J4/Vega.app.previous`.

## Residuals

- ACCEPTED: a selected project with no opened task keeps its existing neutral
  active surface; this distinguishes project-only selection from no selection.
- Spec deviations: none.

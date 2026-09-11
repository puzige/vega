# Vega R34 Sidebar shortcut column — delivery

R34 removes the Search-only outer inset so the two top navigation rows share
one row width and one trailing shortcut column.

## Freeze

- verified_at_utc: 2026-09-11T07:40:28Z
- verified_at_local: 2026-09-11T15:40:28+08:00
- branch: `feat/r34-sidebar-shortcut-column`
- contract commit: `c95a235`
- implementation commit: `adc0619`
- task contract: `docs/vega-r34-sidebar-shortcut-column.md`
- os_arch: macOS arm64
- rustc: `1.98.0`
- cargo: `1.98.0`
- git: `2.55.0`

## Results

| Requirement | Exact command | Result |
|---|---|---|
| Exact top-row and shortcut geometry | `cargo test -p vega_ui r34_top_navigation_rows_share_shortcut_column_across_widths_and_themes -- --nocapture` | PASS, 1/0 |
| Complete UI crate | `cargo test -p vega_ui` | PASS, 180/0 |
| Formatting | `cargo fmt --all -- --check` | PASS |
| Strict workspace lint | `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| Complete workspace, serial | `cargo test --workspace -- --test-threads=1` | PASS |
| Candidate package | `cargo xtask package` | PASS |

## Native acceptance

- PASS: `⌘N` and `⌘K` visibly share one trailing right-edge column.
- PASS: New Task and Search surfaces share the same left edge, right edge, and
  width; Search no longer has the extra 8px outer inset.
- PASS: the Search label starts at the New Task Plus icon's leading edge.
- PASS: Light and Dark appearances remain readable without clipping or changes
  to typography, row height, hover behavior, or activation.
- PASS: appearance was restored to Follow System.

The installed candidate executable SHA256 is
`1a96d3684aaacebc96f1c07cfe3f560119352240dacd864bf5dd9992b8b4cdb6`.
The previous app is recoverable at
`/tmp/vega-r34-candidate-install.mvR5Ur/Vega.app.previous`.

No push or remote release was performed. Scope deviation: none.

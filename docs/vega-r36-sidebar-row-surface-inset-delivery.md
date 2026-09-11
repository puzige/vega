# Vega R36 Sidebar Pinned row surface inset — delivery

R36 gives Pinned task surfaces 8px of leading breathing room without moving
the established title column.

## Freeze

- verified_at_utc: 2026-09-11T08:43:43Z
- verified_at_local: 2026-09-11T16:43:43+08:00
- branch: `feat/r36-sidebar-row-surface-inset`
- contract commit: `26aca76`
- implementation commit: `8da854e`
- task contract: `docs/vega-r36-sidebar-row-surface-inset.md`
- os_arch: macOS arm64

## Results

| Requirement | Exact command | Result |
|---|---|---|
| Mounted R36 geometry | `cargo test -p vega_ui r36_pinned_surfaces_add_leading_padding_without_moving_content -- --nocapture` | PASS, 1/0 |
| Complete UI crate | `cargo test -p vega_ui -- --test-threads=1` | PASS, 183/0 |
| Formatting | `cargo fmt --all -- --check` | PASS |
| Strict workspace lint | `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| Complete workspace, serial | `cargo test --workspace -- --test-threads=1` | PASS |
| Candidate package | `cargo xtask package` | PASS |

## Native acceptance

- PASS: the selected Pinned surface begins before its title and visibly
  provides leading padding rather than touching the glyphs.
- PASS: the title remains aligned with the Pinned heading and top-level Recents
  title column.
- PASS: trailing edge, menu placement, 32px row height, and 8px radius remain
  stable.
- PASS: mounted Light/Dark checks cover selected, unselected, hover, and
  menu-open geometry without layout jumps.
- PASS: the R35 12px Pinned-to-Projects break and non-Pinned rows are unchanged.

The installed candidate executable SHA256 is
`453233cc233b2c426e83b2b284f2325ece8931c48525a5df0d7c66df9faea9e5`.
The previous app is recoverable at
`/tmp/vega-r36-candidate-install.BgWhwG/Vega.app.previous`.

No push or remote release was performed. Scope deviation: none.

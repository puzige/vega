# Vega R30 Sidebar typography — delivery

R30 brings the native Sidebar typography up one readable step without changing
its density: primary navigation is 15px, section headings and Sidebar metadata
are 13px, and navigation rows remain 32px tall.

## Freeze

- verified_at_utc: 2026-09-11T02:42:00Z
- verified_at_local: 2026-09-11T10:42:00+08:00
- branch: `feat/r30-sidebar-typography`
- contract commit: `34153fd1196705093fb832c1ae61da4161badda4`
- implementation commit: `bdc082deac53f8d2813996003ac6861f325dfe5d`
- pre-delivery diff SHA256:
  `dd1b97390f76ca8773bf76a986c81d72a122a83445e401a3585d5e92ed68039f`
- task contract: `docs/vega-r30-sidebar-typography.md`

## Results

| Requirement | Exact command | Result |
|---|---|---|
| Mounted 15/13/13 hierarchy, 32px rows, Light/Dark bounds | `cargo test -p vega_ui r30_sidebar_typography_uses_scoped_sizes_without_changing_row_geometry -- --nocapture` | PASS, 1/0 |
| Typed typography tokens | `cargo test -p vega_theme r30_sidebar_typography_is_scoped_and_frozen -- --nocapture` | PASS, 1/0 |
| Organization mounted regressions | focused R15/R26/R27/R28/R29/R30 suite | PASS, 13/0 |
| Complete UI crate | `cargo test -p vega_ui` | PASS, 177/0 |
| Theme crate | `cargo test -p vega_theme` | PASS, 11/0 |
| Formatting | `cargo fmt --all -- --check` | PASS |
| Strict workspace lint | `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| Complete workspace, serial | `cargo test --workspace -- --test-threads=1` | PASS |
| Candidate package | `cargo xtask package` | PASS |

Concurrent complete-workspace attempts encountered existing diff-refresh,
branch-barrier, and PTY timing flakes. Each affected isolated test passed, and
the subsequent serial complete-workspace run passed with zero failures.

## Native acceptance

- PASS: project labels, task titles, New Task, Search, Settings, and `Show More`
  render at the new 15px primary size without clipping.
- PASS: `Pinned / Projects / Recents` and project/time/shortcut metadata
  render at 13px and retain their subdued hierarchy.
- PASS: 32px row height, 32px title origin, project metadata column,
  progressive-list limits, and contextual controls remain unchanged.
- PASS: Light and Dark appearances remain readable. Appearance was restored to
  Follow System.

The final local-`master` installed executable SHA256 is
`e200ed347a9bc5e1eb136d31a57e82f440f382435bf8985609e0573b6bce9a19`.
The previous app is recoverable at
`/tmp/vega-r30-master-install.e1Q7DZ/Vega.app.previous`.

No push or remote release was performed. Scope deviation: none.

# Vega R29 Sidebar progressive lists — delivery

R29 removes the elastic blank region between Projects and Recents. Sections
now stack by natural content height; Projects initially shows 5 folders and
Recents initially shows 10 tasks, with independent `Show More / Show Less`
controls for additional content.

## Freeze

- verified_at_utc: 2026-09-11T02:05:35Z
- verified_at_local: 2026-09-11T10:05:35+08:00
- branch: `feat/r29-sidebar-progressive-lists`
- contract commit: `719ebf140f35312e2f9d624f6c262c151fb8079b`
- implementation commit: `6230234328a4e4423310d3a0f8b068b85bf8cdbd`
- pre-delivery diff SHA256:
  `0cbe23a31d89f2a9fc2571a5dcdc5e48ba891d366fe63e2d361b3a2a2bb365ad`
- task contract: `docs/vega-r29-sidebar-progressive-lists.md`

## Results

| Requirement | Exact command | Result |
|---|---|---|
| Compact limits, 8px group gap, independent pointer/keyboard toggles | `cargo test -p vega_ui r29_ -- --nocapture` | PASS, 2/0 |
| Organization mounted regressions | focused R15/R26/R27/R28/R29 suite | PASS, 12/0 |
| Complete UI crate | `cargo test -p vega_ui` | PASS, 176/0 |
| Formatting | `cargo fmt --all -- --check` | PASS |
| Strict workspace lint | `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| Complete workspace | `cargo test --workspace` | PASS |
| Candidate package | `cargo xtask package` | PASS |

## Native acceptance

- PASS: with 9 registered projects, only 5 render initially and `Show More`
  appears on the shared 32px content column.
- PASS: Recents begins immediately after the compact Projects section; the
  previous elastic blank region is gone.
- PASS: clicking `Show More` reveals all 9 projects and changes the control to
  `Show Less`; clicking again restores the compact state.
- PASS: the existing title case, neutral colors, folder geometry, task columns,
  contextual actions, composer, and Settings footer remain unchanged.

The final local-`master` installed executable SHA256 is
`eb2cc6da358ba3cf9036a5a83a406e031c914907b22c0f2e9aae4846b708e6cf`.
The previous app is recoverable at
`/tmp/vega-r29-master-install.q1U5OH/Vega.app.previous`.

No push or remote release was performed. Scope deviation: none.

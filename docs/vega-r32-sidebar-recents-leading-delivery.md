# Vega R32 Sidebar Recents leading edge — delivery

R32 removes the stale 32px empty leading slot from top-level Recents task
titles while preserving the project-child hierarchy grid.

## Freeze

- verified_at_utc: 2026-09-11T06:48:02Z
- verified_at_local: 2026-09-11T14:48:02+08:00
- branch: `feat/r32-sidebar-recents-leading`
- contract commit: `78eea03`
- implementation commit: `d1e7a5e`
- task contract: `docs/vega-r32-sidebar-recents-leading.md`
- os_arch: macOS arm64
- rustc: `1.98.0`
- cargo: `1.98.0`
- git: `2.55.0`

## Results

| Requirement | Exact command | Result |
|---|---|---|
| Pinned/Recents leading edges and project-child hierarchy | `cargo test -p vega_ui r32_ -- --nocapture` | PASS, 1/0 |
| Formatting | `cargo fmt --all -- --check` | PASS |
| Strict workspace lint | `cargo clippy --all-targets -- -D warnings` | PASS |
| Complete workspace, serial | `cargo test --workspace -- --test-threads=1` | PASS |
| Candidate package | `cargo xtask package` | PASS |

## Native acceptance

- PASS: every visible production Pinned title begins at the same left edge as
  the `Pinned` section label.
- PASS: every visible top-level Recents title begins at the same left edge as
  the `Recents` section label; the previous empty leading slot is gone.
- PASS: project rows keep their Folder geometry and project-child tasks keep
  the 32px hierarchy origin.
- PASS: compact R31 typography, trailing time, project metadata, row height,
  progressive lists, and hover behavior remain unchanged.
- PASS: Light and Dark appearances preserve the same alignment and readability.
  Appearance was restored to Follow System.

The installed candidate executable SHA256 is
`4e5d3b09a0788a8cefe37d4d187dbfaa000a652111ff3efd17f50e4a848aba37`.
The previous app is recoverable at
`/tmp/vega-r32-candidate-install.wzJdcZ/Vega.app.previous`.

No push or remote release was performed. Scope deviation: none.

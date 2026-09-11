# Vega R35 Sidebar section spacing — delivery

R35 adds one 4px rhythm step after a rendered Pinned section so its final task
row and the following Projects heading have a clear 12px section break.

## Freeze

- verified_at_utc: 2026-09-11T08:13:03Z
- verified_at_local: 2026-09-11T16:13:03+08:00
- branch: `feat/r35-sidebar-section-spacing`
- contract commit: `ff329de`
- implementation commit: `1d7e11c`
- task contract: `docs/vega-r35-sidebar-section-spacing.md`
- os_arch: macOS arm64

## Results

| Requirement | Exact command | Result |
|---|---|---|
| Mounted R35 geometry | `cargo test -p vega_ui r35_ -- --nocapture` | PASS, 2/0 |
| Complete UI crate | `cargo test -p vega_ui -- --test-threads=1` | PASS, 182/0 |
| Formatting | `cargo fmt --all -- --check` | PASS |
| Strict workspace lint | `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| Complete workspace, serial | `cargo test --workspace -- --test-threads=1` | PASS |
| Candidate package | `cargo xtask package` | PASS |

## Native acceptance

- PASS: the selected final Pinned row and Projects heading read as separate
  groups without excessive whitespace.
- PASS: mounted geometry fixes the final Pinned row-to-Projects-header distance
  at exactly 12px in Light and Dark themes.
- PASS: the selected task background remains a 32px row and does not absorb the
  added spacing.
- PASS: no blank compensation appears when Pinned is absent.
- PASS: Projects-to-Recents spacing and all horizontal alignment remain
  unchanged.

The installed candidate executable SHA256 is
`57bc973e9160987e3a3ed9b31eb1fcaefdd1df50e5e93cfcc64ccfa65fe578aa`.
The previous app is recoverable at
`/tmp/vega-r35-candidate-install.ZD0Wek/Vega.app.previous`.

No push or remote release was performed. Scope deviation: none.

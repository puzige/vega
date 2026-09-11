# Vega R31 Sidebar rollback and Pinned leading edge — delivery

R31 rolls back R30's oversized Sidebar typography and removes the stale 32px
empty leading slot from production Pinned task titles.

## Freeze

- verified_at_utc: 2026-09-11T03:05:28Z
- verified_at_local: 2026-09-11T11:05:28+08:00
- branch: `feat/r31-sidebar-typography-revert-pinned-leading`
- contract commit: `62a8bdcce3431450ec7b8cb3671004323e62bf76`
- implementation commit: `7bd042f8088d61a2f9429433e27620f8963b9c5e`
- pre-delivery diff SHA256:
  `40639cc7b6b50791b3550e531adccc3adcf570fbdfc19da76db4c9d0c6af4b8f`
- task contract:
  `docs/vega-r31-sidebar-typography-revert-pinned-leading.md`

## Results

| Requirement | Exact command | Result |
|---|---|---|
| Compact mounted typography and Pinned leading edge | `cargo test -p vega_ui r31_ -- --nocapture` | PASS, 2/0 |
| Compact typed tokens | `cargo test -p vega_theme r31_sidebar_typography_restores_compact_sizes -- --nocapture` | PASS, 1/0 |
| Organization mounted regressions | focused R15/R26/R28/R29/R31 suite | PASS, 13/0 |
| Complete UI crate | `cargo test -p vega_ui` | PASS, 177/0 |
| Theme crate | `cargo test -p vega_theme` | PASS, 11/0 |
| Formatting | `cargo fmt --all -- --check` | PASS |
| Strict workspace lint | `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| Complete workspace, serial | `cargo test --workspace -- --test-threads=1` | PASS |
| Candidate package | `cargo xtask package` | PASS |

## Native acceptance

- PASS: Sidebar primary text is restored to 13px and section/metadata text to
  12px; the R30 increase is no longer visible.
- PASS: every visible production Pinned title begins at the same left edge as
  the `Pinned` section label, with no icon or empty placeholder.
- PASS: Project labels, project-child tasks, and Recents tasks retain their
  existing content grid; fixed project metadata and time/action tails remain
  stable.
- PASS: Light and Dark appearances remain readable. Appearance was restored to
  Follow System.

The final local-`master` installed executable SHA256 is
`fdda39cc371d7cdb7d937d2aecd3adb8f5a13abd1f7cd509a2c3d61435315172`.
The previous app is recoverable at
`/tmp/vega-r31-master-install.woYzEu/Vega.app.previous`.

No push or remote release was performed. Scope deviation: none.

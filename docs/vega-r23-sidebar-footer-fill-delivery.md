# Vega R23 Sidebar footer fill — delivery

R23 turns the persistent Settings control into a full Sidebar footer row. It
keeps the existing Settings route and shared ghost-button states while using
the same 32px height and inner-column width as the other Sidebar navigation
rows.

## Freeze

- verified_at_utc: 2026-09-10T07:16:03Z
- verified_at_local: 2026-09-10T15:16:03+08:00
- branch: `feat/r23-sidebar-footer-fill`
- implementation commit: `d88d4cba6ce22b41b8e5d954a02187ad5cb1c637`
- pre-delivery spec-and-implementation diff SHA256:
  `af0f3284e42dcda681a2f4e7a037f0d27c829ab8467c7c896e1b06e26554a0e8`
- task contract: `docs/vega-r23-sidebar-footer-fill.md`
- environment: Darwin arm64; rustc 1.98.0; cargo 1.98.0; Git 2.55.0

## Changed files

- `crates/vega_ui/src/sidebar/mod.rs`: gives `sidebar-settings` a stable debug
  selector, full inner-column width, and the shared 32px Sidebar row height.
- `crates/vega/src/window/workspace.rs`: extends the production-root visual
  regression through the 240px, 304px, and 365px Sidebar widths.
- `docs/vega-r23-sidebar-footer-fill.md`, the design guidelines, UI spec, and
  README: freeze the footer-row contract and its documentation entry point.

No dependency, lockfile, migration, runtime/provider, store, credential, or
database change was made. No local color literal or geometry constant was
introduced.

## Results

| Requirement | Evidence class | Exact command | Result |
|---|---|---|---|
| Production Sidebar geometry | E2E-REAL | `cargo test -p vega --bin vega window::workspace::tests::r21_shell_mounts_resizable_sidebar_and_exact_environment_boundaries -- --exact --test-threads=1` | PASS, 1/0; 240/304/365px |
| Formatting | STATIC | `cargo fmt --all -- --check` | PASS, empty output |
| Strict lint | STATIC | `cargo clippy --all-targets -- -D warnings` | PASS; only the existing external `block v0.1.6` future-incompatibility notice |
| Complete workspace | MIXED | `cargo test --workspace` | PASS, 1001/0 |
| Candidate package | BUILD | `cargo xtask package` | PASS; signed bundle and plist valid |

The installed candidate executable matched the packaged executable with
SHA256 `1c36371f618d9dd792324152801115e109571367f131448b5d454e0fd35e3aad`.
The previous application bundle is recoverable at
`/tmp/vega-r23-install.PcnUbF/Vega.app.previous`.

## Native acceptance

- PASS: Light appearance idle and hover states. The hover surface fills the
  Sidebar inner content column and the control reads as a normal 32px row.
- PASS: Dark appearance hover state remains readable and has the same bounds.
- PASS: clicking the footer opens the existing General Settings page.
- PASS: the appearance preference was restored to Follow System after the
  check.

No push or remote release was performed. Scope deviation: none.

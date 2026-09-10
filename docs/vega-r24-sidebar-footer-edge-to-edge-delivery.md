# Vega R24 Sidebar footer edge-to-edge correction — delivery

R24 corrects the earlier false-positive interpretation of Sidebar footer
"fill." The visible Settings hover/active surface now spans the complete
Sidebar width, meets the bottom boundary, and remains a 32px square-cornered
footer strip. The inner control retains the existing Settings route, keyboard
focus, accessibility label, and tooltip.

## Freeze

- verified_at_utc: 2026-09-10T09:36:04Z
- verified_at_local: 2026-09-10T17:36:04+08:00
- branch: `feat/r24-sidebar-footer-edge-to-edge`
- contract commit: `b0f2248d8163266f184686231ba4d08af2058059`
- implementation commit: `cfb3a03f51363c2bb36094d4c4aab4ffd52fe04d`
- pre-delivery contract-and-implementation diff SHA256:
  `f57cb6c2158429eeb201f6f2259e217584b0ee84ddf1e06c6b20424153b03779`
- task contract: `docs/vega-r24-sidebar-footer-edge-to-edge.md`
- environment: Darwin arm64; rustc 1.98.0; cargo 1.98.0; Git 2.55.0

## Changed files

- `crates/vega_ui/src/sidebar/mod.rs`: moves ordinary Sidebar content into its
  own 12px-inset column, then mounts a root-level 32px Settings surface. The
  outer surface owns hover/active painting while a transparent full-size Button
  keeps interaction, focus, accessibility, and tooltip behavior.
- `crates/vega/src/window/workspace.rs`: separates painted-surface and
  interactive-button selectors, verifies both layers at 240px, 304px, and
  365px widths, preserves the New Task inset, and exercises the real Settings
  route.
- The R24 contract, design guidelines, UI spec, README, and this report record
  the corrected edge-to-edge requirement and acceptance evidence.

No dependency, lockfile, migration, runtime/provider, store, credential, or
database change was made. No local color literal or geometry constant was
introduced.

## Results

| Requirement | Evidence class | Exact command | Result |
|---|---|---|---|
| Production Sidebar surface, button geometry, widths, and route | E2E-REAL | `cargo test -p vega window::workspace::tests::r21_shell_mounts_resizable_sidebar_and_exact_environment_boundaries -- --exact` | PASS, 1/0; 240/304/365px |
| Formatting | STATIC | `cargo fmt --all -- --check` | PASS, empty output |
| Strict lint | STATIC | `cargo clippy --all-targets -- -D warnings` | PASS; only the existing external `block v0.1.6` future-incompatibility notice |
| Complete workspace | MIXED | `cargo test --workspace` | PASS, 1001/0 |
| Candidate package | BUILD | `cargo xtask package` | PASS; signed bundle and plist valid |

The installed candidate executable matches the packaged executable with SHA256
`9876c757ad55c5a557176542cba6be7420a89ef1c6ed08783efb4ac675c219d3`.
The immediately previous application bundle is recoverable at
`/tmp/vega-r24-visible-install.EaSGB3/Vega.app.previous`.

## Native acceptance

- PASS: Light appearance hover paints from the Sidebar's left edge through its
  right edge and meets the window bottom with no inset capsule or outer radius.
- PASS: a click at the footer's right edge opens the existing General Settings
  page, proving that the edge area is interactive rather than decorative.
- PASS: Dark appearance uses the same bounds and remains readable.
- PASS: the appearance preference was restored to Follow System after the
  check, and the app was returned to its main workspace.

The first R24 candidate exposed why the earlier result looked unchanged: its
test measured the component's layout selector, not an independently painted
surface. Native inspection rejected that candidate. The final implementation
and regression test now model the painted surface and interactive control as
separate layers.

No push or remote release was performed. Scope deviation: none.

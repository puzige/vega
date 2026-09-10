# Vega R22 workspace panel chrome polish — delivery

R22 removes the accidental visual distribution in the integrated terminal
panel while preserving its real PTY and Workspace behavior. Pane-level actions
now form one trailing group; terminal status stays left while copy/restart form
one compact trailing group; the terminal canvas uses a deliberate inset whose
actual bounds continue to drive PTY rows and columns.

## Freeze

- verified_at_utc: 2026-09-10T06:46:20Z
- verified_at_local: 2026-09-10T14:46:20+08:00
- branch: `feat/r22-workspace-panel-polish`
- implementation source: the feature commit immediately preceding this report
- pre-delivery spec-and-implementation diff SHA256:
  `d73f56d8f7bd8d200767a82f76f5fe923c6135e886f935293c7596bfb2342d9b`
- task_contract: `docs/vega-r22-workspace-panel-polish.md`
- environment: Darwin arm64; rustc 1.98.0; cargo 1.98.0; Git 2.55.0

## Changed files

- `crates/vega_theme/src/lib.rs`: freezes the 32px terminal toolbar token.
- `crates/vega_ui/src/terminal.rs`: groups status and terminal actions, applies
  12px horizontal / 8px vertical canvas inset, and extends the mounted real-PTY
  regression through resize, copy and restart.
- `crates/vega/src/window/workspace.rs`: makes pane actions an explicit trailing
  group and extends production-root geometry coverage for right/bottom docks,
  Review and narrow tab overflow.

No dependency, lockfile, migration, runtime/provider, store, credential or
database change was made. New `unwrap`/`expect` calls are confined to tests; no
new hard-coded color or font-size value was added.

## Results

| Requirement | Evidence class | Exact command | Result |
|---|---|---|---|
| Token freeze | UNIT | `cargo test -p vega_theme r22_terminal_toolbar_height_is_frozen` | PASS, 1/0 |
| Mounted terminal geometry and real PTY behavior | E2E-REAL | `cargo test -p vega_ui production_terminal_input_handler_and_keys_reach_real_pty -- --test-threads=1` | PASS, 1/0 |
| Production Workspace root | E2E-REAL | `cargo test -p vega --bin vega window::workspace -- --test-threads=1` | PASS, 5/0 |
| Formatting | STATIC | `cargo fmt --all -- --check` | PASS, empty output |
| Strict lint | STATIC | `cargo clippy --all-targets -- -D warnings` | PASS; only the existing external `block v0.1.6` future-incompatibility notice |
| Complete workspace | MIXED | `cargo test --workspace` | PASS, 1001/0 |
| Runtime dependency direction | STATIC | `cargo tree -p vega_runtime --depth 1` plus scoped match | PASS; no GPUI/UI/theme dependency |
| Candidate package | BUILD | `cargo xtask package` | PASS; signed bundle and plist valid |

The main agent independently reran all three focused commands after diff review.
The installed candidate executable matched the packaged executable with SHA256
`6ed67b6cdca90e6721db529aaf9eea5bf720695655db2edc7252b8eb81542051`.

## Native acceptance

- PASS: current-window Light bottom dock. Copy and restart stay together at the
  right; no action floats in the visual center; prompt content shares the
  specified inset.
- PASS: current-window Light right dock. Tab and pane actions stay in the 40px
  Workspace row; terminal-specific actions remain in the 32px content row.
- PASS: current-window Dark right dock. Separators, selected tab, status,
  actions and terminal content remain readable without a light-only shadow.
- PASS: terminal moved back to the bottom and theme restored to Follow System
  after acceptance.

## Preserved failures and residuals

- The first compile failed because GPUI `Canvas` does not expose
  `debug_selector`. The selector moved to an inner wrapper with the same bounds;
  the Canvas remains `size_full`, and production paint/PTY sizing still consumes
  the real Canvas bounds.
- An early test revision waited for an additional shell marker and timed out.
  The final regression compares the current real screen projection with the
  clipboard result directly; no production timeout or terminal logic changed.
- LIMIT: exact 960x600 native resizing was not completed because the CUA resize
  gesture could not resolve the window corner. The production-root mounted
  regression covers 960x600 tab reveal and dock geometry; this is not presented
  as native visual evidence.
- LIMIT: multiple native terminal tabs were not opened during this acceptance;
  the production-root regression mounts a real terminal beside an existing file
  tab and proves selected-tab reveal without reordering.
- No push or remote release was performed. Scope deviation: none.

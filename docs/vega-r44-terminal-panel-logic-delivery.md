# Vega R44 terminal panel interaction model — delivery

R44 replaces the terminal panel's overlapping hide, create, restore and focus
behaviors with one deterministic interaction model. Global reveal paths keep
task input in the Composer; only explicit terminal activation routes input to
the PTY. Dock, maximize, restore and responsive recovery preserve the selected
terminal entity and process.

## Freeze

- verified_at_utc: `2026-09-11T15:40:06Z`
- verified_at_local: `2026-09-11T23:40:06+0800`
- branch: `feat/r44-terminal-panel-logic`
- implementation commit: `e0ed480`
- local master at native acceptance: `e0ed480`
- implementation range: `d8e0be2..e0ed480`
- implementation diff SHA-256:
  `425e8749e38c115707e965e3178deab0376c76f4c8e15a5735de677a0bbbaf5b`
- task contract: [R44 terminal panel interaction model](vega-r44-terminal-panel-logic.md)
- design contract: [Vega design guidelines](vega-design-guidelines.md)
- environment: macOS 15.7.9 arm64; Rust/Cargo 1.98.0; Git 2.55.0

## Root cause and final model

The defect was not one spacing bug. Four state machines were exposed as one
panel:

- pane visibility, tab selection, creation and destructive management shared
  overlapping controls;
- global reveal implicitly focused the PTY, so task-like text could be sent to
  `zsh` without explicit terminal activation;
- `hidden == false` was treated as rendered visibility, leaving a right-docked
  terminal logically visible after responsive layout had unmounted it;
- closing the selected tab jumped to insertion order instead of an adjacent
  same-pane sibling.

The final contract gives each action one meaning: the leading chevron hides;
Plus creates; the tab strip selects and closes; Dock only moves; Maximize and
Restore activate the visible Workspace content; global Terminal, `Command-J`
and Environment reveal or create while preserving Composer focus.

## Changed surface

- `crates/vega/src/window/workspace.rs`: simplifies panel actions, centralizes
  terminal reveal and focus semantics, adds responsive right-to-bottom
  recovery, preserves terminal identity and selects an adjacent tab on close.
- `docs/vega-r44-terminal-panel-logic.md`: freezes the interaction and native
  acceptance contract.
- `docs/vega-design-guidelines.md`: records the resulting reusable Workspace
  interaction rule.

No dependency, lockfile, database, migration, credential, provider, color or
font-size change was introduced.

## Automated results

| Requirement | Evidence class | Exact command | Result |
|---|---|---|---|
| R44 production handlers and mounted render tree | E2E-REAL | `cargo test -p vega r44_ -- --nocapture` | PASS, 2/0 |
| Adjacent same-pane selection after close | UNIT | `window::workspace::tests::r44_closing_selected_tab_chooses_the_nearest_same_pane_sibling` | PASS |
| Reveal, create, focus, dock, maximize, restore and narrow recovery | E2E-REAL | `window::workspace::terminal_tests::r44_terminal_entry_points_and_creation_menu_preserve_explicit_focus` | PASS |
| Formatting | STATIC | `cargo fmt --all -- --check` | PASS |
| Strict lint | STATIC | `cargo clippy --all-targets -- -D warnings` | PASS; only the existing external `block v0.1.6` future-compatibility notice |
| Complete workspace | MIXED | `cargo test --workspace` | PASS; all unit, integration and doc tests |
| Candidate package | BUILD | `cargo xtask package` | PASS; bundle, plist and signature validation |
| Installed identity | E2E-REAL | strict codesign plus packaged/installed SHA-256 comparison | PASS; both executables `3b15b5f8985f28d03114ca3d03cedadb49decf5eebd58e02b9cef2247a85fdc2` |

## One state / one test / one image

All screenshots below come from the final installed `/Applications/Vega.app`.
The comprehensive production-root R44 test mounts the same handlers and render
tree; the dedicated close test owns the multi-tab adjacency rule.

| State | Production assertion | Native result and image |
|---|---|---|
| Closed | one global Terminal entry and no Workspace pane | PASS — `/tmp/vega-r44-terminal-final/01-closed.png` |
| Bottom reveal | one leading hide, Plus/Dock/Max trailing group and Composer focus | PASS — `/tmp/vega-r44-terminal-final/02-bottom-revealed.png` |
| Creation menu | truthful create actions only; no tab list, preview restore or close-all | PASS — `/tmp/vega-r44-terminal-final/03-creation-menu.png`; the unbound New Task state truthfully exposes only New terminal, while the mounted test covers Review availability |
| Multi-tab close | next sibling at the same index, otherwise previous | PASS — `/tmp/vega-r44-terminal-final/04-multitab-before-close.png`, `/tmp/vega-r44-terminal-final/05-multitab-after-close.png` |
| Explicit PTY activation | terminal tab/canvas accepts real shell input | PASS — `/tmp/vega-r44-terminal-final/06-explicit-pty.png` |
| Hidden terminal | no duplicate generic restore action | PASS — `/tmp/vega-r44-terminal-final/07-hidden-no-duplicate-restore.png` |
| Global reveal | first `Command-J` reveals and subsequent unclicked typing remains in Composer | PASS — `/tmp/vega-r44-terminal-final/08-global-reveal-composer-focus.png` |
| Right dock | same selected terminal moves right; Dock leaves Composer focused | PASS — `/tmp/vega-r44-terminal-final/09-right-dock.png` |
| Maximized | selected visible terminal owns focus and accepts PTY input | PASS — `/tmp/vega-r44-terminal-final/10-maximized.png` |
| Restored | selected terminal still owns focus and accepts PTY input | PASS — `/tmp/vega-r44-terminal-final/11-restored-right.png` |
| Responsive unmount | 960x600 removes the unavailable right pane without discarding its entity | PASS — `/tmp/vega-r44-terminal-final/12-narrow-right-unmounted.png` |
| Narrow recovery | first `Command-J` migrates the same terminal to bottom and preserves Composer focus | PASS — `/tmp/vega-r44-terminal-final/13-narrow-recovered-bottom.png` |
| PTY preservation | explicit terminal activation after recovery reaches the original shell process | PASS — `/tmp/vega-r44-terminal-final/14-narrow-same-pty.png` |
| Environment idempotency | Local terminal reveals the existing tab, creates no duplicate and preserves Composer focus | PASS — `/tmp/vega-r44-terminal-final/15-environment-idempotent-composer-focus.png` |

## Focus and process probes

- Global reveal: `R44_GLOBAL_REVEAL_COMPOSER` appeared in the Composer without
  a terminal click.
- Dock: `R44_DOCK_COMPOSER_OK` appeared in the Composer after moving the pane
  right.
- Environment: `R44_ENV_COMPOSER_OK` appeared in the Composer and the tab strip
  still contained only `终端 1`.
- Maximize: `/tmp/vega-r44-terminal-final/max-focus.txt` contains
  `R44_MAX_FOCUS_OK`.
- Restore: `/tmp/vega-r44-terminal-final/restore-focus.txt` contains
  `R44_RESTORE_FOCUS_OK`.
- Responsive recovery: both
  `/tmp/vega-r44-terminal-final/pty-pid-before.txt` and
  `/tmp/vega-r44-terminal-final/pty-pid-after.txt` contain PID `63805` and have
  the identical SHA-256
  `e2820f3c315fe4664dde2f535a578e08f31b40d7f552c8a6e0f57112467e0b51`.

## Installation and rollback

- Installed application: `/Applications/Vega.app`
- Packaged candidate: `/Users/puzige/Workspace/vega/dist/Vega.app`
- Installed and packaged executable SHA-256:
  `3b15b5f8985f28d03114ca3d03cedadb49decf5eebd58e02b9cef2247a85fdc2`
- Recoverable checkpoints:
  - `/Users/puzige/.Trash/Vega-before-r44.app`
  - `/Users/puzige/.Trash/Vega-r44-pre-max-focus.app`
  - `/Users/puzige/.Trash/Vega-r44-before-narrow-recovery.app`

## Residuals

- Spec deviations: none.
- Known external notice: `block v0.1.6` is future-incompatible with a later Rust
  release; it does not fail the current strict lint or test gates.
- No remote push or release was performed.

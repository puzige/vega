# Vega R46 window-anchored shell slots — delivery

R46 fixes the R45 defect where opening any panel dragged the three header
shell controls sideways and painted panel surfaces over them. The cluster is
now window-anchored and every header that can occupy the window's top band
reserves the cluster's trailing band, matching the layout model measured on
Codex Desktop.

## Freeze

- verified_at_utc: `2026-09-11T21:45:46Z`
- verified_at_local: `2026-09-12T05:45:46+0800`
- branch: `feat/r45-shell-controls-composer`
- implementation commits: `950586b`, `b777605`, `ee9958d`
- task contract: [R46 window-anchored shell slots](vega-r46-window-anchored-shell-slots.md)
- design contract: [Vega design guidelines](vega-design-guidelines.md) v1.29
- base: local `master` at `d7cb2d6` (R45)
- implementation range: `d7cb2d6..ee9958d`
- implementation diff SHA-256:
  `8ae38701021a83784103673e1e5fc06316dcea75108ef3f545f50a63287915de`
- environment: macOS 15.7.9 arm64; Rust/Cargo 1.98.0; Git 2.55.0

## Root cause

R45 mounted the slot cluster inside `main-header`, which is the first child of
the `flex_1` conversation column in `render_workspace`'s row. The Environment
rail (320px) and the right workspace pane are *siblings* of that column, so
opening either one shrank the column and dragged the cluster leftwards. The
rail card also started at y=14 while the header band is 46px tall, so the card
painted over the header row containing the controls.

Native measurement of the R45 build at 1403×860 (slot icon centers, logical
px) — the defect table:

| State | R45 (defective) | R46 (fixed) |
|---|---|---|
| no panel | 1308.8 / 1342.8 / 1376.8 | 1308.5 / 1342.5 / 1376.5 |
| Environment rail | 988.8 / 1022.8 / 1056.8 (**−320**) | 1308.5 / 1342.5 / 1376.5 |
| Environment overlay | n/a (card covered the header) | 1308.5 / 1342.5 / 1376.5 |
| right pane | 859.8 / 893.8 / 927.8 (**−449**) | 1308.5 / 1342.5 / 1376.5 |
| rail + bottom dock | — | 1308.5 / 1342.5 / 1376.5 |
| 960×600 | — | same trailing inset as wide |

The fix is structural, not a coordinate patch: the cluster moved out of the
conversation column entirely and is mounted on the window-level row.

## Adjudicated design decision

Anchoring the cluster to the window's top-right corner puts it over the right
pane's own trailing actions, which R45's "disabled slot ignores activation"
rule then blocks. Codex resolves the same collision by making the three toggles
the **trailing group of the rightmost header row**. Measured on the Codex
right-panel reference (logical px): pane `+` at 1252.2, toggles at 1310.8 /
1346.8 / 1380.8 — one row, pane actions left, toggles right, no overlap.

R46 §2.1.1 adopts that model: `workspace_pane_header_in_top_band(bottom)`
decides whether a pane header opens the top band (`!bottom` — the docked right
pane — or a maximized dock), and such headers reserve
`Layout::SHELL_SLOT_CLUSTER_RESERVE` so their own trailing actions lay out to
the left of the slots.

## Changed surface

- `crates/vega/src/window/render.rs`: extracted `render_shell_slot_cluster`
  (window-level `absolute().top_0().right_0()`, 46px band, `.occlude()`),
  mounted on the outermost window row for every route except Settings;
  `render_main_header` reserves `SHELL_SLOT_CLUSTER_RESERVE` instead of `pr_3`.
- `crates/vega/src/window/workspace.rs`: `workspace_pane_header_in_top_band`
  + `workspace_fullscreen_index` shared predicates; pane headers in the top
  band reserve the cluster band; rail and overlay card top offset =
  `MAIN_HEADER_HEIGHT + ENVIRONMENT_CARD_INSET`; three new `r46_` tests.
- `crates/vega_theme/src/lib.rs`: `Layout::SHELL_SLOT_CLUSTER_RESERVE = 108.0`
  (3×28 + 2×6 + 12px trailing inset).
- `docs/vega-design-guidelines.md`: v1.29 — window-anchoring rule and the
  top-band reservation rule.

No dependency, lockfile, database, migration, credential, provider, color or
font-size change. R44/R45 semantics, tooltips, focus contracts and the
Composer geometry are untouched.

## Automated results

| Requirement | Evidence class | Exact command | Result |
|---|---|---|---|
| Window anchoring across six states, plus size-invariant trailing inset | E2E-REAL (mounted render tree) | `cargo test -p vega r46_slot_cluster_is_window_anchored` | PASS |
| Overlay/rail below the header band, no intersection with slots | E2E-REAL | `cargo test -p vega r46_overlay_and_rail_start_below_header_band` | PASS |
| Top-band pane headers reserve the cluster band | E2E-REAL | `cargo test -p vega r46_top_band_headers_reserve_the_cluster` | PASS |
| Overlay open: slot clicks still work | E2E-REAL | `cargo test -p vega r46_overlay_open_slot_clicks_still_work` | PASS |
| R45 semantics unchanged | E2E-REAL | `cargo test -p vega r45_` | PASS |
| R44 terminal contract unchanged | E2E-REAL | `cargo test -p vega r44_` | PASS |
| Formatting | STATIC | `cargo fmt --all -- --check` | PASS |
| Strict lint | STATIC | `cargo clippy --all-targets -- -D warnings` | PASS; only the pre-existing external `block v0.1.6` notice |
| Complete workspace | MIXED | `cargo test --workspace` | PASS; 1037 passed, 0 failed |
| Candidate package | BUILD | `cargo xtask package` | PASS; strict codesign OK |

Non-vacuity: re-mounting the cluster inside `main-header` (the R45 defect)
fails the anchoring test with the documented 320px displacement; removing the
pane-header reservation fails the reserve test and the r21 pane-action click.

Test changes beyond additions: two assertions pinned geometry this contract
intentionally moves — the R21 "Environment card top inset" (now
`MAIN_HEADER_HEIGHT + ENVIRONMENT_CARD_INSET`, with a new
`card.top() - header.bottom() == ENVIRONMENT_CARD_INSET`) and one pane-header
trailing inset that pinned the former `px_1` value (now
`SHELL_SLOT_CLUSTER_RESERVE`). Every other `r21_`/`r44_`/`r45_` assertion is
unmodified; the r21 `workspace-review-commit` click passes untouched because
the action now lays out left of the reserved band.

## One state / one image

All images from the final installed `/Applications/Vega.app`
(SHA-256 below), captured with `screencapture -l` at 1403×860 (native 1x on
this display) and 960×600, saved under `/tmp/vega-r46-anchored/`.

| State | Native result and image |
|---|---|
| No panel | PASS — `01-anchored-closed.png`; slots 1308.5 / 1342.5 / 1376.5 |
| Environment rail | PASS — `02-anchored-rail.png`; slots **identical**; card starts below the header band |
| Environment overlay | PASS — `06-overlay-below-header.png`; slots **identical**; card hangs below the header, covers nothing |
| Right pane | PASS — `03-anchored-right.png`; slots identical; pane actions (`+`/dock/maximize) sit left of the slots in the same row |
| Rail + bottom dock | PASS — `04-anchored-both.png`; slots identical |
| 960×600 narrow | PASS — `05-anchored-narrow.png`; three slots intact at the same trailing inset |

The user's reported symptom — "点了之后它直接把那几个按钮挤过去的" — is
reproduced in the R45 column of the table above and eliminated in the R46
column, confirmed both by measurement and visually.

## Installation and rollback

- Installed application: `/Applications/Vega.app`
- Packaged candidate: `/Users/puzige/Workspace/vega-r45-shell-composer/dist/Vega.app`
- Packaged and installed executable SHA-256 (identical):
  `9267d4936f9c6f49b58825105e3387397d451d58b7b686c6e70e724fb16cdaf1`
- Previous install removed before replace:
  `/Users/puzige/.Trash/Vega-before-r46.app` (R45 binary
  `3f465d4abae943242d1f796550001807dfb74e2635b26b0d02f59376051cae72`)
- Rollback: `mv /Users/puzige/.Trash/Vega-before-r46.app /Applications/Vega.app`
  and reset `master` to `d7cb2d6` (no remote push performed).

## Residuals and spec deviations

- Spec deviations: none.
- ACCEPTED (test flakiness, pre-existing): the workspace suite is flaky under
  parallel load. During this round
  `git_workspace::trusted_git::tests::runner_mutation::service_cancel_after_real_add_or_commit_returns_authoritative_state_once`
  failed once with `process_control_failed`. Verified unrelated: it passes 3/3
  in isolation on the untouched tree, is a git process-control test with no UI
  surface, and the full suite re-run passed 1037/0. Not introduced by R46.
- ACCEPTED (tooling, carried from R45): the audit harness cannot deliver a
  native synthetic Escape keystroke; the overlay Escape path remains proven by
  the production test's in-process dispatch, and the outside-click path is
  natively proven.
- KNOWN (unchanged): Vega's header controls are still absent from the macOS
  accessibility tree (GPUI a11y surface is empty beyond the sidebar), so this
  round's verification used screen capture plus geometry measurement.
- EXTERNAL: the inert marker `R45` left at the Codex Desktop bottom-terminal
  prompt during the R45 audit is still present (never executed; dismiss with
  Ctrl+C or backspace).

## Merge status

Ready to fast-forward into local `master`. No remote push, no MR.

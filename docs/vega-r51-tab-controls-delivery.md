# R51 Tab controls delivery

## Scope

- Updated the existing workspace tab renderer in `crates/vega/src/window/workspace.rs`.
- Added only R51 tab geometry tokens and freeze coverage in `crates/vega_theme/src/lib.rs`.
- Preserved the existing active-fill branch (`colors.bg_active`) for R50 integration.
- Added R51 production-harness geometry and inactive-close focus/hover regressions.

No Browser, Files, or Side chat placeholders, shortcuts, menu policy, outer header geometry, sidebar rhythm, or terminal/process contracts were added or changed.

## Freeze

- verified_at_utc: 2026-09-13T04:28:15Z
- verified_at_local: 2026-09-13 12:28:15 CST
- branch: `feat/r51-tab-controls`
- git_head: local feature branch (OID intentionally omitted from repository evidence)
- tracked_diff_sha256: intentionally omitted from repository evidence
- task_contract: `docs/vega-r51-tab-controls.md`
- os_arch: Darwin arm64
- rustc: 1.98.0
- cargo: 1.98.0

## Results

| requirement | evidence class | exact command | result | duration | bounded footer/hash |
|---|---|---|---|---:|---|
| Formatting | UNIT/PROPERTY | `cargo fmt --all -- --check` | PASS | ~1s | no output |
| TAB geometry token freeze | UNIT/PROPERTY | `cargo test -p vega_theme r51_workspace_tab_geometry_is_frozen` | PASS | ~1s | 1 passed, 17 filtered |
| Production tab row and close hitbox geometry | E2E-REAL | `cargo test -p vega --bin vega r51_workspace_tabs_keep_frozen_geometry_and_close_hitbox -- --nocapture` | PASS | ~1s | 1 passed, 101 filtered |
| Inactive close parent-focus and group-hover behavior | E2E-REAL | `cargo test -p vega --bin vega r51_inactive_close_follows_parent_focus_and_group_hover -- --nocapture` | PASS | ~10s including compile | 1 passed, 102 filtered |
| Selected-tab fallback after close | UNIT/PROPERTY | `cargo test -p vega --bin vega r44_closing_selected_tab_chooses_the_nearest_same_pane_sibling` | PASS | <1s | 1 passed, 102 filtered |
| Existing terminal tab focus/creation/close flow | E2E-REAL | `cargo test -p vega --bin vega r44_terminal_entry_points_and_creation_menu_preserve_explicit_focus -- --nocapture` | PASS on rerun | ~1s | 1 passed, 102 filtered |
| Focused clippy | UNIT/PROPERTY | `cargo clippy -p vega -p vega_theme --all-targets -- -D warnings` | PASS | ~3s | finished with no warnings |

The first run of the existing terminal-flow E2E failed before assertions because the initial implementation layered a second direct `.hover` style onto the shared `icon_button`; GPUI rejected the duplicate style. The implementation was corrected to use the tab group-hover state plus the existing shared icon hover behavior, and the exact command above was rerun successfully.

## Residuals

- ACCEPTED: Native macOS screenshot/visual checks were NOT RUN in this executor; the production UI harness verifies mounted geometry and interactions but is not native screenshot evidence.
- ACCEPTED: Full `cargo test --workspace` and full-workspace clippy were NOT RUN, per coordinator instruction to avoid competing fixture runs; the main agent owns the final workspace gate.
- ACCEPTED: The GPUI production harness exercises the parent-tab focus relationship and the inactive close's group-hover click path. The harness exposes mounted bounds and input behavior, not native pixels for opacity/focus styling; native macOS screenshot/visual checks were NOT RUN.
- LIMIT: The active tab fill remains the existing expression by design; R50 may adjust that expression independently during integration.

Spec deviations: none.

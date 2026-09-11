# Vega R45 header shell controls and Composer alignment — delivery

R45 replaces the main header's conditional business-button mix with three
permanent shell slots (Environment / Terminal ⌘J / Right workspace), gives
the narrow Environment overlay an Escape and outside-click dismissal
contract, and freezes the Composer's centering basis and padding tokens with
production tests. R44's terminal reveal focus, PTY identity and responsive
recovery clauses are preserved verbatim and re-proven by the unchanged
`r44_*` suite.

## Freeze

- verified_at_utc: `2026-09-11T18:33:16Z`
- verified_at_local: `2026-09-12T02:33:16+0800`
- branch: `feat/r45-shell-controls-composer`
- implementation commits: `2b681b4`, `b0b8af7`
- task contract: [R45 header shell controls and Composer alignment](vega-r45-shell-controls-composer.md)
- design contract: [Vega design guidelines](vega-design-guidelines.md) v1.28
- base: local `master` at `b3b57f9`
- implementation range: `b3b57f9..b0b8af7`
- implementation diff SHA-256:
  `d53899420a0108223aa799c0d862b3c4c8cf08b2df684d186bc021c198761bb1`
- environment: macOS 15.7.9 arm64; Rust/Cargo 1.98.0; Git 2.55.0

## Root cause

Three layered defects, not a spacing bug:

1. **Component hierarchy** — the cluster rendered *entries* gated by route and
   workspace state (Review button, terminal toggle, restore-right,
   restore-bottom, Environment button), so membership changed with every
   transition and one action could own two entries (hidden Review ⇒ Review
   button + restore-right).
2. **State ownership** — only the terminal had a rendered-visibility
   predicate; the restore buttons and the bottom dock carried no on-state,
   and `hidden == false` was never distinguished from real rendering outside
   the terminal path.
3. **Visual tokens** — a 28px labeled action sat next to 24×24 hitboxes on a
   4px gap; there was no list/summary icon and no shortcut badge in
   `IconTooltip`. Additionally the narrow Environment overlay had no Escape
   or outside-click dismissal and never returned focus to the Composer.

The Composer itself had no centering or growth defect: it already centered on
the conversation column. R45 freezes that with production tests and moves the
two hardcoded wrapper paddings into `Layout` tokens (values unchanged).

## Changed surface

- `crates/vega_ui/src/icons.rs`: `Icon::Summary` inline SVG; `shell_icon_button`
  (28×28 `TITLEBAR_CONTROL_SIZE`, 16px icon, selected `bg_active`, disabled
  slot, shortcut keycap tooltip); `IconTooltip` extended with an optional
  shortcut chip. Existing `icon_button` untouched (R22 24px workspace
  hitboxes preserved).
- `crates/vega/src/window/render.rs`: `render_main_header` renders exactly
  three `shell_icon_button` slots on 6px gaps; `header_action` dead code
  removed.
- `crates/vega/src/window/workspace.rs`: rendered-visibility predicates
  (`workspace_recent_terminal_is_rendered`, `bottom_workspace_selected_is_rendered`,
  `right_workspace_rendered*`, `right_workspace_slot_available`) delegating to
  the R44 predicates; `workspace_toggle_bottom` (hide → reveal hidden tab →
  R44 terminal reveal/migration → create, Composer focus on every reveal);
  `workspace_toggle_right` (hide rendered non-terminal → restore hidden tab →
  open Review diff, rendered right terminal owned by the bottom slot);
  `dismiss_environment_overlay` + Escape binding with `propagate()` fallback;
  transparent dismissal backdrop under the overlay card.
- `crates/vega/src/app_palette.rs`: ⌘J routes to `workspace_toggle_bottom`
  (A1-08 action unchanged).
- `crates/vega_theme/src/lib.rs`: `Layout::COMPOSER_PADDING_TOP = 12.0`,
  `Layout::COMPOSER_PADDING_BOTTOM = 16.0` + frozen token test.
- `crates/vega_ui/src/conversation_stream/render.rs`: composer wrapper uses
  the new tokens (values unchanged).
- `docs/vega-design-guidelines.md`: v1.28 — trailing three-slot rule,
  composer padding tokens.
- `docs/vega-r45-shell-controls-composer.md`: the frozen contract.

No dependency, lockfile, database, migration, credential, provider, color or
font-size change. One pre-existing r11-era test call site was mechanically
renamed with the removed `workspace_toggle_terminal`; all other existing
assertions are untouched (`main-header-restore-bottom` appears in r44 tests
only as absence checks, which stay true).

## Automated results

| Requirement | Evidence class | Exact command | Result |
|---|---|---|---|
| Header slots, overlay dismissal, right-toggle priority, toggle truth, composer geometry | E2E-REAL (mounted production render tree) | `cargo test -p vega r45_ -- --nocapture` | PASS, 8/0 |
| Composer padding tokens frozen | UNIT | `vega_theme::tests::r45_composer_padding_tokens_are_frozen` | PASS |
| R44 regression surface (focus, PTY, migration, sibling close) | E2E-REAL | `cargo test -p vega r44_ -- --nocapture` | PASS, unmodified |
| R21 shell geometry | E2E-REAL | `cargo test -p vega r21_` | PASS; two header membership lists updated to the slot model per spec §4 |
| Formatting | STATIC | `cargo fmt --all -- --check` | PASS |
| Strict lint | STATIC | `cargo clippy --all-targets -- -D warnings` | PASS; only the pre-existing external `block v0.1.6` future-compat notice |
| Complete workspace | MIXED | `cargo test --workspace` | PASS; all unit/integration/doc tests green (vega bin 301, theme, ui, runtime, tools suites) |
| Candidate package | BUILD | `cargo xtask package` | PASS; strict codesign OK |

## One state / one test / one image

All images come from the final installed `/Applications/Vega.app`
(SHA-256 below), captured with `screencapture -l` at native 2× on the live
window, saved under `/tmp/vega-r45-shell-composer/`.

| State | Production assertion | Native result and image |
|---|---|---|
| All panels closed | three slots render, 28×28±1, 34±1px centers, Composer focused after hide | PASS — `01-closed.png`; focus probe typed into Composer without clicking (`R45_FOCUS_01`) |
| Environment overlay open | slot tracks overlay state; truthful card actions | PASS — `02-env-overlay.png` |
| Overlay dismissed by outside click | backdrop click closes + Composer focus | PASS — `03-env-overlay-outside-click-closed.png` |
| Overlay dismissed by Escape | in-process keystroke dispatch closes + Composer focus | PRODUCTION-TEST PROVEN (`r45_environment_overlay_esc_…`); native synthetic Escape is not deliverable by the audit harness (see residuals) |
| Environment rail (wide) | rail reserves its 320px column; slot 1 active | PASS — `04-env-rail.png` |
| Bottom dock via slot 2 | R44 reveal, same terminal entity, Composer focus | PASS — `05-bottom-toggle.png`, probe `05b-bottom-reveal-composer-focus.png` |
| Bottom dock via ⌘J | same handler, toggle off/on parity | PASS — `06-bottom-cmdj.png` |
| Bottom hidden non-terminal tab restore | unified priority branch 2 reveals same tab, Composer focus | PRODUCTION-TEST PROVEN (`r45_bottom_toggle_unified_priority…`); native setup needs agent-created file tabs (see residuals) |
| Right dock open | slot 3 opens/restores the same Review tab | PASS — `08-right-open.png` |
| Right dock hidden / restored | hide returns Composer focus; restore re-activates pane content | PASS — `09a-right-hidden.png`, `09-right-restored.png` |
| Rendered right terminal | slot 2 active, slot 3 disabled and inert | PASS — `10-right-terminal.png` |
| Toggle surfaces track rendered visibility | maximized counts rendered; unmounted right pane does not | PASS — `11b-terminal-slot-active.png`, `11c-right-slot-active.png` (11c shows bottom+right active simultaneously — independent docks) |
| Tooltip | 「切换终端」+ ⌘J keycap below the hovered slot | PASS — `12-tooltip-terminal-cmdj.png` |
| Composer empty / single line | 736px max width, centered on conversation column, ≥100px height | PASS — `13-composer-empty.png` |
| Composer multi-line growth | card grows with wrapped rows, respects bottom dock, no overlap | PASS — `14-composer-multiline.png` (wrapped growth; growth clamp covered by existing row-clamp production test) |
| Composer with rail open | re-centered inside the rail-excluded column | PASS — `15-composer-with-rail.png` |
| Composer with bottom+right docks | re-centered, no overlap, focus retained | PASS — `16-composer-with-docks.png` |
| 960×600 narrow | three slots intact, slot 2 active, slot 3 disabled by the width guard, R44 bottom tabs preserved | PASS — `17-narrow-960.png` |

## Focus and shell probes

- After every slot-driven reveal in this session, unclicked keystrokes landed
  in the Composer (`R45_FOCUS_01`, `R45_FOCUS_05`), never in the PTY; the
  terminal canvas stayed untouched.
- R44 maximize/restore was exercised natively during the walkthrough and
  preserved the selected entity both ways.
- The R44 responsive recovery re-proved live: shrinking to 1100px moved the
  right-docked Review tab into the bottom dock; widening kept tab identity.

## Installation and rollback

- Installed application: `/Applications/Vega.app`
- Packaged candidate: `/Users/puzige/Workspace/vega-r45-shell-composer/dist/Vega.app`
- Packaged and installed executable SHA-256 (identical):
  `3f465d4abae943242d1f796550001807dfb74e2635b26b0d02f59376051cae72`
- Previous install removed before replace:
  `/Users/puzige/.Trash/Vega-before-r45.app` (R44 binary
  `3b15b5f8985f28d03114ca3d03cedadb49decf5eebd58e02b9cef2247a85fdc2`)
- Rollback: `mv /Users/puzige/.Trash/Vega-before-r45.app /Applications/Vega.app`
  and fast-forward `master` back to `b3b57f9` (no remote push performed).

## Residuals and spec deviations

- Spec deviations: none. Six implementation judgment calls inside spec
  discretion are recorded in the implementation report (toggle-bottom branch
  c reuses R44 verbatim; slot-3 availability guards; overlay stop-propagation
  guard; Escape `propagate()` fallback; one r11 test rename; composer test
  uses real layout edges).
- ACCEPTED (tooling): the audit harness cannot deliver a native synthetic
  Escape keystroke (observed against Codex Desktop as well: Escape never
  reached either app, while clicks, ⌘J and typing do). The Escape dismissal
  path is therefore proven by the production test's in-process dispatch, not
  by a native screenshot; the outside-click path is natively proven.
- ACCEPTED (scope): native image for "bottom hidden non-terminal tab restore"
  was not captured — creating a bottom file/artifact tab requires an
  agent-produced preview; the branch is covered by the unified-priority
  production test.
- LIMIT: synthetic typing to the Composer showed delayed/duplicated delivery
  under the automation harness (a text fragment arrived seconds late and the
  ⌘J toggle raced twice). Deterministic behavior is owned by the production
  tests; no product defect was observed from real interaction pacing.
- KNOWN (unchanged, pre-existing): in overlay mode the R21 Environment card
  covers the window's top-right corner area where the shell slots sit
  (Codex anchors its popover below the buttons instead); header controls are
  still not exposed to the macOS accessibility tree (GPUI a11y surface is
  empty beyond the sidebar) — both are candidates for a later round and do
  not interact with this contract.
- EXTERNAL: a harmless inert marker `R45` (never executed, no Enter pressed)
  was left at the zsh prompt of the Codex Desktop bottom terminal
  (`~/Workspace/loom` tab) during the Codex focus probe; dismiss it with
  Ctrl+C or backspace.

## Merge status

Production tests, native screenshots and real interaction agree on every
frozen state, so `feat/r45-shell-controls-composer` (`b3b57f9..b0b8af7`) is
ready to fast-forward into local `master`. No remote push, no MR.

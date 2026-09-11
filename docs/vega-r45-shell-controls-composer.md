# Vega R45 Header shell controls and Composer alignment

**Status:** implementation contract

**Scope:** the main-header trailing control cluster, the Environment overlay
dismissal contract, the unified bottom/right workspace toggles, and the
Composer geometry freeze that backs them.

**Evidence:** six user-provided Codex Desktop screenshots measured at original
resolution (2806×1718 @2x ≙ 1403×859 logical, the Vega acceptance size), plus
a live computer-use audit of Codex Desktop (`com.openai.codex`, window
1403×859) and of the installed Vega `b3b57f9` on 2026-09-12. Screenshots and
live observations are product-behavior evidence only; no third-party asset,
token or code was extracted.

**Preserves:** every clause of
[R44 terminal panel interaction model](vega-r44-terminal-panel-logic.md)
(reveal focus, PTY identity, unavailable-right recovery, adjacent sibling
selection, creation menu), [R21 environment geometry](vega-r21-screenshot-parity.md)
(320px rail, 1230px breakpoint, card inset/radius), and
[R43 titlebar control spacing](vega-r43-titlebar-control-spacing.md) for the
leading Sidebar/Search/Back/Forward group. No font size, theme color,
provider, database, migration or terminal PTY contract changes.

## 1. Audit result — Codex vs Vega state matrix

| Dimension | Codex (measured/live) | Vega `b3b57f9` (measured/live) |
|---|---|---|
| Cluster composition | Always exactly 3 icon slots: summary, bottom panel, side panel | 1–5 conditional entries: `Review` label button (project route), terminal toggle (project route), restore-right/restore-bottom (hidden non-terminal tab), Environment button (project route and right pane not visible) |
| Slot semantics | Each slot = one global layout surface; business content lives inside the panel as tabs | Business entries (Review, Environment) and layout toggles share one cluster; a hidden Review pane can be reachable from both the Review button and restore-right |
| Order | summary → bottom → side | Review → terminal → restore-right → restore-bottom → Environment (conditionals change membership, not just state) |
| Toggle state visibility | Every slot renders a persistent active surface while its surface is rendered (live AX: `Toggle summary = 1`, `Toggle bottom panel = 1`) | Only the Environment button gets `bg_active` while visible; terminal toggle and restore buttons have no on-state at all |
| Geometry | 28×28 active pill (56px @2x, both headers), icon centers 34px apart (68px @2x, measured twice) → 6px gaps; icon ≈13px | Mixed: labeled `header_action` (28px high) next to `icon_button` 24×24 hitboxes, `gap_1` (4px); centers uneven (Review→terminal ≈66px, terminal→environment 28px) |
| Tooltip | Below the hovered button: name plus keycap badge ("Toggle bottom panel ⌘J"); summary/side have no shortcut | `IconTooltip` text only ("切换终端"), no shortcut badge |
| Summary surface | Floating popover anchored under the button, overlays content, reserves no layout column; Esc closes and resets the AX checkbox to 0; focus moves into the popover | No summary slot. Nearest surface is Environment: docked rail ≥1230px (R21, reserves a 320px column) or absolute overlay card below 1230px; **Esc and outside-click do not close the overlay** |
| Bottom toggle | One checkbox, ⌘J; reveal focuses the terminal input (live probe: typed marker landed in the shell) | One toggle with R44 semantics: reveal keeps **Composer** focus (deliberate safety deviation, kept); but bottom restore for a hidden non-terminal bottom tab is a *separate* restore-bottom entry |
| Right toggle | One checkbox; when open, bottom+side buttons re-anchor into the right panel header row at the window's top-right corner; reveal keeps Composer focus (live AX: textarea focused) | No generic toggle; Review button opens the diff pane; restore-right appears only when a hidden non-terminal right tab exists; restore focuses the pane content (kept) |
| Bottom + Right | Independent states, any combination legal (AX checkboxes), summary composes with both | Independent state arrays, but the cluster cannot express "both open" stably because buttons appear/disappear per state |
| AX exposure | Named checkboxes with live values, shortcut in tooltip | Header cluster absent from the AX tree; only sidebar/window controls exposed |
| Composer | Centered on the conversation column (between sidebar and side panel), above the bottom panel; action row `+ / permission … model / mic / send` | Same centering basis by construction (`mx_auto` inside the conversation column, `flex_1` sibling of rail/pane); action row `+ / Execute / 确认 … model / provider / send`; wrapper paddings `12/16` are hardcoded literals, not tokens |

## 2. Root causes

1. **Component-hierarchy error (primary).** The cluster renders *entries*
   instead of *slots*: each child appears only while its precondition holds
   (`review_available`, `project_route`, `hidden_workspace_available(i)`,
   `!right_visible`). One layout system is expressed as a variable-length list
   of business buttons, so membership — not just state — changes with every
   route/panel transition, and two entries can expose one action (hidden
   Review ⇒ Review button + restore-right).
2. **State-ownership error.** "Rendered visibility" is only computed for the
   terminal (`workspace_terminal_is_rendered`). The Environment button's
   on-state uses rail/overlay visibility, and the bottom/right restore buttons
   carry no on-state at all, so open panels are not reflected on their toggles
   (Codex drives every toggle from one rendered-visibility predicate per dock,
   including the maximized case).
3. **Visual-token error.** The cluster mixes a 28px labeled action with 24px
   `icon_button` hitboxes on a 4px gap, while R43 froze 28×28 + 16px icons for
   titlebar controls and Codex measures 28×28 pills on 6px gaps. There is no
   list/summary icon, and `IconTooltip` cannot carry a shortcut badge.
4. **Missing dismissal contract.** The Environment overlay is a transient
   card, but only its own close button, the header toggle, route changes or
   opening a right pane dismiss it — Esc and outside clicks do nothing, and
   focus is not returned to the Composer.

The Composer itself has **no centering or growth defect**: it already centers
on the conversation column (`mx_auto` inside the `flex_1` column that also
hosts the header), re-centers when the rail or right pane opens, and grows to
a frozen 8-row cap. R45 freezes that behavior with production tests and moves
two hardcoded wrapper paddings into `Layout` tokens; it does **not** reshape
the Composer or its action row.

## 3. Contract

### 3.1 Three stable shell slots

The main-header trailing cluster renders **always exactly three icon slots,
in this order, at every route and window size ≥960px**:

| Slot | Selector | Surface it owns | Icon | Tooltip (label + badge) |
|---|---|---|---|---|
| 1 | `main-header-environment` | Environment (rail ≥1230px per R21, overlay card below) | new `Icon::Summary` (16px, two-row list glyph in the shared 24×24/2px-stroke Lucide grammar) | 「切换环境」 |
| 2 | `main-header-terminal` | Bottom workspace dock (R44 global terminal entry) | `Icon::DockBottom` (Codex bottom-panel glyph parity; replaces `Icon::Terminal`) | 「切换终端」+ `⌘J` keycap |
| 3 | `main-header-workspace-right` | Right workspace dock | `Icon::DockRight` | 「切换右侧面板」 |

- All three use one new shared control, `vega_ui::icons::shell_icon_button`:
  28×28 hitbox (`Layout::TITLEBAR_CONTROL_SIZE`), 16px centered icon, 6px
  adjacent gap (34px centers — the Codex-measured value; the R43 4px rule
  stays scoped to the leading Sidebar/Search/Back/Forward group), hover
  `bg_hover`, keyboard focus ring unchanged, persistent `bg_active` surface
  while the owned surface is rendered, disabled presentation (tertiary icon,
  no hover surface, activation ignored) when the action is unavailable.
  A disabled slot keeps its geometry — the grid never reflows.
- `gap_1` in the cluster becomes `px(6.)`; the old conditional children
  (`main-header-review`, `main-header-restore-right`,
  `main-header-restore-bottom`) are removed.
- Each slot has exactly one stable meaning: slot 1 = Environment surface,
  slot 2 = bottom dock, slot 3 = right dock. Business content (Review,
  files, artifacts, terminals) is created and selected inside the dock it
  belongs to (creation menu per R44), never by a second header entry.

### 3.2 Slot 1 — Environment

- Enabled iff a project route exists and the right dock is not rendered
  (Environment is replaced by the right workspace, R21); otherwise disabled.
- Clicking toggles the same state as today (`environment_collapsed` wide /
  `environment_overlay_open` narrow); rail geometry, breakpoint and card
  styling are untouched.
- **Overlay dismissal contract (new):** while the overlay card is open,
  pressing Escape or pressing the mouse inside the conversation/sidebar area
  outside the card closes it and returns focus to the Composer, exactly like
  dismissing an R44 creation menu. The rail modality gains no Escape path
  (it is persistent docked chrome; its close button and slot 1 remain).
- The button shows `bg_active` whenever the surface is rendered in either
  modality (already true; now also covered by a production test so overlay
  and rail cannot drift apart).

### 3.3 Slot 2 — Bottom dock / global terminal entry (⌘J)

One deterministic priority, replacing both the old toggle and
restore-bottom; every branch keeps the R44 focus contract (Composer stays
focused after reveal; hide returns focus to the Composer):

1. The bottom dock's selected tab is actually rendered → hide the dock.
2. Otherwise the bottom dock has a hidden selected tab (non-terminal) →
   reveal it (Composer focus, pane content is not implicitly activated).
3. Otherwise the project's most recent terminal exists anywhere → R44 reveal,
   including the unavailable-right migration (same tab, same PTY, bottom
   dock, Composer focus).
4. Otherwise a terminal can be created → create the first one (R44).
5. Otherwise the slot renders disabled.

`⌘J` dispatches this same handler. The button shows `bg_active` iff branch 1's
rendered predicate holds — `hidden == false` alone never lights it up
(maximized counts as rendered, narrow-window unmounted right terminals do
not). R44's `workspace_terminal_is_rendered`, migration and PTY-preservation
clauses are reused verbatim; no terminal test assertion changes.

### 3.4 Slot 3 — Right dock

Enabled iff one of:

1. The right dock is rendered (R44 "rendered" predicate: not hidden, selected
   tab present, available width ≥ `MIN_RIGHT_WORKSPACE_AVAILABLE_WIDTH`, or
   maximized) **and its selected tab is not a terminal** → hide the dock
   (Composer focus). A rendered right-terminal is owned by slot 2, so slot 3
   is disabled then — one hide action, one entry.
2. The right dock has a hidden selected non-terminal tab → restore it
   (existing `restore_hidden_workspace(0)` semantics: pane content activation
   per R44 "generic restore", environment overlay closes).
3. Nothing is hidden but a Review diff is available for the current project
   thread → open the diff pane (the removed Review button's capability, now
   reachable from exactly one place).
4. Otherwise disabled (e.g. narrow width guard, standalone task, terminal
   selected).

At window widths where the responsive guard cannot render the right dock, the
slot renders disabled; the R44 unavailable-right terminal recovery stays
exclusively on slot 2.

### 3.5 Tooltips

`IconTooltip` gains an optional shortcut badge: label text plus a small
keycap chip (`bg_hover` surface, `METADATA` size, `text_secondary`) rendered
after the label —「切换终端 ⌘J」. Tooltips keep the native GPUI mechanism and
the shared component; no new color or font size is introduced.

### 3.6 Composer

- Centering basis frozen: `mx_auto` inside the conversation column
  (`flex_1` sibling of the Environment rail / right pane), max-width
  `COMPOSER_MAX_WIDTH` 736px, min-height 100px, radius 20px — unchanged.
- The wrapper's hardcoded `.pt(px(12.))` / `.pb(px(16.))` move into
  `Layout::COMPOSER_PADDING_TOP = 12.0` / `Layout::COMPOSER_PADDING_BOTTOM =
  16.0` and the design guidelines table records them; values unchanged.
- Multi-line growth stays clamped at 8 rows; the send button stays truthfully
  disabled while the input is empty; running/stop states keep their existing
  controller bindings. No action-row content, order, provider/model control,
  or attachment feature changes in this round.
- Panel transitions must never overlap or unfocus the Composer: opening or
  closing any dock re-centers it inside the surviving column and leaves
  focus per §3.2–3.4 (reveals keep Composer focus except the R44 explicit
  activation paths).

### 3.7 Responsive

At 1403×860 and 960×600 the cluster is three 28×28 slots with 6px gaps
(~96px + trailing inset). Slots disable per §3.2–3.4 instead of
disappearing; the 1229/1230px Environment boundary behavior is unchanged.

## 4. One state / one test / one image

Every row: production test mounted on the real render tree (R44 harness
pattern, `debug_bounds`/focus assertions), then a native screenshot from the
final installed app at the named window size, saved under
`/tmp/vega-r45-shell-composer/`.

| # | State | Automated proof (test name prefix `r45_`) | Native image |
|---|---|---|---|
| 1 | No panel open | `r45_header_cluster_renders_three_stable_slots` — 3 slots, order, 28×28, 6px gaps, at 1403×860 and 960×600 | `01-closed.png` |
| 2 | Environment overlay open / Esc | `r45_environment_overlay_esc_and_outside_click_close_with_composer_focus` | `02-env-overlay.png`, `03-env-overlay-esc.png` |
| 3 | Environment rail (wide) | `r45_environment_slot_tracks_rendered_state` | `04-env-rail.png` |
| 4 | Bottom dock via slot 2 and via ⌘J | `r45_bottom_toggle_unified_priority_and_cmd_j_parity` | `05-bottom-toggle.png`, `06-bottom-cmdj.png` |
| 5 | Bottom hidden non-terminal tab restore | covered by the unified-priority test (branch 2) | `07-bottom-hidden-tab-restored.png` |
| 6 | Right dock open via slot 3 (Review) | `r45_right_toggle_hide_restore_open_diff_priority` | `08-right-open.png` |
| 7 | Right dock hidden then restored | same test (branch 2) | `09-right-restored.png` |
| 8 | Rendered right terminal: slot 2 active, slot 3 disabled | `r45_terminal_owns_bottom_and_right_hide_paths` | `10-right-terminal.png` |
| 9 | Active-state truthfulness | `r45_toggle_surfaces_track_rendered_visibility` (rendered vs `hidden==false` unmounted) | `11-active-states.png` |
| 10 | Hover + tooltip | native-only (hover each slot, capture tooltip incl. ⌘J keycap) | `12-tooltip-terminal-cmdj.png` |
| 11 | Composer empty / single line | `r45_composer_centering_and_geometry` (bounds math) | `13-composer-empty.png` |
| 12 | Composer multi-line growth | same test (row clamp) | `14-composer-multiline.png` |
| 13 | Composer with Environment rail open | same test (center = column − rail/2) | `15-composer-with-rail.png` |
| 14 | Composer with right dock / bottom dock open | same test (no overlap, re-centered) | `16-composer-with-docks.png` |
| 15 | Focus probes after every reveal | R44 focus assertions extended: after slot-1/2/3 reveal, unclicked keystrokes land in the Composer | proven inside 02/04/05/06/08/09 images where applicable |

Existing `r44_*` assertions pass unmodified: the removed
`main-header-restore-bottom` only appears in them as `shell_absent` checks,
which removal keeps true, and the `main-header-terminal` / `cmd-j` /
composer-focus assertions carry over verbatim. Two r21 route-fencing
membership lists assert the *old* conditional header membership
(`main-header-review` existence on a project route; `main-header-terminal` /
`main-header-environment` absence on standalone routes). R45 supersedes
membership with availability: those lists are updated to assert the slot
model — the three slots are present at every route, standalone/project-less
routes render them disabled and clicking them performs no state change, and
the fenced surfaces themselves (`environment-rail`, `environment-*` cards)
keep their existing absence assertions. No other r21 assertion changes.

## 5. Gates

```text
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
cargo xtask package
```

Native acceptance is main-owned on the installed candidate at 1403×860 and
960×600, including the focus probes of §4 row 15. Packaged and installed
executable SHA-256 must match; the previous `/Applications/Vega.app` is
moved to a recoverable checkpoint before install.

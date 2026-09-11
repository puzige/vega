# Vega R46 window-anchored shell slots

**Status:** implementation contract

**Scope:** the geometry owner of the three trailing shell slots, the overlay
surface anchor, and the native proof that panel state never moves the header
controls.

**Evidence:** the user's 2026-09-12 review of R45 on the installed
application, plus a native measurement of the R45 build (screenshots
`/tmp/vega-r45-shell-composer/*.png`, 2806×1720 @2x ≙ 1403×860 logical) and a
live computer-use comparison with Codex Desktop. The Codex summary popover is
anchored under its own button and displaces nothing; screenshots and live
observations are product-behavior evidence only.

**Supersedes:** the R45 clause that placed the slot cluster inside the
conversation column's header (R45 §3.1 geometry owner). Every other R45
clause — three stable slots, one meaning per slot, rendered-visibility
active state, disabled slots keep their grid, unified bottom toggle, right
toggle priority, overlay dismissal, Composer geometry and the whole R44
contract — is preserved unchanged.

## 1. Problem

R45 renders the slot cluster inside `main-header`, which is the first child
of the `flex_1` conversation column in `render_workspace`'s row. The
Environment rail (320px) and the right workspace pane are *siblings* of that
column, so both shrink it and drag the cluster leftwards. Native measurement
of the R45 build at 1403×860:

| State | slot centers (logical px) | displacement |
|---|---|---|
| no panel | 1308.8 / 1342.8 / 1376.8 | — |
| Environment rail open | 988.8 / 1022.8 / 1056.8 | **−320** |
| right pane open | 859.8 / 893.8 / 927.8 | **−449** |

Two further defects follow from the same ownership error:

- the rail card's top edge sits at y=14 while the main header is 46px tall,
  so the card paints over the header row that contains the controls;
- the overlay card is positioned against the same inner container, so in
  narrow mode it too can cover the slot cluster instead of hanging below it.

A global layout control that moves when a panel opens is not a shell control;
it is a property of whichever column happens to render it. Codex anchors its
three buttons to the window's top-right corner and expands the summary
surface downwards from the pressed button, displacing nothing.

## 2. Contract

### 2.1 Window-anchored slot cluster

- The three slots render as a **window-level trailing cluster**, anchored to
  the window's top-right corner, outside every column, rail and pane.
- Its geometry is a function of the window box and the trailing inset only.
  Opening, closing, docking, maximizing, restoring or resizing any panel
  must not change any slot's x coordinate. The native acceptance asserts
  **identical centers** across: no panel, Environment rail, Environment
  overlay, right pane, bottom pane, right pane + bottom pane, and 960×600.
- The cluster keeps R45's frozen internals: three 28×28 controls
  (`Layout::TITLEBAR_CONTROL_SIZE`), 16px icons, 6px gaps, fixed order
  Environment → Terminal(⌘J) → Right workspace, disabled slots retaining
  their grid.
- Vertical placement: the cluster is vertically centered in the 46px main
  header band, matching the leading Sidebar/Search/Back/Forward group's
  band, and uses the same trailing inset as the header (`pr_3`).
- The `main-header` element keeps its existing responsibilities (project
  label, title, leading controls, 1px bottom border, 46px height) and its
  `main-header` selector. The slot cluster is no longer a child of it.

### 2.1.1 Surfaces that occupy the top band yield to the cluster

The right pane and a maximized pane render their own header row inside the
same 46px top band as the main header (R22/R44 pane geometry, unchanged), so
the cluster would otherwise sit exactly on top of that pane's trailing
actions. Codex resolves the same situation by making the three toggles the
**trailing group of the rightmost header row**, with the pane's own actions
to their left (measured on the right-panel-open reference: pane tab, `+`,
expand/maximize, then the three toggles, all in one row).

Vega keeps the cluster absolutely anchored (so its x is structural, not
emergent) and instead requires every header that can occupy the top band to
reserve the cluster's trailing band:

- `render_main_header` reserves `Layout::SHELL_SLOT_CLUSTER_RESERVE` (3×28 +
  2×6 + the 12px trailing inset) instead of its former `pr_3`.
- A Workspace pane header rendered in the top band — the docked right pane,
  and either dock's maximized pane — reserves the same trailing band so its
  own trailing actions sit left of the slots.
- The bottom-docked pane header (bottom band) reserves nothing: the cluster
  never occupies that band.
- The cluster keeps `.occlude()`: a press inside the reserved band belongs to
  the slots, and a disabled slot ignores activation rather than falling
  through to whatever is beneath it (R45 §3.1).

Consequence for existing assertions: an R21 test that clicked a right-pane
trailing action through the cluster's band now reaches its target because the
action moved left. Any assertion that pinned a pane action's absolute x, or
the rail card's top inset, to the pre-R46 overlap is updated to this contract
and listed in the delivery note.

### 2.2 Overlay and rail surfaces must not cover the controls

- The narrow Environment overlay card hangs **below** the header band: its
  top edge is at or below the header's bottom border, and its right edge
  keeps the existing R21 card inset. The card must not intersect the slot
  cluster's bounds.
- The wide rail keeps its R21 geometry (320px column, card inset 16px,
  radius 18px) but must start below the header band for the same reason:
  rail and overlay share one top offset token.
- The dismissal contract from R45 is unchanged: Escape and outside-click
  close the overlay and return focus to the Composer; the rail has no
  Escape path.
- Panels keep their own geometry: the bottom dock, right pane and their
  splitters are untouched.

### 2.3 No new tokens, no behavior change

- One new shared geometry token may be introduced for the header band's
  trailing cluster inset only if the existing `Layout::TITLEBAR_CONTROL_*`
  values cannot express it; no new color, font size, radius or shadow.
- The three slots keep exactly the R45 semantics, tooltips and disabled
  rules; `workspace_toggle_bottom`, `workspace_toggle_right`,
  `dismiss_environment_overlay` and the R44 terminal contract are unchanged.
- The Composer keeps R45's centering basis, max width, min height, growth
  clamp and padding tokens.

## 3. One state / one test / one image

| # | State | Automated proof (`r46_` prefix) | Native image |
|---|---|---|---|
| 1 | Slot centers identical with no panel vs rail vs overlay vs right pane vs bottom pane vs both | `r46_slot_cluster_is_window_anchored` — bounds equality (±1px) across all six states at 1403×860 | `01-anchored-closed.png`, `02-anchored-rail.png`, `03-anchored-right.png`, `04-anchored-both.png` |
| 2 | Slot centers identical at 960×600 | same test, narrow window | `05-anchored-narrow.png` |
| 3 | Overlay card starts below the header band and does not intersect the cluster | `r46_overlay_and_rail_start_below_header_band` | `06-overlay-below-header.png` |
| 4 | Rail card starts below the header band | same test | `02-anchored-rail.png` |
| 5 | Right pane / maximized pane trailing actions sit left of the reserved band (no overlap) | `r46_top_band_headers_reserve_the_cluster` | `07-right-pane-reserve.png`, `08-maximized-reserve.png` |
| 6 | Cluster keeps R45 internals (28×28, 34px centers, order, disabled keeps grid) | `r45_header_cluster_renders_three_stable_slots` (existing, must stay green unmodified) | reuses `01`/`05` |
| 7 | Semantics unchanged: toggles, tooltips, focus, dismissal | the full existing `r45_*` and `r44_*` suites, unmodified | R45 image set still valid |

Existing `r45_*`, `r44_*` and `r21_*` assertions must pass unmodified except
where they pinned geometry that this contract intentionally moves (the rail
card's top inset, and any absolute x of a right-pane trailing action that now
sits left of the reserved band). Each such change is listed in the delivery
note with its reason.

## 4. Gates

```text
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
cargo xtask package
```

Native acceptance is main-owned on the installed candidate at 1403×860 and
960×600: the six-state coordinate table above, plus a visual check that the
overlay and rail hang below the header. Packaged and installed executable
SHA-256 must match; the previous `/Applications/Vega.app` is moved to a
recoverable checkpoint before install.

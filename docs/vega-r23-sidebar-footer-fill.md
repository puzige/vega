# Vega R23 Sidebar footer fill — implementation contract

**Status:** implementation contract  
**Frozen:** 2026-09-10 (Asia/Shanghai)  
**Scope:** the persistent Settings entry at the bottom of the main Sidebar.

## 1. Problem

The Settings entry currently reuses the shared `small` button size. That makes
the control 24px high while every ordinary Sidebar row is 32px high. In the
resized 365px Sidebar this reads as a detached, undersized pill instead of the
footer row of the navigation rail.

## 2. Visual contract

- The Settings hit target is exactly the shared 32px Sidebar row height.
- It fills the Sidebar's inner content column: its horizontal bounds equal the
  New Task row bounds and its width is `sidebar width - 2 × 12px`.
- The Sidebar keeps its existing 12px outer padding. The footer must not add a
  second horizontal inset or a fixed width.
- The label remains centered. The shared ghost button owns hover, pressed,
  keyboard focus, Light/Dark colors, and radius; no local color literal or new
  one-off geometry token is introduced.
- The control stays pinned below the scroll region with a 12px bottom inset.
  A short project list may leave flexible space above this persistent footer.

## 3. Behaviour and ownership

- Clicking Settings still opens the existing Settings route and refreshes
  windows through the current handler.
- `Cmd+,`, accessibility label, tooltip, tab stop, Sidebar collapse, resize,
  and scroll ownership are unchanged.
- The fix does not change project/session rows or Settings page navigation.

## 4. Acceptance

1. A production-mounted GPUI visual test measures `sidebar-settings` at 32px
   high and horizontally aligned with `sidebar-new-task` within ±1px.
2. The same contract holds at Sidebar widths 240px, 304px, and 365px.
3. Existing Settings route tests continue to pass.
4. `cargo fmt --all -- --check`, strict workspace Clippy, and all workspace
   tests pass.
5. The packaged macOS app is opened and checked in Light and Dark appearance;
   the footer looks like one full Sidebar row and remains operable.

## 5. Non-goals

- No edge-to-edge footer outside the Sidebar's 12px content grid.
- No redesign of shared `Button::small()`.
- No new Settings features, icons, surface tokens, or animation.

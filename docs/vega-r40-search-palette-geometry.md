# R40 Search palette geometry

The global Search palette must use a distinct, roomy command-palette geometry
instead of inheriting the compact floating-menu width. On a sufficiently large
window its outer width is exactly 520px and its maximum height is 480px. These
values are shared layout tokens owned by `vega_theme`; changing the general
menu token is out of scope.

The palette remains horizontally centered and keeps its existing top offset.
Its width is capped to the viewport width minus 32px. Its maximum height is the
smaller of 480px and the existing viewport-height allowance (viewport height
minus 108px), with the existing 160px defensive floor retained for unusually
short windows.

The input, scope tabs, transient status/error rows, and keyboard-help footer
remain part of the fixed palette chrome. Only the results region scrolls when
the result set exceeds the available height. The footer must stay visible and
the palette must never grow into a nearly full-window result column.

Search behavior, scopes, ranking, actions, keyboard navigation, focus,
backdrop, radius, colors, typography, and result contents are unchanged.

## Acceptance

- A mounted production palette in a roomy viewport resolves to 520px wide and
  no more than 480px high.
- A narrow viewport preserves 16px clearance on both sides.
- A short viewport caps the palette to the available viewport allowance while
  retaining the defensive minimum used by the existing implementation.
- A long result set scrolls inside `palette-results`; the input, scopes, and
  footer remain visible.
- The Sidebar Search button and Command-K still open the same palette.
- Formatting, strict Clippy, and workspace tests pass; the packaged macOS app
  is opened and the Search palette is visually verified.

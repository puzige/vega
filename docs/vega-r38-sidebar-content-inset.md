# R38: match the sidebar content inset to the Codex reference

## Decision

The current Vega selected row leaves only about 4px between its surface and
the application edge. The reference leaves an 8px logical edge inset. Move the
whole `Pinned / Projects / Recents` organization content column inward by one
4px rhythm step so section labels, titles, project rows, and task rows remain a
single aligned grid.

The Pinned surface keeps its existing 8px title padding and complete rounded
edge from R37. The content column's trailing edge stays fixed; only its
available width contracts by 4px. Sidebar top navigation, Settings, outer
scroll behavior, row heights, and section spacing remain unchanged.

## Acceptance

1. Mounted organization section labels share one leading edge after the inset.
2. The Pinned selected/hover surface is 8px from the Sidebar application edge
   and its title remains 8px inside the surface.
3. Pinned, Projects, and Recents right edges remain unchanged; rows retain 32px
   height and all action hit areas remain stable.
4. Light and Dark themes show complete rounded edges without clipping.
5. The 12px Pinned-to-Projects gap, project child indentation, progressive
   lists, top navigation, and Settings row remain unchanged.

## Non-goals

- No color, typography, radius, data, persistence, shortcut, or menu behavior
  change.

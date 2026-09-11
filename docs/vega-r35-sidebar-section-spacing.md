# Vega R35 Sidebar Pinned-to-Projects spacing

## Decision

The `Pinned` task list and the following `Projects` heading need a clear
section break. The vertical distance from the bottom edge of the final Pinned
row to the top edge of the Projects header is 12px: the existing 8px section
gap plus one 4px rhythm step.

This adjustment applies only when the Pinned section is present. It must not
change row height, selection geometry, typography, horizontal alignment, or
the spacing between Projects and Recents.

## Implementation boundary

- Add one existing 4px spacing primitive between `Pinned` and `Projects`.
- Preserve the current 32px task rows and 28px section headers.
- Preserve the R34 top shortcut-column alignment and all Pinned/Projects row
  content and interaction behavior.
- Do not add a new layout token for this one 4px rhythm step.

## Acceptance

1. With at least one pinned task, the mounted distance from the final Pinned
   row bottom to the Projects header top is exactly 12px.
2. The final Pinned row remains 32px high and its selected background bounds
   do not grow.
3. The Projects header remains 28px high and retains its current leading edge.
4. When Pinned is absent, Projects does not retain a blank Pinned-only spacer.
5. Light and Dark themes render the same geometry without clipping.

## Non-goals

- No typography, color, row content, pagination, hover/menu, project expansion,
  data, persistence, or shortcut change.

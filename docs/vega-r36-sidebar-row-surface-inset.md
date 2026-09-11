# Vega R36 Sidebar Pinned row surface inset

## Decision

A Pinned task title must retain its established leading content column while
its interactive row surface provides 8px of visible leading padding. The
selected/hover surface therefore begins 8px before the title instead of
sharing the title's left edge.

The title itself stays aligned with the `Pinned` section label and top-level
`Recents` task titles. The row's trailing edge, 32px height, 8px radius, and
action hit area remain unchanged.

```text
Pinned/title column
│
│  Pinned
│
├──────── selected / hover surface ───────────────┐
│  8px  task title                         menu   │
└─────────────────────────────────────────────────┘
          ^ title remains on the section column
```

## Implementation boundary

- Give Pinned task surfaces an 8px leading outset and an equal 8px internal
  title inset, preserving the current title x-coordinate.
- Apply the same geometry to rest, hover, selected, focus, and menu-open row
  bounds so interaction does not jump.
- Preserve the R35 12px Pinned-to-Projects section break.
- Use existing Sidebar spacing and radius primitives; add no new token.

## Acceptance

1. Mounted Pinned row title left edge remains equal to the Pinned section label
   left edge.
2. Mounted Pinned row surface left edge is exactly 8px before its title.
3. Its right edge and 32px height are unchanged from the existing content
   column, and the selected background uses the full row bounds.
4. Light and Dark themes show the same geometry for selected and unselected
   Pinned rows without clipping.
5. Recents and project/project-task rows are unchanged.

## Non-goals

- No typography, color, section spacing, pagination, menu visibility, data,
  persistence, shortcut, or non-Pinned row change.

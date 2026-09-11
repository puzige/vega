# Vega R31 Sidebar typography rollback and Pinned leading edge

## Decision

R31 supersedes the R30 Sidebar typography increase. Restore the established
compact values:

- Sidebar primary navigation and task/project titles: 13px.
- Sidebar section labels, project ownership, relative time, and shortcuts:
  12px.
- Sidebar navigation row height remains 32px.

Pinned task rows no longer reserve the shared 32px icon/title inset. Because
production Pinned rows have no icon, brand mark, or disclosure control, their
title begins at the section's leading content edge:

```text
Pinned
R12 Alpha                 project       time
```

The Pinned title's left edge must equal the `Pinned` section label's left edge.
Project folder labels, project-child task titles, and Recents task titles keep
their existing 32px content origin.

## Implementation boundary

- Fully remove or cease consuming the R30-only `SIDEBAR_PRIMARY`,
  `SIDEBAR_SECTION`, and `SIDEBAR_META` tokens; restore the prior typed
  `SIDEBAR = 13px` and `METADATA = 12px` call sites.
- Give production Pinned rows an explicit zero leading content inset without
  changing retained/legacy pin indicators or other task rows.
- Preserve fixed project metadata width, action/time tail, truncation, hover
  behavior, accessibility, sorting, pin persistence, progressive lists, and
  Light/Dark colors.

## Acceptance

1. Mounted Sidebar primary/section/meta text returns to the pre-R30 sizes.
2. Each production Pinned title aligns exactly with the Pinned section label.
3. Project labels, project-child tasks, and Recents tasks retain the R27 32px
   content origin.
4. Pinned metadata and time columns remain stable across short/long titles and
   rest/hover states.
5. R26–R30 mounted regressions are updated only where R31 intentionally
   supersedes their typography/alignment assertions.
6. Native Light/Dark inspection shows compact readable text without clipping.

## Non-goals

- No row-height, section-spacing, icon, project/Recents limit, Show More,
  schema, persistence, provider, runtime, or data-order change.

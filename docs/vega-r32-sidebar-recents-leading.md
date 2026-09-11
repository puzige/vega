# Vega R32 Sidebar Recents leading edge

## Decision

R32 extends the R31 zero-leading-inset rule from Pinned tasks to top-level
Recents tasks. Neither projection renders a leading icon or disclosure control,
so their titles begin at the same leading content edge as their section labels:

```text
Pinned
R12 Alpha                 project       time

Recents
Untitled task                           time
```

The title left edge of every production Pinned and top-level Recents row must
equal its section label's left edge. Project rows keep the Folder icon and its
8px label gap. Tasks nested under an expanded project keep the established 32px
content origin so hierarchy remains visible.

## Implementation boundary

- Give production top-level Recents rows an explicit zero leading content
  inset, matching production Pinned rows.
- Keep project-child task rows on `Layout::SIDEBAR_NAV_CONTENT_INSET`.
- Preserve compact R31 typography: 13px primary text, 12px section labels and
  metadata, and 32px row height.
- Preserve fixed metadata width, action/time tail, truncation, hover behavior,
  accessibility, sorting, pin persistence, progressive lists, and Light/Dark
  colors.

## Acceptance

1. Every production Recents title aligns exactly with the `Recents` section
   label.
2. Every production Pinned title remains aligned with the `Pinned` section
   label.
3. Project labels and project-child task titles retain their established icon
   and hierarchy geometry.
4. Relative time remains trailing-aligned and does not shift between rest and
   hover states.
5. Mounted regression coverage distinguishes top-level Pinned/Recents rows from
   project-child rows.
6. Native Light/Dark inspection shows the same leading-edge rhythm as the user
   reference without typography changes or clipping.

## Non-goals

- No font-size, row-height, section-spacing, icon, list-limit, Show More,
  schema, persistence, provider, runtime, or data-order change.

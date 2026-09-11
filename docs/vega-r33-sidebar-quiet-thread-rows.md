# Vega R33 Sidebar quiet thread rows and per-project progressive lists

## Decision

Sidebar task rows prioritize titles and contextual actions. Relative timestamps
are removed from every production task projection, including Pinned, expanded
project children, and top-level Recents. Pinned rows also stop rendering project
ownership metadata: their section already communicates the important state, and
the title must remain the only resting content.

The trailing action trigger remains mounted for stable keyboard access and is
shown only by the existing hover, focus, or menu-open rules. Removing resting
metadata must not shift the title or the trailing action hitbox between states.

Each expanded project uses its own in-memory progressive list:

- Show the first 5 non-pinned tasks by the current authoritative sort.
- When more than 5 tasks exist, render `Show More` after the visible rows.
- `Show More` expands only that project and becomes `Show Less`.
- `Show Less` returns only that project to 5 visible rows.
- Collapsing and reopening a project preserves its progressive state for the
  lifetime of the mounted Sidebar.
- The control aligns with the established 32px project-child content origin.

## Implementation boundary

- Remove production task-row timestamp rendering at rest without changing
  action-menu visibility, width, focusability, or behavior.
- Remove project ownership metadata from production Pinned rows and allow the
  title to use the released width.
- Add project-id-scoped progressive expansion state. Reconcile stale ids when
  projects disappear; do not persist the state to disk.
- Reuse the existing progressive-control visual language and typed layout
  tokens. Do not add colors, font sizes, row heights, or dependencies.
- Preserve R32 leading edges: Pinned and top-level Recents use zero inset;
  project children retain `Layout::SIDEBAR_NAV_CONTENT_INSET`.

## Acceptance

1. Pinned, project-child, and Recents rows have no resting relative-time node.
2. Pinned rows have no project metadata node and keep their title aligned to the
   Pinned heading in rest and hover/focus states.
3. The existing trailing task action remains invisible at rest, visible on
   hover/focus/menu-open, keyboard reachable, and positionally stable.
4. An expanded project with 6 or more eligible tasks initially mounts exactly 5
   rows plus `Show More`; expanding mounts all rows plus `Show Less`; collapsing
   the progressive list returns to 5.
5. Progressive expansion is independent between projects and survives ordinary
   project collapse/reopen within the mounted Sidebar.
6. Pinned tasks remain excluded from their project list; archive filtering and
   current sort are applied before the 5-row limit.
7. Light and Dark native inspection confirms the quieter rows, compact R31
   typography, R32 leading edges, and per-project controls without clipping.

## Non-goals

- No task ordering, pin/archive persistence, top-level Projects/Recents limits,
  row height, font size, icon, schema, provider, runtime, or data migration
  change.

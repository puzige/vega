# Vega R27 Sidebar row alignment correction — implementation contract

**Status:** implementation contract
**Frozen:** 2026-09-11 (Asia/Shanghai)
**Scope:** production Pinned task rows, project headers, and task content-column
alignment.

## 1. User correction

R26 correctly introduced the `PINNED / PROJECTS / RECENTS` hierarchy, but its
Pinned rows still render a blue Pin glyph. The section heading already conveys
that state, so the repeated icon adds noise and shifts pinned titles away from
the shared task grid. Project folder icons also sit too close to their labels.

This contract supersedes R26 only where R26 requires a visible pin glyph inside
Pinned rows. R26 remains authoritative for grouping, exactly-once projection,
hover-reveal actions, metadata truth, and menu behavior. R25 remains
authoritative for neutral selection and folder open/closed disclosure.

## 2. Pinned row grid

- Production rows inside `PINNED` render no Pin icon, brand mark, placeholder,
  or empty icon slot. The `PINNED` heading is the sole persistent visual marker
  of membership.
- Pinned title text starts on the same shared content column as project child
  task titles and Recents task titles.
- A project-bound pinned task keeps its real project name in a fixed 85px
  metadata column before the existing fixed action/time tail. Short project
  names do not move that column; long names truncate within it.
- The existing relative-time/action tail retains its fixed width and right
  alignment. Hovering still replaces time with the task menu without moving the
  title or project metadata columns.
- The Pin command and persisted `thread.pinned` state remain unchanged. Only
  the redundant row glyph is removed from the production Pinned projection.

## 3. Folder-to-label spacing

- Project headers keep the existing 8px row inset and 16px open/closed folder
  icon.
- The gap between folder icon and project label is 8px, producing a shared
  32px content inset from the project-row left edge (`8 + 16 + 8`).
- Expanded project child titles and ordinary task rows use that same 32px
  content inset. No label or hit target may shift when contextual actions
  appear.
- This shared inset is a typed `Layout` token rather than another component
  literal. Current and retained projection paths consume the same token.

## 4. Acceptance

1. A production-mounted Pinned fixture proves that no Pin indicator or empty
   slot is rendered and that pinned titles share the task content column.
2. Two project-bound pinned rows with differently sized project names prove the
   project metadata column has a stable 85px width and the time/action tail does
   not move.
3. Mounted project geometry proves an exact 8px folder-to-label gap and exact
   alignment between project label, project child title, Pinned title, and
   Recents title.
4. Hovering project/task rows preserves the same title, metadata, and tail
   origins while revealing R26 contextual actions.
5. Pin/unpin persistence, section projection, folder disclosure, navigation,
   Light/Dark rendering, resizing, and Settings remain unchanged.
6. Formatting, strict Clippy, workspace tests, package verification, and native
   macOS review pass before local integration.

## 5. Non-goals

- No removal of the Pin command, pin persistence, or `PINNED` section.
- No schema, migration, provider/runtime, credential, project-order, or task
  ordering change.
- No custom icon or new color.
- No change to section order or hover-reveal policy.

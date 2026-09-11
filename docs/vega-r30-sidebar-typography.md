# Vega R30 Sidebar typography

## Problem

The native Sidebar is visually one typographic step smaller than the Codex
reference: primary navigation rows use 13px, while section labels and row
metadata use 12px. At the current 304px width this makes the navigation feel
faint and undersized rather than compact.

## Decision

Introduce Sidebar-specific typography tokens and apply them to navigation
surfaces only:

- `SIDEBAR_PRIMARY = 15px`: New Task, Search, Settings, project labels, task
  titles, empty row labels, and `Show More / Show Less`.
- `SIDEBAR_SECTION = 13px`: `Pinned`, `Projects`, and `Recents` headings.
- `SIDEBAR_META = 13px`: project ownership, relative time, and Sidebar
  keyboard shortcuts.
- Keep `SIDEBAR_LINE_HEIGHT = 32px` and all existing width, inset, gap,
  truncation, color, weight, and hover/selected behavior.

The general `BODY`, `SIDEBAR`, and `METADATA` tokens remain unchanged because
they are consumed by conversation, settings, command palette, workspace, and
other non-navigation surfaces.

## Scope

- Production Sidebar navigation and retained Sidebar project/task projections.
- Shared Sidebar row helpers used by those projections.
- Typed tokens and geometry/typography tests.

Popup menus, conversation content, composer, command palette, Settings
content, workspace, terminal, and main-window chrome are out of scope.

## Acceptance

1. Mounted primary Sidebar row text resolves to 15px.
2. Mounted section labels resolve to 13px.
3. Mounted time/project metadata and shortcut text resolve to 13px.
4. Row heights remain 32px and the R27 title-column geometry is unchanged.
5. Projects 5 / Recents 10 progressive behavior and all hover actions remain
   unchanged.
6. Light and Dark appearances remain readable without clipping CJK or Latin
   labels.

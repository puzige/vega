# Vega R25 Sidebar navigation correction — implementation contract

**Status:** implementation contract
**Frozen:** 2026-09-10 (Asia/Shanghai)
**Scope:** Sidebar project/session navigation, shared neutral interaction tokens,
and the persistent Settings entry.

## 1. User correction

The native R24 review showed that an edge-to-edge Settings footer reads as a
detached status bar rather than part of the Sidebar navigation. The user also
rejected the R18 blue project selection and the redundant disclosure Chevron
beside every project folder.

This contract supersedes R18 for ordinary navigation selection color and
project disclosure rendering. It supersedes R24 for Settings outer geometry.
The existing 32px row height, Sidebar resizing, project collapse persistence,
navigation behavior, keyboard access, tooltips, and Settings route remain
authoritative.

## 2. Visual language

- Ordinary interactive surfaces use neutral gray: Light hover `#F3F3F3` and
  selected `#EDEDED`; Dark hover `#282828` and selected `#303030`. These values
  live only in `vega_theme`; components consume `bg_hover` and `bg_active`.
- Selected project and task labels use `text_primary`; their functional icons
  stay neutral. Brand blue is not a navigation-selection indicator.
- Vega blue remains available for primary actions, focus indication,
  Agent/AI identity, unread/pinned semantics, and other intentional brand
  emphasis. Success, warning, and danger colors are unchanged.
- Project headers, session/task rows, New Task, Search, and Settings are 32px
  navigation rows with the shared 8px (`radius-lg`) corner treatment. Compact
  icon buttons, chips, menus, and text fields keep their existing radii.

## 3. Project disclosure

- A project header has no independent left Chevron.
- A collapsed project renders the familiar closed `Folder` outline; an
  expanded project renders the matching `FolderOpen` outline.
- The folder icon is the only visual disclosure state. The full project row
  keeps the existing pointer and keyboard toggle behavior and announces its
  expanded/collapsed state accessibly.
- Removing the Chevron must not leave an empty slot. The project folder and
  label move onto the normal content grid, and child task titles align with
  the project label rather than with the removed glyph.
- Project add and more controls retain their current 24px hit targets,
  visibility rules, labels, and event isolation.

## 4. Settings correction

- Settings remains persistent below the scroll region but is not an
  edge-to-edge strip.
- Its 32px painted and interactive row is inset 12px from the Sidebar left,
  right, and bottom edges and uses the same 8px radius as other navigation
  rows.
- The row uses a neutral Settings icon and label aligned like the other
  navigation actions; the `⌘,` hint may remain at the trailing edge.
- Hover/active/focus painting belongs to that inset row. No background may
  extend to the window or Sidebar edge.

## 5. Acceptance

1. Production-mounted Sidebar tests cover 240px, 304px, and 365px widths.
2. Settings painted and interactive bounds are 12px from the Sidebar left,
   right, and bottom, are 32px high, and still open General Settings.
3. Mounted project rows expose no Chevron slot; collapsed and expanded rows
   render closed/open folder states without changing row height or action
   targets.
4. Selected project and task rows use neutral `bg_active` with primary text;
   native Light/Dark review shows no blue selection wash or blue selected
   label/icon.
5. Navigation rows use the shared 8px radius and retain truncation, hover,
   focus, pointer, keyboard, resize, collapse, scroll, add, more, and route
   behavior.
6. Formatting, strict Clippy, all workspace tests, package verification, and
   native macOS Light/Dark review pass before integration.

## 6. Non-goals

- No redesign of the project/session information architecture.
- No change to project persistence, sort, archive, drag/drop, branch metadata,
  runtime/provider, credentials, database, or migrations.
- No global removal of Vega blue from primary actions, focus, Agent identity,
  unread/pinned semantics, or the App Logo.
- No new custom folder artwork; use the shared GPUI Kit/Lucide icon family.

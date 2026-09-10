# Vega R26 Sidebar sections and hover-reveal controls — implementation contract

**Status:** implementation contract
**Frozen:** 2026-09-10 (Asia/Shanghai)
**Scope:** the production Sidebar organization projection and its contextual
controls.

## 1. User correction

The current `SESSIONS / PROJECTS` split mixes pinned and recent tasks while
showing header and project actions continuously. The Sidebar must use the quiet
information hierarchy visible in the user's reference: `PINNED`, `PROJECTS`,
then `RECENTS`, with contextual controls revealed only when the user is
interacting with their owning row or section.

This contract extends R25. R25 remains authoritative for neutral selection,
8px navigation radii, folder open/closed disclosure, and the inset Settings
row.

## 2. Section projection

The production Sidebar renders these sections in this exact order:

1. `PINNED`: every pinned task eligible for the current active/archive filter,
   regardless of whether it is standalone or belongs to a project. Omit the
   complete section when it has no rows.
2. `PROJECTS`: registered project folders in the existing durable project
   order. Expanded folders render only their unpinned eligible tasks.
3. `RECENTS`: unpinned standalone eligible tasks, sorted by the existing
   Updated/Created preference. Keep the header and its empty guidance when no
   such task exists.

A task appears exactly once. Moving a task between pinned and unpinned states
must move it between sections after the existing persisted mutation completes;
there is no new database field, schema, grouping service, or duplicate cache.
When archived tasks are explicitly enabled by the existing filter, they use the
same projection rules and keep their archived actions.

Pinned rows retain the visible pin glyph because it communicates section
membership. A pinned project task may show its project name as compact metadata
so the global section does not erase ownership context.

## 3. Quiet contextual controls

- Persistent primary navigation remains visible: New Task, Search, folder
  disclosure, task/project labels, timestamps, pin status, and Settings.
- Section-header actions (sort/archive, new standalone task, add project) are
  visually hidden at rest and revealed when the pointer is anywhere over that
  header, when a contained action has keyboard focus, or while its menu is
  open.
- Project-row `+` and `…` actions are visually hidden at rest, including while
  the project is merely selected. They reveal while the pointer is over the
  project row, when reached by keyboard focus, or while the project menu is
  open.
- Task-row `…` remains mounted and keyboard reachable but is hidden at rest;
  the existing timestamp occupies the quiet state and gives way to the trigger
  on row hover, focus, or while the task menu is open.
- Opening a popup holds its trigger visible until the popup closes. Pointer
  travel from trigger into popup must not make the popup disappear.
- Hidden controls retain their existing 24/28px hit targets and labels. Reveals
  change only opacity/background, never row width, text origin, or height.
- Keyboard focus must make an otherwise hidden control visible. Opacity-zero
  controls may not be removed from the accessibility tree or tab order.

## 4. Labels and visual language

- Section labels render as `PINNED`, `PROJECTS`, and `RECENTS` using the existing
  metadata typography and tertiary text token.
- Section headers remain 28px high. Navigation rows remain 32px high with R25's
  8px radius and neutral Light/Dark interaction tokens.
- Contextual controls use neutral icons at rest/reveal. Destructive menu
  commands keep the danger semantic; Vega blue is not used to decorate these
  controls.

## 5. Acceptance

1. A production-mounted fixture containing pinned standalone, pinned project,
   unpinned standalone, and unpinned project tasks proves the exact section
   order and exactly-once membership.
2. The `PINNED` section is absent when empty; pin/unpin mutations still persist
   through the existing conversation service and reproject correctly.
3. Debug bounds remain mounted for header, project, and task action controls at
   rest; rendered opacity is quiet at rest and visible on owning-header/row
   hover, keyboard focus, and open-menu states.
4. Project selection alone does not expose `+` or `…`. Folder disclosure and
   row selection retain the R25 behavior.
5. The existing archive filter, sorting, project add/new task actions, popup
   menus, timestamps, resizing, scrolling, Light/Dark themes, and Settings route
   continue to work.
6. Formatting, strict Clippy, workspace tests, package verification, and native
   macOS hover review pass before local integration.

## 6. Non-goals

- No new user-created group model or reactivation of the retired Groups view.
- No schema, migration, provider/runtime, credential, project-order, or task
  persistence change.
- No hover-only hiding of primary navigation or semantic state indicators.
- No animation or layout shift during reveal.

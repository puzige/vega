# Vega R28 Sidebar heading case

## Decision

The production Sidebar section labels render as `Pinned`, `Projects`, and
`Recents`. They use ordinary title case rather than all caps.

This is a typography-only refinement. The existing section order, subdued
label color, size, weight, spacing, visibility rules, task projection, and
hover/focus/menu-open behavior remain unchanged.

## Scope

- Change every mounted Sidebar section-label path from uppercase strings to
  `Pinned`, `Projects`, and `Recents`.
- Keep production and retained/legacy project surfaces consistent where they
  expose the same visible section label.
- Update mounted UI assertions so a regression back to all caps fails.

## Acceptance

1. The populated Sidebar displays `Pinned`, `Projects`, and `Recents` in that
   order.
2. `PINNED`, `PROJECTS`, and `RECENTS` are absent from mounted section labels.
3. Empty-Pinned behavior and the existing R26/R27 geometry remain unchanged.
4. Light and Dark appearances remain readable.

## Non-goals

- No typography token, section order, row geometry, icon, action visibility,
  projection, persistence, schema, runtime, or provider change.

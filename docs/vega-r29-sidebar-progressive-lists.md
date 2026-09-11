# Vega R29 Sidebar progressive lists

## Problem

The production `Projects` section currently flex-fills the available Sidebar
height. With several projects and no expanded children, this leaves a large
blank region before `Recents` and makes the groups feel unrelated.

## Decision

- Sidebar sections stack by natural content height inside the existing outer
  Sidebar scroller. `Projects` must not flex-fill the gap before `Recents`.
- `Projects` initially renders 5 top-level projects. If more exist, a `Show
  More` row appears; expanding reveals all projects and changes the row to
  `Show Less`.
- `Recents` initially renders 10 standalone tasks. If more exist, it uses the
  same `Show More / Show Less` interaction.
- `Pinned` remains complete and keeps its existing five-row viewport because
  it is the highest-priority, typically small group.
- Project limits count top-level folders, not tasks inside an expanded folder.
  Expanded project children remain attached to their owner.
- The view state is in-memory and resets to the compact form when a new Sidebar
  block is created. It does not add persistence or schema.

## Interaction

- `Show More` and `Show Less` are quiet text rows aligned to the shared 32px
  navigation content column.
- They are pointer-clickable, keyboard focusable, and activate with Enter or
  Space.
- The control is absent when the section contains no hidden items.
- Toggling one section does not change the other section's expanded state.

## Acceptance

1. With 9 projects and at least one recent task, Projects shows 5 folders plus
   `Show More`, and Recents begins immediately after that compact content.
2. Clicking Projects `Show More` reveals all 9 folders and changes only that
   control to `Show Less`; clicking again restores 5.
3. With more than 10 Recents, the same behavior reveals and collapses that
   section independently.
4. Existing ordering, exactly-once projection, folder expansion, Pinned rules,
   hover actions, title alignment, Light/Dark colors, and Settings footer stay
   unchanged.

## Non-goals

- No database pagination, data-fetch limit, schema, persistence, provider,
  runtime, project ordering, or task ordering change.

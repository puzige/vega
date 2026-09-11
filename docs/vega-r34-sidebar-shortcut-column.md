# Vega R34 Sidebar shortcut column alignment

## Decision

The top-level `新建任务` and `搜索` navigation rows share one horizontal
geometry. Their surfaces use the same available Sidebar width and the same 8px
internal horizontal padding, so the trailing `⌘N` and `⌘K` labels resolve to
one exact right-edge column.

The Search row must not add a second horizontal outer inset. Its leading label
begins at the same x-coordinate as the New Task row's Plus icon, while its
shortcut aligns with the New Task shortcut:

```text
+  新建任务                                      ⌘N
   搜索                                          ⌘K
```

## Implementation boundary

- Remove the Search-only horizontal outer inset or otherwise make both row
  bounds and right padding identical using existing layout primitives.
- Preserve Sidebar outer 12px padding, row height, 8px radius, typography,
  colors, hover/pressed behavior, click handlers, labels, and shortcuts.
- Do not add a new token unless the existing shared padding cannot express the
  geometry.

## Acceptance

1. Mounted New Task and Search row bounds have the same left edge, right edge,
   and width.
2. `sidebar-new-task-shortcut` and `sidebar-search-shortcut` have exactly equal
   right edges in Light and Dark themes.
3. The Search label left edge equals the New Task Plus icon left edge.
4. Both rows retain 32px height, 8px inner horizontal padding, existing hover
   behavior, and working activation.
5. Native macOS inspection confirms the shortcut glyphs form one vertical
   column without clipping at supported Sidebar widths.

## Non-goals

- No Sidebar typography, section spacing, project/task row, progressive list,
  shortcut binding, data, persistence, or runtime change.

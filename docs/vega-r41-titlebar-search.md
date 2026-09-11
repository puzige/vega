# R41 Titlebar Search adjacency

Move the global Search button into the shared window navigation-control group
so it sits immediately beside the Sidebar visibility button. The visible order
is `Sidebar, Search, Back, Forward`; Search must not remain separated from the
Sidebar button by history controls.

The shared group is rendered in both shell states. Search therefore remains
available in the same titlebar position when the Sidebar is visible or hidden.
Remove the Sidebar-owned Search child so exactly one Search button is mounted.

Reuse the existing shared outline Search icon, neutral hover treatment,
`搜索 (⌘K)` accessible name and tooltip, and the existing `OpenPalette`
production action. Clicking or keyboard-activating the button opens the same
global Search palette. Command-K remains unchanged.

Do not move New Task, change history behavior, add labels, add tokens, or alter
the Search palette geometry and contents established by R40.

## Acceptance

- A production-mounted shell with the Sidebar visible contains exactly one
  Search button directly beside the Sidebar visibility button and before Back.
- The same adjacency and single-button invariant hold with the Sidebar hidden.
- Pointer and keyboard activation open the existing Search palette; Command-K
  still opens it.
- Existing Back, Forward and Sidebar actions remain functional.
- Formatting, strict Clippy, workspace tests, packaging and a native macOS
  walkthrough pass.

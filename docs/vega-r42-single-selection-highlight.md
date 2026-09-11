# R42 Single selection highlight

Sidebar selection must identify the most specific active destination. When an
opened task belongs to the selected project, only that task row receives the
neutral active background. The containing project row must remain at rest; its
open Folder icon already communicates expansion.

The project row may retain its active background only when that project is the
selected destination and no task from that same project is currently open.
This preserves project-only selection while preventing the double highlight
shown by a selected child task and its ancestor.

Pinned project tasks follow the same precedence even though their active row is
projected into Pinned rather than under the project. Hover, keyboard focus,
drag-over, project expansion, menu visibility, project authority and route
state are unchanged. Hover and focus may still show their temporary neutral
surface without becoming a second persistent selection.

Use the existing semantic `bg_active` / `bg_hover` colors. Do not add tokens,
change row geometry, move tasks, clear the selected project, or alter Folder
open/closed behavior merely to obtain the visual result.

## Acceptance

- With an opened child task in the selected project, its task row is active and
  the containing project row has no persistent active background.
- The same rule holds when the opened project task is shown in Pinned.
- With the project selected and no opened task, the project row keeps its
  existing neutral active background.
- Hover, focus, expansion, collapse and project actions still work.
- Light and Dark production-mounted tests, formatting, strict Clippy,
  workspace tests, packaging and native macOS walkthrough pass.

# Issue #347 · Project task list lazy loading

## Status

User-authorized implementation scope: lazy-load task rows inside expanded projects only. The top-level Projects list and its existing Show More / Show Less control are unchanged. Recents behavior is unchanged.

The Issue body is empty. The reported behavior and scope were clarified by the user: clicking Show More for a project's task list currently renders every remaining task at once; that task list must instead continue loading as the user scrolls.

## Behavior contract

1. Each expanded project's task list initially renders up to five eligible task rows, preserving the existing compact first page.
2. The project task list has no Show More / Show Less control. Project-level controls for the top-level Projects list remain unchanged.
3. Reaching the bottom of the outer Sidebar scroll container appends up to five rows to each expanded project task list that has hidden eligible rows. Repeated bottom reaches continue until those lists are complete.
4. If the complete Sidebar content fits its viewport, the existing viewport-fill mechanism continues loading pages until all eligible project task rows are rendered or the content overflows.
5. Growth is in-memory and monotonic for each project during the lifetime of the ThreadsBlock. Collapsing and reopening a project preserves already loaded rows; a new ThreadsBlock starts at five rows per project.
6. Existing task eligibility, pin/archive behavior, project ownership, sorting, row interactions, project order, and the independent Recents lazy loading remain unchanged. Loaded rows are prefixes of each project's existing sorted task projection.
7. When there are no hidden tasks, scrolling performs no growth. The collapsed project's rows are not grown while collapsed; reopening it resumes from its previous render window.

## Acceptance matrix

| ID | Scenario | Expected result | Layer |
|---|---|---|---|
| 347-1 | Expanded project has more than five eligible tasks in a short Sidebar viewport | Initially five rows are mounted; no project-task Show More / Show Less selector exists | GPUI UI test |
| 347-2 | Scroll the Sidebar to its bottom with more project tasks available | Exactly one five-row page is appended per expanded project per bottom reach; repeated reaches load remaining tasks | GPUI UI test |
| 347-3 | Expanded project list fits in a tall Sidebar viewport | All eligible rows become mounted through viewport fill | GPUI UI test |
| 347-4 | Two projects have hidden task rows; collapse one while growing | Only expanded projects grow; their independent loaded prefixes are preserved | GPUI UI test |
| 347-5 | No hidden project tasks remain | Further bottom reaches do not add rows | GPUI UI test |
| 347-6 | Regression: project folders and Recents | Top-level Projects Show More remains unchanged; Recents keeps its existing lazy loading and selectors | Existing GPUI UI tests |
| 347-7 | Ordering and eligibility | Rows remain in the existing sorted order; pinned rows remain in Pinned; archived rows follow the existing archive filter | GPUI UI test |

## Implementation plan

- Replace the per-project expanded set with independent in-memory visible and rendered-total counts in `ThreadsBlock::Organization`.
- Record each open project's current eligible task total while projecting its sorted rows.
- Extend the existing Sidebar bottom/viewport-fill callback to grow project task windows by one page alongside the already-existing Recents window.
- Remove only the project-task Show More / Show Less control and its toggle behavior.
- Add UI regression coverage for short/tall viewports, independent project state, sorting, and exhaustion.

No database pagination, persistence, schema changes, new dependencies, or changes to the top-level project list are in scope.

# Issue #347 · Project task list lazy loading

## Status

User-authorized implementation scope: the task rows inside an expanded project page in lazily instead of rendering every remaining row on one activation. The top-level Projects list and its existing Show More / Show Less control are unchanged. Recents behavior is unchanged.

The Issue body is empty. The reported behavior and scope were clarified by the user: activating 「展开更多」 for a project's task list currently renders every remaining task at once; that task list must instead append one page per activation and keep loading as the user scrolls.

The user selected this shape explicitly (2026-10-15), after the first implementation removed the per-project control entirely: keep the 「展开更多」 control, make one activation append one page, and let it disappear once the list is complete. No 「收起」 state is added.

## Behavior contract

1. Each expanded project's task list initially renders up to five eligible task rows, preserving the existing compact first page.
2. An expanded project whose eligible rows exceed its current render window shows a 「展开更多」 control below its rows, sharing the child text column. The control carries no 「收起」 counterpart and never removes already rendered rows.
3. Activating that control appends up to five rows to that project's task list only. Repeated activations continue until the list is complete, at which point the control disappears.
4. Reaching the bottom of the outer Sidebar scroll container appends up to five rows to each expanded project task list that has hidden eligible rows. Repeated bottom reaches continue until those lists are complete. The control and the scroll route share the same per-project window.
5. If the complete Sidebar content fits its viewport, the existing viewport-fill mechanism continues loading pages until all eligible project task rows are rendered or the content overflows.
6. Growth is in-memory and monotonic for each project during the lifetime of the ThreadsBlock. Collapsing and reopening a project preserves already loaded rows; a new ThreadsBlock starts at five rows per project.
7. Existing task eligibility, pin/archive behavior, project ownership, sorting, row interactions, project order, the top-level Projects Show More / Show Less control, and the independent Recents lazy loading remain unchanged. Loaded rows are prefixes of each project's existing sorted task projection.
8. The collapsed project's rows are not grown while collapsed; reopening it resumes from its previous render window.

## Acceptance matrix

| ID | Scenario | Expected result | Layer |
|---|---|---|---|
| 347-1 | Expanded project has more than five eligible tasks in a short Sidebar viewport | Initially five rows are mounted; the project-level 「展开更多」 selector exists; no project-level 「收起」 selector exists | GPUI UI test |
| 347-2 | Activate the project-level 「展开更多」 repeatedly | Exactly one five-row page is appended per activation to that project only; the control disappears once the list is complete | GPUI UI test |
| 347-3 | Scroll the Sidebar to its bottom with more project tasks available | Exactly one five-row page is appended per expanded project per bottom reach; repeated reaches load remaining tasks | GPUI UI test |
| 347-4 | Expanded project list fits in a tall Sidebar viewport | All eligible rows become mounted through viewport fill; no project-level control remains | GPUI UI test |
| 347-5 | Two projects have hidden task rows; collapse one while growing | Only expanded projects grow; their independent loaded prefixes are preserved | GPUI UI test |
| 347-6 | No hidden project tasks remain | Further bottom reaches do not add rows and the project-level control stays absent | GPUI UI test |
| 347-7 | Regression: project folders and Recents | Top-level Projects Show More remains unchanged; Recents keeps its existing lazy loading and selectors | Existing GPUI UI tests |
| 347-8 | Ordering and eligibility | Rows remain in the existing sorted order; pinned rows remain in Pinned; archived rows follow the existing archive filter | GPUI UI test |

## Implementation plan

- Replace the per-project expanded set with independent in-memory visible and rendered-total counts in `ThreadsBlock::Organization`.
- Record each open project's current eligible task total while projecting its sorted rows.
- Render the project-level 「展开更多」 control only while that project's window is below its eligible total, and route activation through the shared `Control::More` path to append one page.
- Extend the existing Sidebar bottom/viewport-fill callback to grow project task windows by one page alongside the already-existing Recents window.
- Add UI regression coverage for the control's one-page growth and disappearance, short/tall viewports, independent project state, sorting, and exhaustion.

No database pagination, persistence, schema changes, new dependencies, or changes to the top-level project list are in scope.

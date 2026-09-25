# Issue #191 — Branch switching in existing conversations

Source: https://github.com/puzige/vega/issues/191
Status: frozen for implementation, 2026-09-25.

## Decision

Keep the R49 utility bar draft-only and keep the Environment card free of a
branch row. Mount the existing `BranchSelector` in the persisted conversation's
Composer footer when the current thread belongs to a Git project. This is the
separate Composer entry option from the 2026-09-13 branch-entry handoff. It
preserves the R49 page states and restores access without duplicating the
selector or changing Git authority.

## Contract

- A new-task draft with no messages keeps the existing folder/branch chips in
  the R49 utility bar. The bar geometry, selectors and visibility predicate do
  not change.
- A persisted conversation with at least one message and a Git project renders
  one current-branch chip in the Composer card's leading footer control group,
  immediately after the existing add-context control. The utility bar remains
  absent on that route.
- The chip uses the existing route-owned `BranchSelector` entity and its
  current-head label. It must be mounted exactly once on each route state.
- Opening, closing, switching, pending state, typed errors, focus and scrolling
  continue through the existing BranchSelector request/controller flow. Keep
  its existing popup placement configuration and `set_chip_chrome` style.
- Standalone threads, missing project context and non-Git projects do not gain
  a branch chip. The Environment card remains without `environment-branch`.
- No new persistence, Git process, branch authority, shared type or dependency
  is introduced.

## Acceptance matrix

| ID | Setup and action | Expected evidence |
|---|---|---|
| B1 | Mount a project-backed new-task draft | R49 utility bar and branch chip remain present once; Composer footer has no duplicate selector |
| B2 | Mount a persisted Git conversation with a message | Utility bar is absent; current-branch chip is visible in Composer footer |
| B3 | Activate the footer chip | Existing selector opens through its production request path and lists the supplied branch snapshot |
| B4 | Select a different branch | Existing switch request is emitted and controller result updates the current-head label; pending/error paths remain intact |
| B5 | Mount standalone, no-project and non-Git routes | No branch chip is rendered |
| B6 | Render Environment and draft utility bar | `environment-branch` remains absent; existing R49 geometry and visibility assertions pass |

Tests use the production `ConversationStream` and controller event flow. Native
macOS acceptance after merge checks the chip's discoverability, footer fit,
popup placement and a real branch switch. Automated GPUI results do not claim
that native acceptance.

## Plan and ownership

The main agent owns this contract, review, PR and integration. A dedicated
implementation agent owns the Composer mount point and focused production UI
tests. First add the failing persisted-conversation visibility/open tests, then
mount the existing selector in the active conversation footer and run only the
Issue #191 tests plus relevant R49 regressions. Do not run the workspace suite
locally; cloud PR checks are the merge gate.

## Non-scope

Do not render the utility bar after a message exists. Do not restore the
Environment row or change R49 geometry. Do not redesign the selector, alter tab
or workspace visuals, or add Browser, file-tree or side-chat functionality.

## Contract alignment

The 2026-09-13 handoff offered preserving the bar, adding a separate Composer
entry or restoring the Environment row. This spec selects the separate Composer
entry because the current frozen R49 contract explicitly keeps the utility bar
off persisted conversations and removes the Environment branch row.

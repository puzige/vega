# Issue #192 — Review focus intent

Source: https://github.com/puzige/vega/issues/192
Status: frozen for implementation, 2026-09-25.

## Problem

The global Review action opens the Diff pane but arms `focus_pending`; rendering
the pane then moves keyboard focus away from the Composer. The same pending
mechanism is also needed when a user explicitly activates the Diff tab.

## Contract

- Global reveal actions (`workspace_open_diff`, including the main header and
  workspace menu) open or reveal Review while preserving the current focus.
  When first invoked from a conversation Composer, Composer remains focused.
- Explicit Diff-tab activation, including keyboard activation, focuses the Diff
  view through `workspace_focus` after it is mounted.
- Repeating a global reveal action is idempotent with respect to focus: it does
  not steal focus from Composer or displace focus from an already active Diff.
- Keep `focus_pending` (or an equivalent explicit focus-intent state) for the
  user-activated route. Do not remove the deferred focus handoff needed after a
  newly mounted Diff view becomes renderable.
- The global and explicit routes must be distinguishable before the pending
  focus is armed. Diff rendering must only consume pending focus that came from
  explicit tab activation.
- Preserve Diff creation, project/thread ownership, refresh, window placement,
  and close behavior. Do not change Commit panel focus behavior or its separate
  pending state.

## Acceptance matrix

| ID | Setup and action | Expected evidence |
|---|---|---|
| F1 | Focus Composer, then invoke the production global Review action for a project thread | Diff opens; Composer still contains focus after rendering settles |
| F2 | With Review available, explicitly activate the Diff tab | Diff focus handle contains focus after rendering settles |
| F3 | Invoke global Review again while Composer is focused | Review stays open and focus remains with Composer |
| F4 | Invoke global Review while Diff is explicitly focused | Review stays open and Diff remains focused |
| F5 | Exercise Commit open/focus and close paths | Existing Commit focus contract and pending flag are unchanged |

Use `TestAppContext`/GPUI focus handles and production handlers. Synthetic
keyboard or mouse input and screenshots are not evidence of focus ownership.

## Plan and ownership

The main agent owns this contract, review, PR and integration. A dedicated
implementation agent owns the focus-intent plumbing and production tests in the
Diff/workspace path. First add regression coverage for F1 and F2, then separate
global reveal from explicit tab activation with the smallest typed intent. Run
only Issue #192 tests and relevant Diff/Commit focus regressions. Do not run the
workspace suite locally; cloud PR checks are the merge gate.

## Non-scope

Do not remove explicit Diff focus, change Review or Commit visuals, alter pane
geometry/window ownership, or change branch selector behavior.

## Decision basis

The 2026-09-13 handoff identifies global-button open and explicit tab activation
as separate user intents. The global route preserves focus; selecting a tab
explicitly transfers focus to its content, matching `workspace_focus` and the
existing global-pane behavior.

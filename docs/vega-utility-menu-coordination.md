# Utility menu coordination follow-up

User-authorized correction, 2026-09-17. Scope: branch menu initial row highlight
and mutual exclusion of the Composer project/branch menus. Preserve the chip
geometry/colors and R68 dismissal/capture contracts from the prior task.

## Diagnosis

BranchSelectorModel::apply_snapshot initializes the logical keyboard candidate
to the first switchable branch. render_branch_row paints that candidate as
focused immediately, in addition to painting the current branch as selected.
This creates an unearned second highlight before keyboard/pointer navigation.
Logical activation candidate is not evidence of user keyboard navigation.

Project and branch open states have separate owners. close_composer_popovers
omits the branch entity, and BranchListRequested opening does not coordinate
with project open state. Trigger capture intentionally blocks outside-close
handlers, so outside-click dismissal cannot provide sibling mutual exclusion.

## Contract

C1. On opening/reopening and receiving a snapshot, with pointer on trigger or
away from rows, only the current branch has a selected surface/check. Do not
paint automatic logical candidate as keyboard focus. Actual arrow-key navigation
must still show its target and activate the correct visible switchable branch.
Filtering, refresh and open/close must not leak stale visual keyboard intent.
Preserve controller authority, logical switchability, row colors and geometry.
The first arrow action with no visual keyboard target reveals the logical
candidate rather than skipping it; subsequent arrows perform the existing
bounded walk. Enter keeps its existing logical candidate semantics.

C2. Project and branch popups are mutually exclusive at production event
boundaries in both directions. Clicking the other trigger takes one click:
old popup closes through its normal close path, new popup opens, only its chip
retains open background after pointer leaves. Clicking that trigger again closes
it. Do not rely on mouse-down-out, which capture correctly suppresses. Do not
close and immediately reopen the same popup via event ordering/reentrant entity
updates. Retain BranchSelectorClosed cleanup semantics and pending operation
ownership. No new Git refresh or mutations except existing normal open action.

C3. Cover real mounted Composer mouse sequences project→branch→project and
branch→project→branch, repeated toggling, outside close and Esc. Assert exactly
one visible popup and relevant entity state, no hidden ghost menu. Painted-quad
tests (light/dark) prove initial current-only row surface and reopen reset;
actual keyboard action verifies intentional focus still works. Preserve R68,
R64, previous chip tests and project350 width. Add no dependency/public testing
API, non-test unwrap/expect, or unrelated redesign.

## Verification and ownership

Dedicated executor owns branch_selector, narrow ConversationStream subscription/
actions/utility-bar wiring, and their tests. Main owns this spec, independent
review and native installation. Read AGENTS and exec/design docs first. Tests
must fail on the pre-fix behavior; reverse each behavioral fix independently
to demonstrate the new test fails, restore and run focused regression + full
workspace/fmt/clippy via cargo-lock. Save fresh raw logs and exact failure names
in docs/vega-utility-menu-coordination-delivery.md. No push/merge/install by
executor, do not overwrite others' work. Native acceptance is primary-owned.

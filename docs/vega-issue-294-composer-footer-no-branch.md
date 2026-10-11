# Issue #294 — Composer footer without a branch entry

Status: frozen for implementation, 2026-10-11.

## User decision and evidence

The user supplied a Composer screenshot showing a branch icon and `master` between the add-context control and the permission status, and explicitly requested that this branch entry not be shown.

The current production Composer mounts `composer-footer-branch-chip` for a non-draft project-bound conversation with existing messages. This came from the earlier Issue #191 entry contract. This new user decision supersedes the requirement to expose branch switching in the existing conversation's Composer footer. It does not supersede branch-service safety limits or the separate New Task utility-bar controls.

Issue: https://github.com/puzige/vega/issues/294

The implementation baseline is current master containing the official v0.1.64 release. The user's supplied screenshot does not identify a version; it is not evidence of an independently verified installed build.

## Contract

1. The Composer card footer never mounts a branch icon, branch label, branch-selector trigger, focus target, or reserved branch placeholder. This applies to Native and Codex tasks, existing/history-hydrated project tasks, non-Git and projectless tasks, and fresh drafts.
2. The existing add-context control, Native permission/model controls, Codex backend status, and send/stop controls remain rendered according to their current state. Removing the branch entry must let these controls use the existing footer layout naturally.
3. The separate utility bar above the Composer on New Task retains its current project/branch selection contract. Do not relocate the removed footer entry or introduce a replacement control for existing conversations in this task.
4. Preserve branch state projection, branch-switch safety policy, draft text, and existing controller subscriptions used by the New Task utility bar. No user repository branch or runtime preference is modified as part of implementing this UI change.
5. Ordinary and narrow layouts and both themes follow the same footer rule. No new dependency, token, database migration, preference, or provider/ACP runtime behavior is needed.

## Acceptance matrix

| ID | Production path and action | Expected result | Evidence |
|---|---|---|---|
| F01 | Render a project-bound Native conversation with existing messages and a live branch projection | No footer branch entry/icon/name or branch placeholder; add, permission, model, and send controls remain | GPUI production render regression, demonstrate failure before the fix |
| F02 | Hydrate an existing conversation/history and render again | The branch entry does not reappear after hydration/state changes | GPUI production render regression |
| F03 | Render project-bound Codex conversation | No footer branch; Codex status and send/stop ownership remain | GPUI production render regression |
| F04 | Render the same footer in Light/Dark and ordinary/narrow layouts, including active send/stop states where relevant | No footer branch or clipping of required controls; geometry uses existing tokens | Scoped GPUI render/geometry coverage |
| F05 | Render fresh New Task utility bar and exercise its existing branch selection route | Project/branch utility controls still exist and keep current request/draft behavior; footer has no branch | Existing scoped utility-bar/controller regressions plus updated entry contract coverage |
| F06 | Render projectless/non-Git context | No footer branch; unrelated controls remain | Scoped existing or updated GPUI regression |
| F07 | After merge, inspect the official app in the user's affected conversation | Composer footer matches the user's request | User manual acceptance, NOT RUN until supplied |

## Implementation plan

1. Preserve a focused failing production GPUI regression showing the unwanted footer branch before changing production rendering.
2. Remove footer-only mounting and dead footer-only helpers. Keep the New Task utility-bar selector, branch projection, and security behavior intact.
3. Revise only legacy assertions explicitly requiring the superseded Issue #191 footer entry. Preserve unaffected selector/controller/draft assertions and align the legacy entry documentation and current UI specification with this user decision.
4. Run only the affected package's card-specific and nearby regressions locally; retain exact commands, first failure, final results, and raw log hashes in the delivery record. Run formatting and diff checks. Leave the full workspace gate to PR CI.
5. Review the final diff, open one PR referencing this Issue without automatic-close keywords, wait for the required cloud gate, merge, and move the card to In review. Await the user's manual acceptance before closing or cleaning this feature's worktree.

## Boundaries and verification

Implementation belongs to a dedicated subagent in the isolated task worktree. Do not change unrelated cards, runtime config/credentials, installed application, or user Git branches. Do not add code comments, dependencies, blanket test skips, weaker safety assertions, or local workspace-wide tests. Existing branch safety tests remain authoritative for the retained utility-bar service.

Local command filters will be finalized against the actual affected test names. Native screenshot geometry and GPUI behavior are separate evidence classes; an automatic render test is not user manual acceptance.

## Revision record

- 2026-10-11: freeze the user's instruction to remove the Composer footer branch entry; explicitly supersede only the prior #191 footer visibility requirement.

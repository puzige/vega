# Draft branch access and quiet chrome

2026-09-16 user-requested correction to R69 R6/A7 and branch-popup-upward scope.

## Cause

Home renders a real branch selector but `window/render.rs` skips `ensure_branch_route` for drafts. Its list handler has no active route and returns StaleGeneration, rendered as Branches unavailable. `artifact_project_root` resolves the project row, not a persisted thread row; excluding branch access for that reason was incorrect.

## Requirements

- Project-bound home drafts initialize the existing branch controller and list the actual selected project's Git branches when opened. Standalone drafts must not create a branch controller. Keep artifact controller exclusion unchanged.
- Keep normal branch switching through existing prepare/execute authority and guards; no alternate Git mutation path. Listing and switching must not materialize a thread, send a message or lose draft text. Project switches and route changes invalidate old work through existing fences. Do not change user repositories during native checks: only list their branches; switch only in owned temporary test repositories.
- Update the old R69 R6/A7 spec/test narrowly: artifact remains absent, branch is allowed with a valid project. Do not weaken dirty-repo, busy, stale-generation or route authority checks.
- Composer branch trigger: no border, transparent at rest, neutral hover, fully rounded pill instead of small rounded corners, balanced horizontal inset using existing spacing. Keep 16px icon, existing font and hitbox height. Non-Composer bordered trigger unchanged.
- Branch popup: remove explicit border in light and dark, retain existing elevated surface, radius, subtle shadow, upward placement and deferred paint. No global style change. Preserve search, loading/error honesty, outside-click/Esc dismissal and disabled creation action.

## Acceptance

Real production root + owned Git repo: draft lists real branches, can switch via existing controller, HEAD changes only in owned repo, zero durable thread rows, draft text preserved. Non-Git/standalone and project-switch stale reply regressions. Existing R69 and branch safety tests remain valid except explicitly superseded A7 assertion. UI popup/upward/R68 tests pass. Run fmt, clippy, workspace tests with cargo-lock; retain initial failures. Native check installed candidate shows real branch rows and quiet rounded trigger/popup. Delivery: docs/vega-draft-branch-access-delivery.md; mark untested items honestly.

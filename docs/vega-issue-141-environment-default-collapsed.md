# Issue #141 — Environment starts collapsed

Status: implementation contract. Source: [Issue #141](https://github.com/puzige/vega/issues/141).

The user requests that Environment no longer automatically expand. This supersedes
the default-open premise in Issue #69 and earlier Environment test fixtures.
Every new VegaWindow starts with `environment_collapsed = true` and
`environment_overlay_open = false`, including a window created after restart.
Project/task navigation and resize must not open a never-manually-opened panel.
The header toggle still opens/closes the rail or overlay, with the existing
breakpoint, dismissal, focus and truthful selected-fill behavior. A manually
opened wide rail keeps its existing behavior across narrow/wide resize.
No persistence, setting, migration, route-reset policy or new API is introduced.

## Acceptance matrix (before implementation)

| ID | Precondition / action | Observable result | Evidence | Status |
|---|---|---|---|---|
| A1 | Fresh wide project task window | No rail/overlay; header slot unselected | Production root + painted quads; native | Production PASS; native pending where applicable |
| A2 | Never-opened window; switch task→project draft→task | Remains absent/unselected | Production root | Production PASS; native pending where applicable |
| A3 | Never-opened window; resize narrow→wide | Remains absent/unselected | Production root | Production PASS; native pending where applicable |
| A4 | Click header toggle | Rail opens, selected fill appears; existing close/focus/overlay behavior remains | Production root; Issue #69 + R21/R45 regressions; native | Production PASS; native pending where applicable |
| A5 | Open first window manually, construct another window / restart app | New window starts collapsed | Production root construction; native restart | Production PASS; native pending where applicable |
| A6 | Existing geometry/action tests require open panel | Explicitly open through production toggle before unchanged assertions | Full Vega binary tests | Production PASS; native pending where applicable |
| N/A | Persistence and external service failure | No persistence/network changes | Assignment audit | N/A |

## Implementation plan

Add a failing production-root default visibility/paint regression first and keep
the raw red output. Change only constructor default, then adapt open-panel test
preconditions through real header toggles rather than changing their assertions.
Audit all Environment assignments for automatic opening. Run focused test,
Vega binary regressions and formatting; package candidate for main-owned native
acceptance and cloud integration. Rollback is a revert of this task.

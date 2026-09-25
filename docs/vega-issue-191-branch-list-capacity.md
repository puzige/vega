# Issue #191 — Branch list capacity and error reporting

Status: frozen for implementation, 2026-09-25.

## Problem and evidence

The Composer branch selector added by [the Issue #191 entry contract](vega-issue-191-branch-entry.md) is intentional and remains in scope. In v0.1.13, opening it for a clean project with 13 branch refs showed `Too many branches` and no rows. The inspected project has 9 local heads and 4 remote-tracking refs; the existing selector lists local heads only.

Read-only inspection confirmed 14,449 tracked paths. Their NUL-delimited `git ls-files` output is 1,030,287 bytes and the corresponding `check-attr --all` output is empty, so this project is well within the existing 8 MiB retained-output budget. The branch-list path calls `parse_nul_paths`, which uses the workspace-wide 10,000-path cap. That parser returns `OutputTooLarge` before the selector can receive a branch snapshot. The selector also maps the generic `OutputTooLarge` and `ArtifactLimit` codes to `Too many branches`, which obscures the actual failure.

## Contract

- A clean Git project with 14,449 tracked paths and 13 total branch refs (9 local heads and 4 remote-tracking refs in the reported project) can load its local branch snapshot when existing byte, retained-memory, process-output, and timeout budgets are respected. The selector continues to list local heads only.
- Branch-service filter identity capture has an explicit finite limit of 25,000 tracked paths. This identity is captured for branch-list refresh and switch preflight. Workspace snapshot enumeration, branch-switch authority/change paths, and unrelated parsers retain their existing 10,000-path limit.
- Existing 8 MiB branch retained-output and runner stdout ceilings, 10-second read timeout, process cancellation, and validation of every path and attribute remain in force. The cap is not made unbounded.
- Exceeding the branch-list tracked-path count returns a branch-specific typed error and an actionable selector message that identifies too many tracked files.
- Exceeding the local branch-ref count returns a distinct branch-count error and a selector message that identifies the 10,000-local-branch display limit.
- Other output or artifact safety-limit failures never render as `Too many branches`; their message describes a branch-list safety limit and gives a practical recovery action.
- The existing Composer entry point and #195 selector wiring remain unchanged.

## Acceptance matrix

| ID | Requirement | Setup/action | Expected evidence |
|---|---|---|---|
| C1 | Report the actual cause | Parse a branch-list path payload above 25,000 entries | Returns the branch-path-limit code, not generic output overflow |
| C2 | Support the reported repository size | Feed the branch service a clean fixture with 14,449 distinct tracked paths, valid attributes/status and 9 local refs | `BranchWorkspaceService::refresh` returns 9 local rows, matching the reported project; its 4 remote refs remain excluded by the existing `refs/heads/` query |
| C3 | Keep the bound finite and scoped | Parse 25,001 branch-list paths and run existing workspace/switch path-limit cases | Branch list refuses with its typed limit; unrelated 10,000-path guards remain unchanged |
| C4 | Distinguish ref overflow | Parse more than 10,000 local branch refs | Returns a branch-count-limit code |
| C5 | Show accurate messages | Exercise selector labels for path-limit, ref-limit, generic output overflow and artifact limit | Only ref overflow mentions too many branches; path limit identifies tracked files; generic limits identify safety bounds |
| C6 | Preserve entry wiring | Run existing Composer branch-entry/selector tests | Existing entry opens and controller request flow remains unchanged |
| C7 | Keep budgets and cancellation | Exercise exact/over retained bytes and cancellation/read-timeout paths | Existing byte, output, timeout and cancellation results remain bounded and typed |
| C8 | Native application behavior | Open the Composer selector in a clean 14,449-path Git project | Real branch rows display; native check is recorded separately from mocked automation |

## Implementation plan

1. Add a path parser with an explicit caller-supplied count limit while retaining the existing 10,000-path parser contract for its current callers.
2. Use a 25,000-path constant only for branch-service filter identity capture; retain current byte/output/time ceilings and attribute validation.
3. Add distinct branch-path and branch-count error codes. Return the branch-count code at the service parser and selector snapshot count guards.
4. Replace the inaccurate UI mapping with precise messages for branch-path count, branch-ref count, and remaining generic safety errors.
5. Add process-boundary-stub service coverage for 14,449 paths/13 refs, path-count overflow, ref-count overflow, selector messages, and existing selector wiring.

## Non-goals

Do not remove or move the Composer selector; do not change the draft utility bar, Environment card, branch switching semantics, branch authority, or Git process policy. Do not globally raise the workspace `PATH_LIMIT`, remove count or byte caps, or modify the Loom user project. Desktop acceptance is pending and is not implied by unit/GPUI test results.

## Verification scope

Run only Issue #191 branch service and selector tests locally, plus `cargo fmt --all -- --check`, `git diff --check`, and Clippy for the affected crates if required by the delivery workflow. Do not run the workspace-wide suite locally. Record exact commands and raw results in the delivery report. Native desktop acceptance remains `NOT RUN` until verified on the installed application.

## Revision record

- 2026-09-25 initial freeze: isolate the branch filter-identity path cap, keep all other resource limits bounded, and replace the inaccurate selector error.
- 2026-09-25 clarified the inspected repository has 9 local heads and 4 remote refs. The existing selector continues to show the 9 local heads only; acceptance fixtures and documentation do not claim that remote refs are displayed.

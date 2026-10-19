# Issue #129 — Sidebar titlebar vertical alignment

Source: https://github.com/puzige/vega/issues/129

## Contract and root cause

The screenshot marks Sidebar/Search/Back/Forward below the main header title.
The sidebar mounts a 40px toolbar after 12px top padding: center = 12 + 40/2 = 32px.
The main header and window shell controls use a 46px band: center = 46/2 = 23px.
The actual mismatch is 9px. Align the sidebar controls to the existing main
header band; preserve the main header border's existing half-pixel content
centering (tolerance 0.5px), and all button actions and horizontal geometry.

Use MAIN_HEADER_HEIGHT for sidebar toolbar height and mount it at window top.
Preserve subsequent sidebar content positions by retaining the existing total
space before New Task: old = 12 + 40 + 12 = 64; new = 46 + 18 = 64.
Represent any new shared spacing with a documented Layout token. Toolbar must
not shrink at short window heights. Preserve footer/scroll behavior and native
traffic light reservation. This supersedes R43's unchanged-titlebar-geometry
constraint only for the sidebar toolbar vertical placement.

## Acceptance matrix (before implementation)

| ID | Scenario / operation | Expected observation | Evidence | Initial status |
|---|---|---|---|---|
| A1 | Render production root with sidebar visible | Four 28px controls centered at y=23; main header content within 0.5px; gaps 4px and icons 16px | Production layout test, native screenshot | Screenshot fails: y=32 |
| A2 | Toggle sidebar hidden then visible | Controls remain on header centerline; search and history actions unchanged | Existing palette production test extended | Pending |
| A3 | Disabled and enabled history; empty home and thread route | Slots retain geometry; correct actions/state | Existing production tests | Pending |
| A4 | Compare New Task and scroll/footer bounds | New Task starts y=64; content horizontal positions unchanged | Production bounds | Pending |
| A5 | Light/dark; 1403x860, 1200x760, 960x600; 1229/1230 widths | Alignment stable, no toolbar shrinking/overlap | Production layout/native evidence | Pending |

No persistence schema or error handling changes; existing sidebar visibility
persistence remains the recovery contract. No provider/model/network required.

## Implementation plan

1. Extend existing production root geometry assertions to detect vertical defect;
   record failing output before layout fix.
2. Update sidebar toolbar/container spacing only, using theme geometry; document
   any spacing token in design guidelines before implementation.
3. Run focused palette/layout regressions and formatting; cloud PR check remains
   required before merge. Preserve first failure and raw logs.
4. Review diff and capture real native application geometry with the candidate
   build in persistent evidence outside the worktree. Do not replace/stop a busy
   user app. Record any blocked native validation honestly.

Rollback: revert this issue's commit. No data migrations or dependencies.

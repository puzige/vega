# Composer branch popup direction and icon

User-approved scope, 2026-09-16. This task supersedes earlier downward-placement behavior only for the Composer branch selector. Reference is the user's visible screenshots, not third-party source/assets.

## Contract

1. Composer branch menu opens above its trigger with the existing 4px gap; preserve horizontal anchoring and window safety margins. Remove the unconditional downward setting in ConversationStream render. Non-Composer placement API may remain.
2. Replace the misleading up/down chevrons with a shared, neutral 16px Git branch outline icon in the trigger and each branch row. Use the existing bundled icon if available, otherwise an independently authored SVG in the shared icon module using its existing stroke/viewBox conventions. No app-brand logo change, font change, or dependency addition.
3. Preserve deferred painting, outside-click dismissal, trigger toggling, Esc, search, pending/disabled/error state and controller authorization. Do not invent branches or implement branch creation.
4. Long lists scroll within existing bounded menu geometry. Validate that normal and minimum-size Composer layouts place the menu above the trigger without covering the input; if available height makes this impossible, report the case before expanding layout scope.

## Evidence

Production render tests: popup.bottom <= trigger.top, expected gap 4px where not window-constrained, popup within viewport at 1403x860 and 960x600; short/error and long-list states. Test actual Composer mounting, not only an isolated selector default. Regress existing R68 dismissal/search/trigger tests and branch controller tests. Check icon render paths for both trigger and rows.

Arithmetic: when trigger top is Y and popup height H, desired popup bottom is Y-4 and top is Y-4-H. The 4px gap is unchanged; no additional geometry is introduced.

Run formatting, strict clippy and workspace tests through the repository lock protocol; record exact commands/results and NOT RUN items in docs/vega-branch-popup-upward-delivery.md. Native screenshot evidence is distinct from behavioral tests.

# R51 Tab controls — approved implementation slice

## Authority and corrections

This task supersedes the proposed R51 PRD/TDR in Loom for this slice. User approved main-agent coordination and Luna Max implementation on 2026-09-13. Source is user-visible screenshots and Vega code, not extracted third-party code/assets.

The supplied screenshot shows a file tab and Review tab together. The earlier claim that the reference has no tab strip is withdrawn. Preserve Vega multiple tabs. Pixel measurements are approximate visual targets, not proof of another product's internal implementation.

## Scope

- Existing workspace tabs only: explicit 28px height, 10px radius, 13px label, 16px existing type icon, 8px horizontal inset and 8px content gap; use named theme tokens for new geometry.
- Inactive tabs transparent, active primary text, inactive secondary text. Preserve existing active fill expression so R50 can change it independently.
- Close control retains a 24px accessible hit area inside the 28px row. Use existing shared icon rather than a 7px click target. Keep close visible on active tabs; reveal inactive close on hover or keyboard focus without moving text or changing width. If the current GPUI focus APIs cannot support this robustly, report to main agent before changing semantics.
- Preserve tab switching, close propagation (closing must not activate a stale tab), selected-tab fallback, dock move, maximize/restore, terminal process identity and focus contracts.
- Keep existing outer header geometry and global controls. No new placeholder Browser, Files or Side chat entries. No new shortcuts or menu availability policy changes.

## Ownership

Executor owns workspace tab renderer and relevant existing tests in crates/vega/src/window/workspace.rs, narrowly scoped TAB_* additions in crates/vega_theme/src/lib.rs, and this task's delivery report. R50 separately owns active fill and sidebar rhythm. Do not edit R50 worktree or implement its changes. Main agent owns integration and design-guideline update.

## Verification

Use existing production UI harness where available to check single/multiple tabs, active/inactive close, long titles, narrow layout, keyboard activation and unchanged outer geometry. Meaningful behavior tests only. Run fmt, focused tests and clippy; coordinate full workspace suite with main agent to avoid competing fixture runs. Report native visual checks as NOT RUN unless actually performed. Do not install, merge or push. Delivery: docs/vega-r51-tab-controls-delivery.md with files, commands/results, evidence limitations and residuals.

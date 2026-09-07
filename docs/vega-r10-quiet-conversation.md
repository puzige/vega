# R10 quiet conversation

User asks to remove per-conversation trailing cost/Token/duration indicators (2026-09-06); assistant agreed to keep these in task details or usage view. Implement the approved UI change rather than leaving a promise. Default conversation transcript should no longer show full statistical summary cards; preserve essential running/error/interruption status and retry/continue actions. Retain underlying usage/pricing computation and persistence. Provide a real unobtrusive task-details/usage disclosure or panel to inspect existing statistics on demand. Do not fabricate aggregate data or remove error visibility. Keep existing three-column/dock/draft/controller contracts and original theme tokens. Header usage may be moved into same disclosure to reduce noise when appropriate.

Parallel credential agent owns keystore/Settings/provider plumbing; your scope vega_ui conversation/summary rendering and minimal root wiring only if needed (coordinate before modifying root). Also replace preparing-request Keychain conditional hint with generic request-preparation text, because user explicitly removed Keychain dependency. No credential backend, Settings files, Cargo/dependency edits. Main owns native verification. Focused production UI test for default hidden stats + reveal with actual existing summary + failures still visible; fmt/check/build appropriate scope. Deliver docs/vega-r10-quiet-conversation-delivery.md with changed files/behavior/logs/limits and commit. Performance deferred.

## Revision — centralized Settings Usage (2026-09-06)

User clarified that the desired presentation is ZCode's Settings usage statistics. This supersedes the per-task usage disclosure. Settings gains a fifth `使用统计` section with an `应用用量` pill, muted metrics band, annual daily/weekly/cumulative activity visualization, 7/30-day model-token chart, model distribution donut and numeric legend, and refresh. All data is a typed persisted-usage projection from the conversation service; UI does not query SQLite. Metrics include actual total tokens, priced cost (explicit unpriced coverage), peak daily tokens and active-day/streak facts. Unsupported duration metrics are omitted. Empty/loading/error states remain explicit; charts never fabricate history. Repeated successful transcript summaries occupy no layout; failed/interrupted outcomes remain visible independently of statistics. Existing retry/continue actions remain unchanged. Credential preparation failure clears pending submission while preserving the draft and displays a fixed actionable error. Rendering follows existing theme tokens and scrollable Settings layout. Main performs native acceptance; worker uses production render/handler regressions and required build gates.

The empty-conversation composer project band also omits the visible cost/token meter; internal meter restoration and accounting APIs are retained. Usage currency is labeled estimated cost and uses integer microcent formatting.

Default trend/distribution range is 近7日, matching the observed reference. Empty heatmap cells retain faint theme border contrast against the muted card.

## Native parity follow-up (2026-09-06)

Heatmap date labels must identify the exact inclusive rolling-window start and end, including a partial final month, instead of sampled month labels that can incorrectly end at the previous month. The model distribution donut centers the selected range's compact total and a Tokens label; this overlay is noninteractive and does not alter chart controls or layout.

The nonempty conversation header also omits the visible meter text between the branch selector and trusted actions. Its title, project/branch, review/commit actions, tail controls and internal meter snapshot/calibration APIs remain unchanged.

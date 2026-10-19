# Thinking stream continuity and truthful diagnostics

## User decision and evidence

2026-10-15: user explicitly requests removing remaining fixed per-response/per-delta thinking limits and completing error classification and diagnostics. Supersedes #375's retention of reasoning-specific per-response byte guards. In 0.1.122, a primary call failed after 226 seconds with reasoning_limit; stored data cannot prove whether 64 KiB delta or 256 KiB response guard fired. Do not backfill guesses.

## Contract

- Valid ThinkingDelta, SummaryDelta and provider-required reasoning replay must not terminate a task solely for fixed thinking byte thresholds (delta, response, run) or equivalent arbitrary replay item thresholds. Remove gates, not raise them. Preserve legitimate generic parser/protocol validation, cancellation, provider/model output limits and actual context budgeting/recovery.
- Separate display retention from model-visible data. Display/event duplicates remain bounded with UTF-8-safe retention; reaching a display cap never cancels execution. Required replay and reasoning_content remain intact until normal safe compaction; do not truncate protocol data to fit display limits. Non-replayed thinking must not be retained unboundedly just because the abort was removed.
- Keep context pressure based on actual model request tokens. A true output limit, malformed provider payload, transport interruption or context overflow must not be mislabeled generic reasoning size failure or advised historical compaction when it cannot help.
- Complete structured safe diagnostics through runtime, conversation, storage and restored UI. Preserve applicable failure source/kind, scope, observed/limit quantities and units (nullable when unknown), not raw provider text or thinking. Removed guards remain readable for legacy events, unknown legacy scope remains unknown. Additive migration if needed; no live DB/config edits. Avoid invented values and wide unnecessary new diagnostic framework.
- Reopened and live errors agree on known cause and actionable next step. No routine success/help banners. Detailed measurements belong in diagnostics, not repeated main UI text.

## Acceptance matrix (before implementation)

| Case | Action | Observable expected outcome | Verification |
|---|---|---|---|
| Large delta | Stream valid >64 KiB thinking/summary | Final answer/tool continuation completes, no reasoning failure | production runtime mock |
| Long response | Stream >256 KiB total in one response | Completes without task cancellation, display bounded | runtime + conversation |
| Required replay | Receive large reasoning/replay including >256 items | Exact protocol data reaches next request, actual context budget applies | provider/runtime mock |
| Non-replayed long thinking | Receive repeated large chunks | bounded duplicate/display retention, no phantom context pressure | runtime regression |
| Real provider/context/transport failure | Inject distinct typed failures | Correct code and safe detail, no incorrect recovery | runtime/conversation |
| Restart | Persist then reload classified failure | live/restored classification and relevant metrics agree | store/conversation/UI |
| Legacy database | Load prior reasoning_limit record | unknown detail stays unknown; migration safe | migration/store |
| Cancellation | Cancel long stream | promptly cancelled, no extra tool execution | runtime mock |

## Plan and boundaries

Dedicated implementation subagent owns affected runtime/provider/conversation/store/UI code, additive migrations and regressions. It may update this spec with concrete diagnostic schema before editing that schema, and must flag material scope deviations. Main owns coordination, review, integration, PR and CI. No new dependencies, unrelated refactors, code comments, local build/check/test/clippy/package or application restarts. Allowed local checks: fmt and diff/static. Compilation and all feature regressions execute via cloud PR Check. Preserve prior context-budget recovery and tool audit behavior. Old tests asserting removed guard behavior may be replaced with continuity assertions under this explicit contract, never weakened without replacement.

Deliver implementation evidence in docs/vega-issue-393-delivery.md. Squash merge only green checked head, set issue In review, leave open until user real-model acceptance. Do not claim installed application updated merely because source merged. Rollback via checked revert PR.

## Concrete diagnostic schema and adapter review

Add migration 0025 with nullable failure_source, failure_kind, failure_scope, failure_observed, failure_limit, failure_units, failure_observed_items, failure_limit_items and failure_tool_index (to retain existing credential-projection classification). Closed vocabulary plus nonnegative numeric validation; old rows remain NULL. Reuse existing diagnostic events/metrics and failure ownership, no new logging subsystem. Separate protocol frame capacity from malformed data, output-token limit, transport and model-context overflow. Root and primary diagnostics preserve applicable cause and units. Provider prose is never persisted as diagnostic detail.

Responses shares its text assembler between reasoning and ordinary output. Removing its arbitrary delta/done/aggregate text gates applies to both; reintroducing separate final-output byte counters would reproduce the same synthetic interruption. Preserve existing generic 4 MiB individual SSE frame and 256 KiB tool-argument checks. Frame protection counts complete events across network chunks and accepts LF/CR/CRLF separators; it must not count a whole long stream as one frame. Replace full text copies used only for delta/done dedup with incremental length and existing SHA-256 verification; required replay remains exact.
## Public implementation comparison (2026-10-15)

Inspected public streaming paths, not a claim about undisclosed server internals or every code path:

- Pi `packages/ai/src/api/openai-completions.ts` at `550f1f3ecd75bf756ab3c379ca9503e808a6c6d1`: reasoning delta is appended and emitted; reasoning_details preserved separately. No equivalent 64 KiB delta / 256 KiB response abort in inspected handler. https://github.com/earendil-works/pi/blob/550f1f3ecd75bf756ab3c379ca9503e808a6c6d1/packages/ai/src/api/openai-completions.ts
- Codex `codex-rs/codex-api/src/sse/responses.rs` at `e89b7c442c0433ad5167e72a5a25bb6d0712766a`: reasoning summary/text deltas map to response events, no such reasoning byte cutoff in inspected handler. https://github.com/openai/codex/blob/e89b7c442c0433ad5167e72a5a25bb6d0712766a/codex-rs/codex-api/src/sse/responses.rs
- OpenCode `packages/ai/src/protocols/openai-chat.ts` and `utils/lifecycle.ts` at `4397ee44d038a5f17bf6ac161431673d93e563b5`: reasoning deltas route to lifecycle events without equivalent byte cutoff. Its runner distinguishes provider output-token `length` and bounded continuations from context compaction. https://github.com/anomalyco/opencode/blob/4397ee44d038a5f17bf6ac161431673d93e563b5/packages/ai/src/protocols/utils/lifecycle.ts ; https://github.com/anomalyco/opencode/blob/4397ee44d038a5f17bf6ac161431673d93e563b5/packages/core/src/session/runner/llm.ts
- Claude official thinking documentation describes thinking/output token budgets, not client-side 64/256 KiB thought cutoffs. Claude Code internal implementation is not established by API docs; do not claim proof of absence. https://platform.claude.com/docs/en/build-with-claude/extended-thinking

Conclusion: no supporting evidence for treating Vega's fixed reasoning byte aborts as a shared product convention. Generic framing/protocol resource checks and model token budgets are distinct; keep their true classification.

# Unified model-context budgeting and bounded thinking display

## User decision (2026-10-14)

Issue #375 scope is revised by explicit user authorization: remove the independent 1 MiB cumulative reasoning task hard limit; use the selected model's context budget for actual requests, manage display buffers separately, and implement pre-send checks plus bounded context-overflow recovery so the same task continues with goals and progress. This supersedes the earlier 768 KiB proactive reasoning-pressure design, and the cumulative run byte/item guards retained by #372. Per-event/per-response protocol protections are distinct and must not be confused with cumulative task limits.

Evidence: a 25m50s run failed on primary attempt 34 with reasoning_limit despite prior reported input <=86,222 tokens and no context-summary attempt. The independent counter charged received thinking even when it was not replayed. Public Codex, Claude Code, OpenCode V2 and Pi context-management sources use model context/token pressure and compaction/recovery, rather than this fixed cumulative task cutoff.

References: https://github.com/openai/codex/blob/main/codex-rs/core/src/session/context_window.rs ; https://code.claude.com/docs/en/how-claude-code-works#when-context-fills-up ; https://dev.opencode.ai/v2/docs/compaction/ ; https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/compaction.md . These establish design direction, not a claim about every internal resource limit of those products.

## Required behavior

1. No task failure, forced compaction or artificial headroom trigger solely because cumulative thinking reaches 1 MiB. Remove equivalent cumulative reasoning/replay byte/item task gates; do not simply raise thresholds or reset counters while retaining unbounded duplicate event data. Retain meaningful per-response/parser guards, with truthful typed errors.
2. Model-visible reasoning_content and required opaque replay must be included in actual request context accounting, once, alongside normal messages, tools, system and images. Non-replayed display thinking must not cause phantom context pressure. Honor frozen current-model/provider policy, output reserve, token accounting/revision semantics and cancellation. Never discard protocol-required active reasoning or tool pairs merely to fit.
3. Before each next primary send (including tool continuations), check the actual request budget. Existing safe durable compaction must work within one user turn and repeat as needed, preserving task goal, user corrections, completed progress and pending work. Validate the accepted projection and re-estimate before sending.
4. If a provider rejects a request for context overflow despite preflight, attempt one safe compact-and-retry recovery for that logical request when automatic compaction is enabled and a safe complete prefix exists. Retry only a typed, confirmed context-overflow failure, not generic HTTP/network/protocol errors. Preserve visible/audit state, avoid duplicating text or executing tools from a rejected partial attempt. Cancellation, failed/invalid/no-progress compaction or a repeated overflow terminates truthfully and cannot loop. Reuse existing classification/compaction pathways where present rather than duplicate state machines. No promise of continuation when fixed input or one indivisible response exceeds real model/protocol capacity.
5. Runtime/conversation/UI display retention remains bounded independently of task execution. Release acknowledged redundant thinking payloads or retain a bounded display tail without clearing meaningful user/assistant/tool audit. UI cache eviction/truncation must not cancel the model, alter its input or claim task loss. Preserve existing visible behavior; no extra explanatory banners.
6. Legacy diagnostics remain readable and accurate. No edits to live user DB/config/install, no schema/dependency changes expected, no added code comments. Keep model-switch summaries and partial assistant cursor/CAS protections from previous fixes.

## Acceptance matrix before implementation

| Scenario | Expected production evidence |
|---|---|
| >1 MiB thinking across many tools, low model-visible token use | Same run completes, no artificial reasoning failure/forced summary, bounded event/display retention |
| Replayed thinking and opaque replay make actual request approach model limit | Correct accounting, token-based safe compaction, required replay/pairs valid |
| Actual preflight overlimit within current user turn | Summary then safe primary request, goal/progress/corrections survive, no duplicate tools |
| Typed provider context rejection despite lower local estimate | One compaction recovery and retry succeeds in same run |
| Repeated provider overflow, invalid/failed/no-progress summary, cancellation | Bounded truthful failure, no unsafe sends, lost audit or duplicate execution |
| Generic provider error / automatic disabled / individual oversized response | No inappropriate context recovery; original policy and guards retained |
| Repeated accepted compactions, restart/model switch | Latest durable summary and uncovered progress retained |

Use production runtime and conversation paths with in-process mocks, including frozen provider policy; do not rely only on helper tests. Keep assertions meaningful and adjust obsolete aggregate-limit tests to the newly authorized behavior. Local formatting/diff/static only; no build/check/test/clippy locally. Cloud PR Check runs compilation, full Clippy and Nextest. Real-provider acceptance remains user hand-test after merge.

## Delivery

Dedicated subagent owns implementation/tests in this worktree; main owns coordination, review, PR/CI/merge. Replace earlier unpushed threshold-based implementation with the revised scope while preserving unrelated work. Keep <=3 commits for this issue before PR. Merge only green checked head, mark In review and retain worktree until user acceptance. Rollback through checked revert PR. Spec deviations must be resolved before implementation.

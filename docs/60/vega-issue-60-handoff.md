# Issue 60 implementation handoff

Type: TASK
Status: ACTIVE
Purpose: Remove pricing-only chat blockers without fabricating free usage.
Handoff initiator: main agent
Recipient: dedicated implementation subagent
Primary owner / reviewer: main agent
Decision entry: [Frozen contract and test matrix](vega-issue-60-unpriced-chat.md)
Date: 2026-09-19
Time base: Asia/Shanghai

## 10-Minute Take-Over Package

Read AGENTS.md, exec-guide, kanban-delivery and the frozen spec first. Implement
P1/P2 failing production-path tests before changing production. Do not change
credentials, user configuration, unrelated cards or live UI. Report spec conflict.

## Current Status

Sibling worktree vega-issue-60-unpriced-chat, branch feat/issue-60-unpriced-chat,
base local master 4853bea. Remote fetched before creation; shared target connected.
Issue 60 is In progress. Production changes and additional matrix tests exist;
see review ledger for red evidence, 5 focused passes and latest 159/2 app result.
Original executor stopped on usage limit. Replacement dedicated executor owns
the bounded P7 assertion investigation and HTTP-test verification, focused gates
and delivery report. Main reserves the UI and full acceptance.
Main already fetched and synchronized the baseline. Executor must not rebase,
merge or switch branches: local master contains unpublished merge history, and
plain rebase can rewrite that unrelated history even when the tree is identical.

## Timeline

- 2026-09-19: #65 verified, locally integrated and closed; task branch cleaned.
- 2026-09-19: #60 Ready taken, current blockers independently inspected, contract
  frozen with Saving.previous and non-Ready unknown-price behavior.
- 2026-09-19: executor plain rebase rewrote local history before editing; main
  verified identical trees and restored the exact 4853bea task branch. No source
  or user document was lost; Git baseline/integration ownership remains main.

## Scope

Implement R1–R5 and test matrix. Executor owns necessary app source/tests and
delivery report only; main owns spec/handoff/review/full acceptance/integration.
Return exact changes, commands, pass/fail counts, first failures, log paths,
remaining risks and cargo ownership. Do not commit before main review.

## Non-Scope

No new dependencies, schema migration, settings safety relaxation, guessed prices,
provider/credential manipulation, #68 reasoning changes, UI redesign or deployment.
Do not edit master or overwrite user-untracked documents.

## Key Entry Points

- app_agent.rs PricingController::select_exact and run_agent_worker catalog input.
- window/session.rs model_options_for_pricing and apply_thread_model_selection.
- window/agent.rs start_agent_run_with_reasoning and first draft materialization.
- conversation/types/meter.rs Option estimator; store/token_usage.rs unknown totals.
- tests/pricing.rs, tests/r69.rs, tests/model_selection.rs, tests/automatic_titles.rs.

## Acceptance

Spec test matrix plus fmt, strict clippy, workspace tests and ignored tests, build,
package, real UI E2E and persistent screenshots. Executor does focused tests first;
main owns full serial gates and installation. Never call a mock native acceptance.

## Risks and Human Decisions

No unresolved product question: user explicitly removed pricing as a send gate.
Malformed pricing must stay byte-preserved and visible in Settings; chat uses
unknown cost. Native configured-model availability is checked at acceptance.
GitHub Project remains the authoritative queue, not the unrelated Loom queue.

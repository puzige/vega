# Issue 65 implementation handoff

Type: TASK
Status: ACTIVE
Purpose: Implement the frozen automatic-title contract after #63 acceptance.
Handoff initiator: main agent
Recipient: dedicated implementation subagent
Primary owner: main agent
Reviewer: main agent
Decision entry: [Issue 65 spec](vega-issue-65-auto-title.md)
Date: 2026-09-19
Time base: Asia/Shanghai

## 10-Minute Take-Over Package

First read AGENTS.md, exec-guide, karpathy-guidelines and the frozen spec.
Implement only #65; no provider/pricing fixes, cleanup or deployment.
Worktree is the sibling directory named vega-issue-65-auto-title.

## Current Status

Branch feat/issue-65-auto-title, based on accepted local master cbe9436.
#63 is locally merged, user accepted and board Done; no remote push.
#65's user-approved extra-call policy is recorded in its GitHub comments.
Shared target is connected. Every cargo invocation must use cargo-lock.sh.

Implementation is frozen and executor cargo ownership returned to main.
Source manifest SHA-256:
`d9edf3bab9badb34917e8798880f69c7ff7238adfadeb76a78ab542b25833677`.
Main full gates: workspace 1,321 passed / 0 failed / 9 ignored; explicit ignored
run 9 passed / 0 failed; fmt, strict workspace/all-target clippy, workspace/all-target
build and runtime dependency-direction check passed. First failed run remains
documented in the review ledger. Package and native acceptance passed: real
generated title, manual rename, leave/reopen and restart/reopen. Persistent local
evidence identifier is `issue-65-2026-09-19`; see the review ledger for hashes.
Evidence run identifier: `vega-issue65-acceptance.ap67AY`.
Current master has an additional documentation-only kanban-delivery commit
`d58ee3f`; preserve it at integration. Accepted binary is installed; local
integration and task cleanup are the remaining handoff steps. No remote push.

## Timeline

- 2026-09-18: user chose one extra title request, first-message fallback.
- 2026-09-19: #63 accepted; main froze #65 and created isolated worktree.

## Scope

Executor exclusively owns necessary source/test/migration changes in
vega_store, vega_conversation, vega_runtime, vega application and vega_ui.
Write docs/vega-issue-65-delivery.md with changes, raw test evidence references,
risks and unresolved gaps. Main alone owns spec and this handoff.
Do not commit until main review; do not roll back other changes.

## Non-Scope

No other issues, dependencies beyond the spec's explicit tracing approval, master edits, pushes, app installation,
credential/config modification, destructive user operations or unrelated refactor.

## Key Entry Points

- app window/agent.rs submission and window/artifact.rs MessageStarted ingress.
- app window/session.rs materialize_draft; capture original text before @ expansion.
- conversation agent/pipeline.rs durable first-turn transaction.
- conversation threads.rs and store threads.rs rename/update paths.
- UI sidebar/threads_block.rs reload and manual rename; OpenedThread header state.
- store token_usage accounting supports nullable message association.

## Acceptance

Follow spec acceptance; focused production-chain tests first. Report exact
commands, counts and first failures. Main handles final whole-workspace gates
and native installed-app tests after review; mocks are not live acceptance.

## Risks and Human Decisions

No unresolved user product choice. Network at-most-once claim is intentionally
not crash-safe exactly-once dispatch. Respect manual-title provenance, thread
identity fencing, original-text privacy, and concurrent state preservation.
GitHub Project #2 is the authoritative queue, not the unrelated Loom queue.

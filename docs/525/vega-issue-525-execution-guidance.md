# Execution and delegation guidance

## Scope and authorization

- Issue: <https://github.com/puzige/vega/issues/525>.
- User decision, 2026-10-21: implement the agreed execution/delegation guidance improvements while preserving existing independent review and CI gates; create a separate card.
- Production baseline: `e658cb4e2db1cff95ffb79e11a85fba444d6bd24`, fetched before creating `feat/525-execution-guidance`.
- Plan: v1, frozen before edits to the rule and prompt surfaces below.

#509 motivated the change: its repair run took about 65 minutes; the parent made 119 model requests, two children ran 12 delegated executions, and one implementation review began before the source was ready. These are case observations, not proof that all coordination was unnecessary. This task changes guidance; effectiveness on new tasks remains unmeasured.

## Acceptance cases

| Situation | Required guidance | Verification |
| --- | --- | --- |
| Implementation is still being written or no exact deliverable exists | Wait for a declared ready version and cessation of implementation writes before reviewing that implementation. Separately scoped plan/test review may proceed when its own inputs are ready. | Independent semantic review of rule/prompt edits; no claim of runtime enforcement. |
| Implementation is ready | Give an independent non-implementer the exact version, permitted scope, evidence and unresolved questions; coordinate subsequent writes. Parent still owns review/integration rather than implementing delegated code. | Guidance review against existing ownership and review rules. |
| Prior review exists and nothing relevant changed | Reuse unchanged conclusions; do not launch followups merely to request status or repeat completed review. | Guidance review; no fixed maximum review count. |
| New changes, evidence or findings appear | Review the delta and affected invariants against prior evidence; independently confirm the complete final head and its required gate before merge. A material baseline or acceptance change still triggers the existing revalidation/test-first requirements. | Compare wording to AGENTS, execution guide and current lifecycle rules. |
| Ordinary feature or behavior fix requires test-first | Preserve pre-implementation test review, valid cloud RED, frozen tests, same-test GREEN, evidence retention, independent test deletion and final gate. Implementation-readiness guidance cannot defer required test review until after implementation. | Explicit boundary in execution rules and native delegation guidance. |
| Existing diagnostic evidence is sufficient | Reuse it only after checking source/toolchain/task applicability; optional diagnostic enhancement is not inherently a blocker. Earlier logs never replace required current-cycle RED/GREEN or exact-final-head checks. | Semantic review of evidence limits. |
| Several independent reads are known | Request them together when tool contracts allow. Dependent operations, mutations and approval boundaries stay ordered; inspect necessary omitted detail. | Prompt diff review; no tool schema or execution changes. |
| Child or CI result is the only remaining dependency | Use the existing wait/status facilities and respond to meaningful changes, respecting current deadlines, cancellation, CI cadence and user input. Do not create status-only followup tasks or arbitrary hard-stop budgets. | Guidance review; no new persistent-wait protocol or scheduler. |

## Implementation plan and ownership

The dedicated implementation Agent owns only:

1. `.agents/skills/vega-kanban-delivery/SKILL.md`: concise, discoverable handoff/readiness and incremental-review guidance in the existing workflow; retain frontmatter, authorization boundaries and all applicable delivery gates.
2. `docs/vega-exec-guide.md`: a short canonical pointer/clarification for those execution rules, preserving the constitution's existing obligations rather than copying the entire skill again.
3. `crates/vega/src/agent_context.rs`: edit only `SYSTEM_PROMPT` prose for batched independent reads, sufficient evidence and ordered dependent/approval steps.
4. `crates/vega_runtime/src/agent/loop_.rs`: edit only the existing appended subagent-instruction string to cover complete task packets, ready implementation handoffs, evidence-driven followups and delta review with final-head confirmation. Retain delegation restrictions, context/ownership boundaries, execution identities, independent review, cancellation and trust boundaries.

The parent owns this spec, Git/PR state and integration. The implementation Agent produces a brief delivery report outside the repository, reports ready, then stops writes. Only then is a separate read-only reviewer assigned the delivered diff. The reviewer and existing cloud checks can proceed against that same immutable head; new findings allow justified followups and revalidation.

## Verification classification

This is a scoped rule/prompt-copy revision. It changes instructions seen by the model, but no Rust control flow, tool interface, policy decision, storage, provider configuration or scheduler. Review instruction meaning and safety boundaries; do not add source-string or simulated-model tests that cannot demonstrate real task efficiency. Runtime behavioral RED/GREEN/test deletion is N/A for this copy-only scope. This classification does not exempt future deterministic behavior changes from TEST-FIRST; stop and revise the contract before expanding into such changes.

Local work is limited to low-CPU static inspection, formatting, diff and skill-frontmatter validation. No local Vega execution, compilation or tests. The exact final head must pass the unchanged existing cloud `check (fmt, clippy, test)` and independent review. Rust discovery/execution counts must be reported accurately; an empty set is `no Rust tests`, not behavioral proof. Real-model task-quality/efficiency measurement is deferred to #522 under its separately specified experiment boundaries.

## Non-goals and delivery

No workflow/toolchain/dependency changes, new tests or measurement service; no fixed call/review quota, new executor surface, persistent waiting, online recursive optimization or user/global skill/config changes. The separate main checkout's existing dirty files remain untouched. Existing source comments are not expanded; add no new code comments.

After exact-head cloud success and independent approval, squash merge through the existing PR process. Keep the Issue open and move it to In review for user confirmation; no local application build or installation is implied. Rollback reverts this task's rule/prompt changes together. No time-saving percentage or observed improvement is promised by this delivery.

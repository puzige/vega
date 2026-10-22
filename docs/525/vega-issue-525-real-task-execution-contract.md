# Real-task execution and delegation contract

## Authorization and scope

Issue: <https://github.com/puzige/vega/issues/525>. On 2026-10-22 the user approved starting the real-task execution plan, using #514 to reduce repeated delegation, rework and waiting while retaining independent review and CI gates.

This is the next guidance-only revision after PR #526. Baseline: `7f1b001522c8166f83d72881bfccf5efefa35e8b`; branch: `feat/525-real-task-execution-guidance`. The main Agent fetched before creating the isolated worktree. Existing dirty checkouts and the #514 implementation branch are outside this revision's write scope.

The first delivery updates existing instructions and concise handoff templates. It does not implement a runtime state machine, tool validation, scheduler, telemetry service, App installation or a new experiment. Real Vega task results must be reported separately from guidance delivery.

## Evidence and diagnosis

The [#514 PR](https://github.com/puzige/vega/pull/529) records repeated GPUI test-harness API, pointer activation and selection-layer corrections before valid RED. After implementation, review found a list-marker regression and opened an additional test-first cycle. That cycle confirmed the original 16 cases passed while its three new cases failed. The last recorded run executed 19 cases, with 17 passing and two failures: a Markdown stream boundary defect and an invalid headless font observation route.

Eight Actions runs took approximately 3.0–4.65 minutes each, including expected RED and one cancellation. Their accumulated created-to-updated intervals are not a measure of Agent idle time. Complete per-request conversation traces were not obtained; this revision does not claim every Continue was unnecessary or promise a measured speedup.

PR #526 already requires readiness before implementation review, evidence-driven followups and incremental review. This revision makes their input and recovery conditions concrete, without replacing those requirements with a fixed review quota.

## Acceptance cases

| Situation | Required instruction | Verification |
| --- | --- | --- |
| A real task is about to be delegated | Supply the root-cause call chain, original acceptance and affected regressions, actual API and test-observation feasibility, dependencies, permitted writes, non-goals and expected evidence together. Inspect APIs statically first. Where needed, valid positive controls belong to the pre-implementation test patch and its first existing Actions run; do not add a mandatory extra CI stage. | Independent semantic review of the task-packet template and unchanged test-first obligations. |
| Tightly coupled implementation and review will continue | Prefer one implementation writer and a suitable independent read-only reviewer, retaining their context. Implementation review requires an exact ready content version and cessation of writes; earlier plan/test review keeps its own preconditions. | Review ownership and readiness wording in skill and native guidance. |
| An Agent fails or is interrupted | Inspect committed and uncommitted artifacts, preserve a checkpoint and verify the old writer has stopped before replacement. Hand over the same version, evidence and open findings; do not restart completed work or wait forever for an unavailable Agent. | Semantic recovery review; cancellation and tool semantics remain unchanged. |
| Additional implementation or review is needed | Identify the prior and current SHA/content identity when applicable, the precise unresolved issue and the new diff, failure evidence or acceptance change. Uncommitted artifacts use an honest content/diff identity, not a fabricated commit. Status alone uses existing status/wait tools. | Review concise followup and ready-handoff examples; no same-content review without a new question or evidence. |
| CI or review reports a failure | The parent reads the relevant original evidence once and distinguishes product defect, test-harness fault, acceptance/scope change and external CI failure. Correct the responsible layer; keep assertions intact. Acceptance changes reopen the existing test-first cycle. Required regressions cannot be split out merely to obtain GREEN. | Check classification and preservation of original evidence, assertions and current-cycle gates. |
| The same failure repeats twice, or active Agent work has produced no new artifact/evidence for 20 minutes | Trigger parent diagnosis and a concrete next verification step before blindly adding work. These are diagnostic triggers, not task/model/review quotas, automatic termination or automatic Agent replacement. Explicit CI, user or external-dependency waits continue under existing wait/cancellation rules. | Ensure the trigger does not create status-only work, polling, a new scheduler or an escape from quality gates. |
| Several same-stage edits are ready for publication | Consolidate related small fixes and documentation into one push where practical. Preserve independent RED, GREEN and deletion commits/evidence; do not mix new production behavior into post-GREEN deletion. | Review phase boundaries and prevention of avoidable push cancellation. |
| An immutable implementation version is ready | Independent implementation review and existing Actions can progress on the same version concurrently. Neither replaces the other. Changes invalidate affected conclusions; exact final-head independent confirmation and required checks remain mandatory. | Review same-version conditions, incremental-review limits and final-head gates. |
| Results are the only remaining dependency | Use existing waits/status facilities; CI cadence remains every five minutes except completion/failure notifications or immediate user requests. Do independent review/evidence preparation when available, without creating empty tasks. | Preserve deadlines, cancellation and meaningful user input. |
| A status or efficiency result is reported | Bind stage, exact version, evidence, unresolved items and next action. Ready, reviewed, GREEN, merged and user-accepted remain distinct. Actual running App build/prompt and loaded-skill versions must be checked before attributing real-task effects. | Semantic status review; no claims of installation, runtime enforcement or measured improvement. |

## Implementation plan and ownership

The parent owns this specification, baseline, Git/index, PR/CI state and integration. A dedicated implementation Agent is the sole writer of these four surfaces:

1. `.agents/skills/vega-kanban-delivery/SKILL.md`: replace repetitive coordination prose with concise preparation, followup, ready-result and failure-classification templates; retain all authorization, lifecycle, ownership and final-head gates.
2. `docs/vega-exec-guide.md`: keep a short canonical pointer to the operational contract; preserve the constitution and test-first lifecycle rather than duplicating the skill.
3. `crates/vega/src/agent_context.rs`: only existing `SYSTEM_PROMPT` prose, if needed, for complete task preparation, valid evidence and distinct work/wait decisions. Preserve literal representation and surrounding Rust exactly.
4. `crates/vega_runtime/src/agent/loop_.rs`: only the existing appended delegation-guidance string, with concise versioned followups, retained ownership, exception recovery and failure diagnosis. Generic guidance remains applicable to non-Git tasks; do not impose repository-only identifiers on unrelated work.

No files outside these surfaces and this specification are required. The implementer reports READY with precise content hashes, changed scope, static checks, unresolved findings and cessation of writes. The independent non-implementer reviews the delivered immutable content. The parent commits the reviewed content and submits one existing PR check; independent exact-head confirmation and cloud checks may run concurrently.

## Verification classification and preserved gates

This changes model-visible guidance, not deterministic Rust execution behavior. Runtime RED/GREEN/test deletion is N/A for this copy-only revision. Do not add source-string or simulated-model tests that cannot prove task efficiency. Any control-flow, tool/schema, trust, cancellation, storage or permission change requires a revised contract and the existing test-first process before implementation.

Local validation is limited to quick diff, formatting, literal-boundary, skill-frontmatter and other static checks. No local Vega execution, compilation, builds, packaging or tests. Confirm that Rust differences are confined to the two approved string literals and that CI files, dependencies, tools and user configuration are unchanged.

The exact final head must pass independent review and the unchanged cloud `check (fmt, clippy, test)`, including existing fmt, all-target Clippy, Python and Nextest obligations. The current Rust lifecycle for behavior changes remains: valid RED, frozen tests, same-test GREEN, durable source/evidence, independent test deletion and final-head full gate. A zero-test final tree is `no Rust tests` (zero discovered/executed), not proof of behavior or speedup.

## Delivery and later validation

After independent approval and exact-head cloud success, squash merge and return #525 to In review, leaving it open for user confirmation. Preserve prior evidence and unknown dirty worktrees. The active primary checkout/global skill link may be fast-forwarded only when unrelated user changes can be preserved; native system guidance requires a running build that contains the merged revision. Do not claim code merge means App installation.

Later real-task validation must use actual Vega conversations, real task branches, native delegation and existing Actions. Record task complexity, actual version/model, stage durations, delegation and rework reasons, CI queue/execution versus reaction delay, outcomes and blocked/failed samples from existing evidence. #514 history and work performed by Codex are diagnostic inputs, not Vega speedup measurements. There is no new automatic 48-trial run or measurement service, no fixed saving percentage, and no scope expansion to repair #514 in this guidance PR.

Rollback reverts this revision's four guidance surfaces together while preserving prior task and verification evidence.

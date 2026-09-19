# Issue 60 · Non-blocking pricing

Frozen 2026-09-19, Asia/Shanghai. Source: GitHub #60, Ready → In progress.
User requirement: a configured model without a price must be usable for chat.
This supersedes only S7 C1/T37, A7-01 and model-selection rules that require
pricing before selection/start. Pricing editing safety and cost correctness
remain authoritative. No security or provider-readiness exception is authorized.

## Contract

R1. Model menu membership follows the existing configured, uniquely resolvable
provider/model catalog, not the pricing catalog. A priced but unconfigured model
does not become usable. Selecting an unpriced configured model works on both
draft and persisted routes; model identity persists as before. Keep exact IDs,
active-run/trusted-action/route ownership fences and configuration validation.

R2. No first-message, existing-conversation or approved-plan start is rejected
merely because pricing is missing, loading, saving, reloading, invalid, or has no
exact model. Do not redirect to Pricing Settings or erase/reject the composer
for that reason. Keep provider uniqueness, credentials, API/quota errors,
permission modes and tool approval behavior unchanged. First submit still
materializes the same draft ID exactly once, only after non-price readiness.

R3. Freeze optional pricing once at run start. Ready uses its committed authority
(never an unsaved/conflict draft). Saving may use its `previous` committed
authority. Loading, Reloading and Invalid use no catalog. An authority lacking
the exact model yields no pricing selection for that run. Pass the same optional
snapshot to runtime and the estimator; no synchronous UI file reads, no live
price reread or model/provider substitution. New prices affect later runs only.

R4. Preserve actual usage when supplied. Unpriced cost is unknown, shown as `—`,
not free, guessed or estimated from a different model. Reuse the current legacy
storage representation (zero placeholder plus NULL pricing provenance) only
with its existing Unavailable aggregate semantics; no migration is needed.
Mixed known/unknown history keeps total cost unknown; no historical repricing.
Priced runs retain exact integer pricing and immutable run-start semantics.
#65 automatic title requests use the same optional pricing and still work.

R5. Pricing Settings remains available: validation, error states, atomic writes,
byte-preserving malformed-file handling, explicit recovery and dirty-draft
ownership are unchanged. Price errors must not turn into chat errors or silently
rewrite user pricing. Do not disable the feature or synthesize a dummy price.

## Test cases before implementation

Owned temp stores/config and actual app/controller entry points are primary.
MockProvider may replace network, not route/preflight/persistence behavior.

| ID | Preconditions/action | Expected observations | Evidence |
| --- | --- | --- | --- |
| P1 | Configured exact unpriced model; open menu and select on draft/persisted routes | Visible/selectable, identity saved/restored, no Pricing redirect | App GPUI/controller |
| P2 | New draft on unpriced model; submit once | Same ID materialized once, provider called, body retained, no settings takeover | App worker + owned DB |
| P3 | Persisted task / approved plan without price | Run proceeds; original tool permission/plan approval remains | Production start chain |
| P4 | Loading/Reloading/Invalid pricing | Chat proceeds unpriced; pricing error stays in Settings; unsafe file bytes unchanged | Controller/real service |
| P5 | Saving with previous authority; Ready with dirty draft | Only previous/committed matching price used; unsaved values never used | Frozen run evidence |
| P6 | Unpriced usage streaming/completed/reopened | Actual token counts; unknown cost and NULL provenance, no free claim | Meter + store + UI summary |
| P7 | Add valid price after unpriced run, send next turn | New turn priced; old rows unchanged; mixed sum unknown | Production controller/store |
| P8 | Invalid/ambiguous provider, missing key, active run, permission denial, quota failure | Existing non-price protections unchanged | Existing regressions + focused assertions |
| P9 | First unpriced turn with automatic naming | Body and title path work; no false pricing provenance | Production #65 regression |
| N1 | Installed app UI chooses usable configured unpriced model and submits harmless prompt | Real response without editing local files or forced Settings | Native screenshot |
| N2 | Reopen/restart test conversation and inspect usage | Model and body retained, no fabricated cost | Native screenshot + production coverage |

Add a focused P1/P2 regression and run it against current production code first;
retain the expected failure. Then implement. Native access to a usable unpriced
model must be established through UI; if unavailable, report the specific
limitation rather than modifying files or counting a mock as live acceptance.

## Implementation plan / ownership

1. Executor owns minimal production changes in app_agent.rs, window/agent.rs,
   window/session.rs and directly needed app tests. Reuse optional runtime/meter
   paths; no new public API outside necessary app-private projection.
2. Remove only pricing selection/start gates; keep all readiness/ownership gates.
   Optional catalog snapshot replaces the old mandatory capability handoff.
3. Explicitly authorize updating obsolete unpriced-refusal expectations in
   tests/pricing.rs, tests/r69.rs and tests/model_selection.rs to the new R1–R5
   behavior. Preserve unrelated body, identity, cardinality, permission, settings
   recovery, atomic write and provider-readiness assertions. Any other conflicting
   old assertion requires a reasoned report before changing it.
4. Executor runs focused production regressions then fmt/strict app clippy and
   returns code ownership with docs/vega-issue-60-delivery.md and raw log references.
5. Main reviews, runs full workspace + ignored gates, builds/packages and installs
   after backup, validates UI, integrates locally, cleans exact task worktree and
   branch, updates Issue/Project, then takes the next Ready card.

No new dependencies, schema changes, provider configuration writes, remote push,
other issue implementation or changes to user's files. Main owns this spec and
handoff. All cargo commands use scripts/cargo-lock.sh with exclusive ownership.

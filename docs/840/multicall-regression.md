# Native multi-call regression contract

## Scope and evidence

Issue #840, roadmap item 2 (P1). Baseline: e6a7a0e59cb4db63c19ff3ab6dfc7b52a562e8ff.
The historical CPA audit found 53 single-call upstream responses with parallel calls enabled, and earlier multi-call responses delivered intact. This is not evidence of a dropped-call defect. #839 changes planning instructions; its real-model effectiveness remains user-led and is not a prerequisite for deterministic regression checks.

Initially only development regression modules and this specification were planned. The first cloud run and independent review exposed a pre-existing SSE identity defect: a new nonempty ID at an already-bound index was silently ignored. The user explicitly approved including the minimal production repair: remove the by_index exception from the existing ID-conflict predicate in SseAssembler::absorb_tool_call. Empty/omitted IDs, first ID binding, matching repeated IDs and argument accumulation must remain supported. No new production API, scheduler, provider flag, external service request, dependency, or persistent observability system.

## Acceptance before implementation

| Case | Action and observable result | Boundary / evidence |
|---|---|---|
| Interleaved SSE | Feed multiple indices, repeated tool names and fragmented arguments through the production assembler; exact ordered IDs, names and parsed arguments survive | In-process deterministic provider bytes |
| Invalid SSE | Missing identity, malformed arguments, conflicting identities and duplicate IDs at distinct indices fail closed | Existing typed protocol errors; no fabricated successful calls |
| Complete batch | A scripted Provider emits four calls, including two reads with different parameters; the real runtime executes them and sends exactly one subsequent request containing every call/result | Provider mock only; actual filesystem tools in owned temporary directory |
| Partial failure / refusal | Mix successful read, missing file and a refused write; preserve distinct results and leave refused target absent | Real runtime permissions and tool events |
| Persistence | Feed real runtime events into production persist_runtime_event against isolated Store; assert terminal status, exact tool/input identity and exact round-member IDs match next-request calls; stored output remains intact in the model result, with only the explicitly allowed runtime progress notice suffix | Production event adapter, not a handwritten persistence substitute |
| Recovery | Supply previously completed call identities/results and repeat the batch against changed but still valid read/write targets; reuse original outputs without executing the tools again | Side-effect target remains unchanged and execution count agrees |
| Cancellation | Cancel after first terminal event; retain its terminal fact and do not start remaining calls or another provider request | Real cancellation token and sink |
| Question isolation | A question mixed with ordinary calls does not execute the ordinary call or invoke the question hook | Real runtime mixed-batch gate |
| Skill policy | Skill-only and mixed batches preserve the production classifier's distinct policy | Unit boundary; not claimed as full skill activation integration |
| Stale file | Change an already-read target before a subsequent mutation batch; mutation remains refused and replacement bytes unchanged | Real read identity / mutation preparation |

Permission-reset coverage combines the production pause_admission entry, real admission ownership/drain and runtime cancellation between calls. It checks that new owners are blocked, the remaining mutation never starts, and admission reopens only after the guard drops. This is a headless boundary test, not the GPUI reset controller or full configuration-reset transaction; those remain explicitly unexercised. Any unexercised matrix boundary is recorded explicitly, not silently dropped or described as passed.

## Minimal implementation plan

1. Add development-only SSE tests adjacent to the private assembler, without widening visibility.
2. Add development-only conversation tests that use the existing Provider trait, runtime entry, file Tools, Store and persist_runtime_event. Mock only provider output; inspect the next request and stored outcomes.
3. Use existing Actions pr-check for all compilation and execution. Local work is limited to rustfmt and static diff checks. Preserve first failures and repair the responsible layer, never weaken expectations to get green.
4. Preserve accessible test source, exact commit and actual Nextest results in the Issue/PR. After successful development verification remove all added Rust tests and exclusive wiring. Final delivery retains this contract/evidence and must pass required CI with no Rust tests.
5. Freeze exact versions for required independent non-writer review. Reuse unchanged review conclusions for cleanup delta. Merge only the final checked head; keep Issue open/In review for user acceptance.

## Non-goals and validation limits

No concurrency (#841), asynchronous shell (#412), batch tool/code mode (#842), or model A/B traffic. Tests do not establish CPA compatibility for every provider, real-model batching propensity or latency improvements. No local Vega execution/build/test and no production database/configuration changes.

## Current validation

First development head: de105e6919c18df666cbc0a387a4515396f0411f. Actions run https://github.com/puzige/vega/actions/runs/41327025202 completed: fmt/Clippy passed; Nextest 11 run / 7 passed / 4 failed / 0 skipped. Original source remains accessible at that commit. Failures: production identity conflict returned Ok; reset fixture listened for a stage-only event; persistence expected equality despite legitimate model-only guidance; recovery removed the read target before preparation. The latter three are harness/expectation defects, not established product defects. Independent review also requires exact persisted input and round-member assertions. Retain all original safety expectations while correcting the responsible layers. User approved the identity repair before production changes.

Pending: corrected development head cloud regressions, incremental independent review, cleanup and final required CI. No final success is claimed.

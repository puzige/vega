# Issue 268 / Issue 60 / R69 · Streaming fixture observation gate

2026-10-06, Asia/Shanghai. Main-agent review approved F1–F5 and the conditional
D1 → G1/G2 sequence before implementation, with two required G3/G4 helper checks.
This is a narrow acceptance-fixture correction under
[Issue 60 R2–R4 / P2 / P6](vega-issue-60-unpriced-chat.md) and
[R69 stable draft / first-submit contracts](vega-r69-home-lazy-draft-composer.md).
Its independent delivery card is [Issue 268](https://github.com/puzige/vega/issues/268).
The current [Issue 149 mock boundary](vega-issue-149-remove-real-e2e.md) applies.

## Observed facts and unconfirmed history

- PR #264 head `a826a1bb0dd5897c4a93996547790774b140f382` failed its first cloud
  Nextest run `473bba18-f5bc-4bef-b83f-31697c78bbcc`: 1,976 passed / 1 failed /
  5 skipped, exit 100. The sole failure was
  `tests::r69::issue60_unpriced_first_submit_reaches_provider`, after 13.020s.
- Its failure at `r69.rs:105` waited for `meter_snapshot().provisional`. The
  generic pump panic says terminal state, but the actual predicate is a
  streaming observation. The later worker-terminal predicate was not reached.
- The fixture emits TextDelta, a real Tokio 150ms replay delay, then Usage/Done.
  The application drains up to 128 channel events into one UI update. Usage and
  terminal events clear the provisional meter. The submit helper itself pumps
  the application before the outer provisional predicate runs.
- Source comparison with `dfcd65a` found the test, mock, meter and lifecycle
  unchanged. This provides a possible observation loss mechanism; the original
  cloud log does not contain the owned worker/meter/database snapshots needed
  to determine whether it happened in that run. Its historical cause remains
  unconfirmed. The original log is retained privately, SHA-256
  `a371e0581e0c583413cbd549c15f97d14a65f95ab02e60b79317523b5665ab45`.

## Frozen fixture contract

F1. Change only this R69 acceptance test and a helper local to its test module.
The production route, MockProvider recording, owned config/database and existing
owned Git fixture remain the same. Do not alter pricing, provider readiness,
permissions, Store, runtime public API, worker lifecycle, global pump, deadlines,
CI, retries, ignored tests or dependencies. Add no code comments.

F2. Before the final helper, retain a temporary diagnostic patch that sets only
the existing 150ms replay delay to zero and adds bounded owned-state observations.
Run only the exact R69 test filter once. Capture worker-start count, active-run
presence, meter tokens/cost/provisional, primary request count and assistant
status. Preserve the patch, frozen source hash, raw log, run ID and exit code.
If it does not reproduce observation loss, report that result to the main agent
before selecting another approach. A controlled red is evidence of the fixture
mechanism, not confirmation of the original cloud cause or a product defect.

F3. Following a matching controlled red, wrap the existing primary MockProvider
with a fixture-local provider gate, using the existing ConcurrentProvider
pattern. Deliver its first non-empty TextDelta normally. Before polling the next
provider event, wait on an explicit release token or the run cancellation token.
No replay timer is used in the final fixture. No zero-duration delay remains as
a permanent default. Auxiliary title requests retain the existing separate
fixture wrapper.

F4. Release the gate only after the real app/stream has satisfied all original
streaming assertions: provisional meter, active agent and unknown cost. Then
perform the unchanged completion, identity, model, request-count, transcript,
actual token, NULL pricing provenance, aggregate and reopen-summary assertions.
The existing provisional pump predicate must also require the provider's gate
waiting token: UI visibility can precede the worker polling its next event.
This additional fixture readiness condition uses the same pump and deadline.
The gate controls provider timing only; it must not synthesize UI state,
persistence, events, approvals or successful outcomes.

F5. A test-owned release guard releases the gate on scope exit, including an
assertion panic. Run cancellation wakes the waiting stream with its normal
cancelled outcome. These paths must not leave a worker waiting on a token whose
only owner was dropped. Use existing cancellation primitives; do not extend a
deadline or install an automatic retry. Two task-specific, pure in-process helper
checks must prove release-guard drop wakes an already waiting gate, and run
cancellation wakes it with Cancelled without delivering Usage/Done. Keep any
diagnostic output bounded and limited to fixture-owned scalar state, never
credentials, bodies or paths.

## Frozen acceptance matrix and observed results

| ID | Requirement / risk | Preconditions | Operation | Expected observation | Layer | Evidence | Status |
| --- | --- | --- | --- | --- | --- | --- | --- |
| H1 | Historical cloud failure | Original PR/run/log | Read exact failing predicate and lifecycle | Historical failure retained; cause remains unconfirmed | Source / cloud log | Original SHA and run above | RECORDED |
| D1 | Lost streaming observation | Owned existing fixture with temporary zero-delay patch | Exact original R69 test | Controlled red: first and all later bounded observations were completed/usage-calibrated with provisional false | App / mock provider / owned SQLite | `b71c6408-810f-4db8-b881-5f7506b5d503`, 0 passed / 1 failed / 216 skipped, exit 100 | REPRODUCED (CONTROLLED ONLY) |
| G1 | Stable streaming checkpoint | Same fixture with explicit provider gate | Submit through actual Composer and observe stream before release | Provisional true, active agent true, cost None; no Usage/Done may cross the provider gate yet | App / gated MockProvider | `ac7d1bc9-3371-42c9-ab32-64b79e076708`, exact original R69 case | PASS on dbe90d0 |
| G2 | Release and durable completion | G1 observed and gate explicitly released | Finish original test, including reopening owned Store | All original request/model/identity/content/token/cost/reopen assertions remain exact | Production worker / owned SQLite | Same final run, exact original R69 case | PASS on dbe90d0 |
| G3 | Failure cleanup | Pure in-process MockProvider paused at the gate | Poll until waiting, then drop its test-owned release guard | Wake probe fires; waiting next-event future receives the original Usage/Done | Fixture helper lifecycle | Same final run, exact release-guard helper case | PASS on dbe90d0 |
| G4 | Run cancellation | Pure in-process MockProvider paused at the gate | Poll until waiting, then cancel the run token | Wake probe fires; waiting future yields Cancelled and ends without Usage/Done | Fixture helper lifecycle | Same final run, exact cancellation helper case | PASS on dbe90d0 |
| M1 | Fresh integration baseline | Main reported new master `dbe90d06df99dd4211c2bedb46c8e5050cc4c88e` after the first source freeze | Commit clean, fetch/rebase, then freeze and rerun only the same three cases | All three remain exact and pass on latest master | Integration baseline / focused mock acceptance | Separate final run and hashes below | PASS (3 / 3) |

## Implementation and verification plan

1. Main agent reviews this spec before code or diagnostic changes.
2. Work in `codex/60-streaming-fixture-gate`, fetched/rebased on
   `fac20c6e85ff93488eac0d3215d9913dee7417f5`. The new worktree uses its own default
   `target`; it does not share the S16 or S21 build directory. Keep the submitted
   S16 worktree and PR unchanged.
3. Freeze the temporary diagnostic source, run the exact test below with zero
   Nextest retries, retain red evidence and restore the temporary diagnostic
   changes. If D1 is not reproduced or contradicts the planned gate, report.
4. Implement only F3–F5, retain every original assertion and run the exact R69
   case plus the two required helper cases. Run format/diff checks, record final
   source/tree and log hashes, and update this matrix with observed results and
   remaining limits.
5. Main reviews the final diff and owns the independent PR/cloud checks/merge.
   No executor push, installation, native operation or user database access.
   At most three local commits. A later S16 rebase gets separate validation.

```sh
cargo nextest run -p vega --retries 0 --success-output final --failure-output final --target-dir target -E 'test(/^tests::r69::issue60_unpriced_first_submit_reaches_provider$/)'
```

The final focused filter is:

```sh
cargo nextest run -p vega --retries 0 --success-output final --failure-output final --target-dir target -E 'test(/^tests::r69::issue60_unpriced_first_submit_reaches_provider$/) | test(/^tests::r69::issue60_streaming_gate_release_guard_drop_unblocks_stream$/) | test(/^tests::r69::issue60_streaming_gate_run_cancel_stops_before_usage$/)'
```

Only this task filter runs locally. Workspace checks remain cloud-owned. Neither
controlled stress nor a gated mock verifies real provider/network/native behavior.

## Retained controlled diagnostic

The single D1 run used a temporary `150ms → Duration::ZERO` patch; the bounded
fixture snapshots at polls 0, 1, 10, 100 and 399 all reported:

```text
worker_starts=1 active_runs=0 stream_active=false provisional=false
tokens=21 cost=None request_count=1 assistant_status=done usage_rows=1
```

The unchanged provisional predicate failed. This proves observation loss can
occur in the controlled fixture. Batch composition was not measured and the
historical cloud cause remains unconfirmed. No product defect is inferred.
The temporary patch was retained privately and then restored exactly before
the final gate implementation; neither zero delay nor diagnostic output remains
in the final source.

Private evidence is under
`/Users/puzige/Workspace/vega-evidence/issue60-streaming-fixture-gate-2026-10-06-j9_q9u6n`:

- `freeze-stress.json`: tested tree
  `a1146accf8323c3518fd8ae50c508281f431c8c7`, R69 source SHA-256
  `49f37028ccb78db836427ea0f00756c22d223c7259a6b8c5789413a5f4040b2f`.
- `stress-diagnostic.patch`: SHA-256
  `401232d7b8cb4d30e38f45cc07cd0c1af726dd13afcfee0c3900ab16d1eb8ae7`.
- `nextest-stress-first.log`: raw Nextest run
  `b71c6408-810f-4db8-b881-5f7506b5d503`, 0 passed / 1 failed / 216 skipped,
  exit 100, log SHA-256
  `d2d25b5c1f46d6311fbc9bc3d3cf644e79673ecca9f85f5154ec119a0f3b7661`.
- `result-stress-first.json`: raw exit and frozen-source equality checks.

## First gated verification on the original base

`freeze-gate-first.json` records head
`fac20c6e85ff93488eac0d3215d9913dee7417f5`, tested tree
`7e515302d1df3cb59f58da879f9eb1a51f417cbc` and R69 source SHA-256
`bf2ad6a8c0a6d5f053ad8ba22f773c58da9d395d45f9c3905536590a418f7dd3`.

`nextest-gate-first.log` retains run `50e26b9d-0b5b-4771-b65d-9c84c8447763`:
3 passed / 216 skipped, exit 0, zero retries. Its SHA-256 is
`5f70544b4f0a8539f9f46bfe915fef59ee5bafa5813464e71a5b5b88bc29da3c`.
The source stayed unchanged throughout this run. `assertion-preservation.json`
records a byte comparison of the entire original R69 case after only the four
exact gate substitutions listed in F3/F4; all original acceptance assertions,
durable checks and the production submit route remain unchanged.

The two helper tests poll an actual wrapped MockProvider next-event future to
Pending and confirm its waiting token before exercising cleanup. Their owned
ArcWake probe confirms the registered pending task was woken; merely polling
again after a token changed would not establish that. No timers, Tokio runtime,
GPUI/global pump, child process or filesystem fixture is added to those checks.

The first green above remains evidence for its original baseline. M1 is a
separate fresh-base verification; its integration result does not overwrite the
first green or the controlled red.

## Fresh baseline and gate readiness refinement

Main-agent source review after the first green identified an additional fixture
ordering risk: a provisional UI event can be visible before the worker has
polled its next event and set the gate's waiting token. The final source retains
the original provisional condition and adds `waiting.is_cancelled()` to that
same existing pump predicate. It does not add a new pump, timer or deadline.
Active-agent and unknown-cost assertions, explicit release and every durable
assertion remain unchanged. The final assertion-preservation record treats this
readiness conjunction as a fifth precise fixture change.

The first tested implementation was committed cleanly as
`caac3bb7d0436f123ee990acf4c7875563f78224` before another `fetch --prune origin`
and rebase onto `dbe90d06df99dd4211c2bedb46c8e5050cc4c88e`, producing
`25689e61724f116032a3117090f4e0acecaa037f`. The R69 source was unchanged by
that rebase; its old SHA remains the first-green source recorded above. The
readiness refinement is applied after this clean rebase, before the final
source freeze and three-case run. `first-green-commit.patch`, `rebase-latest.log`
and `rebase-latest.json` retain the prior state and successful fetch/rebase.

## Final focused verification

`freeze-final.json` records the fresh base
`dbe90d06df99dd4211c2bedb46c8e5050cc4c88e`, pre-run head
`25689e61724f116032a3117090f4e0acecaa037f`, tested tree
`082d3862d17b2f33a29c6896832d223c93b058a0` and final R69 source SHA-256
`d8537ba0b6f774e34d51c53217288d19527a1a9d8bee6d76f9828ddb213610a9`.

`nextest-final.log` retains run `ac7d1bc9-3371-42c9-ab32-64b79e076708`:
3 passed / 216 skipped, exit 0, zero retries. Its SHA-256 is
`02ff14346caa63b4d134e0c4763caff7929b805c6a9d263d7eeffeda2a27cdb8`.
`result-final.json` confirms both frozen files stayed unchanged during that run.
Only this document's result recording follows the test freeze; the final Rust
source remains exactly the tested source. `assertion-preservation-final.json`
proves the current master original case matches the initial case and preserves
the whole body after the five specific gate substitutions.

Local scope is complete for G1–G4/M1. Full workspace checks, Clippy and the
independent PR/cloud gate remain main-agent integration work. No product source,
provider/public runtime API, pricing, permission, Store, worker lifecycle,
global pump/deadline, CI, dependency or ignored-test set was changed. The old
S16 worktree remains clean and unchanged; its later fresh-base validation is
separate. Native behavior and real provider/network execution were not exercised.

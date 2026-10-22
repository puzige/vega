# Native execution bounds: test-first evidence

Related: #556, [PR #561](https://github.com/puzige/vega/pull/561). Production baseline: `871f91f246e90753743a256cee1eec315e74effc`.

## Acceptance v1 and independent test review

The 19 cases in `crates/vega_conversation/src/agent/subagents/execution_bounds_tests.rs` exercise native Supervisor/Hook, child entry/pipeline, persistence and parent steering. Scripted provider/host and controlled time isolate external dependencies. They do not constitute real-provider, native UI or speed trials.

The non-writer reviewer `review_runtime_efficiency` reviewed the test-only version. A finite-deadline fixture was corrected by pausing time before its first poll; expectations were unchanged. First reviewed source: `22cc23299c8d98e992f58570bd1b8c3d437e6eb6`. The first CI exposed another fixture mistake: `Store::open()` does not migrate a database. Correction `4fe0a8ad179a44fbd912b413e784cd257e9ea207` calls the existing `Store::migrate()` on the temporary database, matching production initialization. The reviewer checked this exact correction before the second run. Neither correction changed production, acceptance targets or assertions.

v1 test-only baseline: `4fe0a8ad179a44fbd912b413e784cd257e9ea207`.
v1 source SHA256: `ffb9275f22f2551dd2d0e29363718a763b281194c112efeac2b3fbdab1bc1fc6`.

## Actions RED

| Run / exact head | Observed outcome | Meaning |
| --- | --- | --- |
| [39819888698](https://github.com/puzige/vega/actions/runs/39819888698) / `22cc23299c8d98e992f58570bd1b8c3d437e6eb6` | Clippy/compilation and Python passed; 19 Rust executed, 0 passed, 19 failed, 0 skipped | Three request-cap assertions reached behavioral RED; sixteen failed during fixture initialization with `no such table: projects`. Those sixteen are not behavioral evidence. |
| [39821042539](https://github.com/puzige/vega/actions/runs/39821042539) / `4fe0a8ad179a44fbd912b413e784cd257e9ea207` | Clippy/compilation and all 60 Python tests passed; 19 Rust executed, 4 passed, 15 failed, 0 skipped | Valid behavioral RED. Target failures cover timeout wakes, absent active budget, release safety, lost partial output, old history propagation, request limits, automatic child summaries and attention/steering wakeups. |

The four compatibility controls already passed: explicit zero/finite waits, cancellation/parent-end wait behavior, completed stages not retroactively cancelled during host release, and continuation source/authority/state guards. Cancellation, release and continuation guards must remain GREEN. The v1 repeated finite/zero compatibility expectation is explicitly superseded by W3 v2 below; its former GREEN is not evidence for the new semantics.

Second-run raw assertions include `active stage must be bounded`, `default wait must not wake the model after 30 or 60 seconds`, failed persistence/release preserving occupation, unexpected old progress in followup history, two provider calls where oversized preparation permits only the first stage's call, and child compaction events where zero are required. These failures occurred in compiled behavioral tests, not a missing API or runner failure. No ignored tests or test retries were used.

The first run took 3m54s from creation to terminal status; the corrected run took 4m17s. These are separate CI wall intervals and include setup/build. The corrected run completed at 17:19:46 UTC on 2026-10-22 and was first observed complete at 17:19:59 UTC. The migration mistake and one failed TLS push are retained as development rework; they are not hidden by restarting the clock. The successful retry was a Git transport retry, not a CI rerun.

## Acceptance v2 correction before resumed implementation

A targeted read of the 215 archived wait tool inputs established that 214 supplied `timeout_ms=60000` and one supplied `timeout_ms=1000`. The first design changed only omitted timeouts, so it would not suppress the observed pattern. Production writing was stopped; the small incomplete diff was preserved outside the worktree and the production files restored to the reviewed test-only head. No completed implementation or valid v2 RED is claimed by that intermediate diff.

W3 is deliberately revised: each exact child execution gets one explicitly bounded observation, shared by positive and zero timeouts. Repeated explicit waits coalesce into event waiting. W6/W7 additionally cover atomic claims, target-set changes and old terminal results in mixed waits. This changes repeated-call compatibility rather than silently changing an assertion to fit implementation. The original elapsed time and v1 work remain in the ledger.

The implementer must first revise/add only wait acceptance tests, including a native parent-provider replay of the explicit 60000-ms pattern. The independent reviewer must inspect the changed contract and test-only version. Existing PR Actions must compile and establish new behavioral RED before production work resumes. Other v1 expectations remain frozen.

The initial 24-case v2 candidate, `22d7c43ca6fd143bf2106e686bbbaa4909a1a085`, was independently reviewed before push and was not frozen. Review found that a fixed 30-ms yield did not establish actual wait-future admission, one permission test could poll a completed future twice, and cancellation needed a direct first-finite-wait case. The same implementer corrected those observations at `0fe2d95a8d9fc7567a2b99dfc347bda2db262200`; the independent reviewer approved that wait-test delta without changing production or weakening assertions. The explicit-timeout replay uses the real RuntimeLoop and Supervisor with a transparent Pending observer; child stages still enter the native pipeline. The parent receipt-ack sink is a fixture boundary, while the separate native-entry steering cases verify real entry wiring.

## Acceptance v3: remove unvalidated byte quotas

Before production resumed, the user asked why entry/request text should be limited to 64/256KiB. Those values were heuristics, not a measured safe or optimal limit. Acceptance C3/C4 is therefore revised before implementation: deterministic history projection remains, model-policy token admission and output reserve remain, but no new byte quota is introduced. Existing unknown-policy behavior and estimator safety checks remain; no verified-capacity claim is made for unknown or assumed model policies.

The previous direct ChildProvider byte-cap assertions and the 64KiB rejection case must be explicitly replaced under this revised acceptance, not silently weakened to make production pass. Revised cases must allow valid large input, reject input over a configured model budget before dispatch, preserve all user constraints/attachments and retain ordinary-parent compaction behavior. Wait, active-budget, release and source/authority expectations remain unchanged. Independent review and new Actions RED on unchanged production are still required before resuming production.

The independent reviewer approved the v3 context design before test revision. Existing budget-accounting and valid-large-input controls may already pass on the baseline; they are not claimed as new behavior. The new continuation projection and delegated-only disabling of automatic summaries still require behavioral RED.

The revised 25-case source is committed at `5518f748` with SHA256 `48812d61121001ba24852d035b591e9334cebebdcce18d3912d1a309908d31cd`. Five compatibility cases cover all provider entry methods, complete native input/schema accounting, tool JSON/results, large followup user constraints and images. None is claimed as new behavior where it already passes.

Before v3 RED, the branch incorporated master `6615748f91273bfb8ee424011b3b5a09b3777d2b` at merge `58c03afc2fc15ebeabb961dbfd16947a9b3248bd`. This includes #557's estimator v3 and initial-input compaction correction. Tests use the existing estimator and real historical messages for the ordinary-parent compaction control; they do not restore the old image byte surcharge or depend on a futile first-input summary. Production changes for #556 remain absent. Independent review of this revised test version is pending.

## Remaining gates

Production implementation initially began after corrected v1 behavioral RED, then stopped for the v2 acceptance correction. New v3 RED and a newly frozen test set are required before it resumes. Same-test GREEN, independent implementation review, complete source/patch archive in the PR, separate test/support deletion, cleanup review and final exact-head full CI are pending. No delivery or actual speed improvement is claimed by this RED evidence.

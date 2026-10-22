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

Before v3 RED, the branch incorporated master `6615748f91273bfb8ee424011b3b5a09b3777d2b` at merge `58c03afc2fc15ebeabb961dbfd16947a9b3248bd`. This includes #557's estimator v3 and initial-input compaction correction. Tests use the existing estimator and real historical messages for the ordinary-parent compaction control; they do not restore the old image byte surcharge or depend on a futile first-input summary. Production changes for #556 remain absent. Independent review approved exact head `fc0b031962a0d88aa6ed696fe176e8d087489919` before this RED run.

## Remaining gates

Production implementation initially began after corrected v1 behavioral RED, then stopped for the v2 acceptance correction. New v3 RED and a newly frozen test set are required before it resumes. Same-test GREEN, independent implementation review, complete source/patch archive in the PR, separate test/support deletion, cleanup review and final exact-head full CI are pending. No delivery or actual speed improvement is claimed by this RED evidence.

## V3 RED and isolated fixture correction

[Actions 39829079034](https://github.com/puzige/vega/actions/runs/39829079034), exact head `fc0b031962a0d88aa6ed696fe176e8d087489919`, compiled and executed all 25 tests: 6 passed, 19 failed, 0 skipped. Clippy and all 60 Python tests passed. The run lasted 18:10:50–18:16:07 UTC (317,000 ms); its terminal result was first observed at 18:21:06 (299,000 ms later). These are separate execution and observation intervals, not both CPU time.

Eighteen failures concern missing target contracts (some stop at new receipt fields before reaching later assertions). One compatibility control unexpectedly failed before its second provider request. The full suite therefore did not authorize resumed production.

The isolated `native_current_stage_token_admission_counts_json_arguments_and_real_tool_results` fixture used the same root for its database and project. The pipeline puts checkpoints beside the database; the existing checkpoint protection correctly rejects a checkpoint directory inside the project. The correction separates workspace from database/checkpoints inside the owned temporary directory. It also checks live model arguments through the existing public `project_model_messages` projection, while requiring persistent audit arguments to remain content-free. The original audit-field assertion was an incorrect oracle for live wire content.

Its single Write payload is now 128,000 bytes, below the existing 256KiB per-tool JSON adapter guard. This existing individual-argument guard is unrelated to the withdrawn proposed total-request quota. Valid total input above 256KiB remains covered by the system-input control. Actual successful Write contents, Read results, completion state, budget accounting and blocked provider call counts remain required. No production change or other test change is included. Independent delta review caught a further oracle mistake before push: captured `ChatRequest.messages` is already projected wire content, so it cannot be asserted to be audit-only or converted back to audit by clearing live bindings. Those two newly added assertions were removed; database audit assertions and the existing budget/provider assertions remain. The corrected source SHA256 is `c5d67018505df15e4e85aaa52ef5228f91f838fbf3dd1bd316c64db429e9884d`; The independent reviewer approved this exact corrected hash; corrected cloud RED remains required before production resumes.

## Corrected V3 target-contract RED

[Actions 39832128801](https://github.com/puzige/vega/actions/runs/39832128801), exact test-only head `b5b4c8ba90dc02444417d3a6b55d2f401acdbe45`, Nextest job `120010694168`, executed **25 tests: 7 passed, 18 failed, 0 skipped** in 4.488 seconds. All 60 Python tests passed. The corrected tool-context compatibility control passed; the remaining 18 failure identities exactly match the prior target-contract failures, with no new failure identity. This includes receipt-contract failures where later assertions await GREEN, not a claim that every later assertion executed in RED.

The test job ran 18:31:19–18:35:44 UTC. Clippy was still running when production implementation was authorized after reading the completed test job's original log; it began at 18:36:18 after queue time. Its result and the full workflow completion are recorded separately. No rerun or test weakening was used.

The exact corrected source hash `c5d67018505df15e4e85aaa52ef5228f91f838fbf3dd1bd316c64db429e9884d` was independently approved and read back from GitHub before production resumed. The full test/support patch against baseline `6615748f91273bfb8ee424011b3b5a09b3777d2b` has SHA256 `66dd737edb05ac3dc0c66d78e0d8712ce832725ab6dac84e3144a4f57bf20dc7` (101,937 bytes). Same-test GREEN, implementation review and final cleanup gates remain outstanding.

The corrected v3 workflow subsequently completed at 18:39:45 UTC (510,000 ms from creation). Clippy passed: 18:36:18–18:39:39, 201,000 ms execution after 303,000 ms from workflow creation to job start. Full status was observed after the five-minute query interval. This completes the RED baseline's format/Clippy/Python checks; only the expected target-contract tests are red.

### Recoverable frozen inputs

- [Complete corrected test source at full immutable head](https://github.com/puzige/vega/blob/b5b4c8ba90dc02444417d3a6b55d2f401acdbe45/crates/vega_conversation/src/agent/subagents/execution_bounds_tests.rs): 97,681 bytes, SHA256 `c5d67018505df15e4e85aaa52ef5228f91f838fbf3dd1bd316c64db429e9884d`.
- [Complete baseline-to-test-head diff, including support and specification](https://github.com/puzige/vega/compare/6615748f91273bfb8ee424011b3b5a09b3777d2b...b5b4c8ba90dc02444417d3a6b55d2f401acdbe45.diff): 138,616 bytes, SHA256 `7bde1e732bcd34e12aa5ae30e010cdc531255bd5b8567d338422d85f2ad8daa1`.

Both were fetched back from GitHub and checked before resumed production. These full SHA links are independent of the task branch name and temporary Actions artifacts. Reachability must be checked again after squash/remote-branch cleanup. GREEN evidence must identify the same source hash before test removal.

## First production candidate and review-discovered boundaries

Production candidate `857de90459e21cd5908120688a5f522eee2a7bf0` retained the exact frozen 25-case source. [Actions 39835164092](https://github.com/puzige/vega/actions/runs/39835164092) executed 25 cases: 24 passed, one failed, zero skipped. Format, Clippy and all 60 Python tests passed. The workflow ran 18:52:23–19:00:17 UTC (474,000 ms). Clippy started at 18:57:27 after 304,000 ms from workflow creation and completed at 19:00:11; Nextest ran 18:52:26–18:56:08. Full terminal status was observed at 19:03:22. These overlapping job intervals are not added together.

The failing native explicit-wait replay had already proved three parent provider calls, one child provider call, two waits and only one timeout across repeated former deadlines. Its final lookup tried to deserialize the whole projected tool-message content as JSON. Independent diagnosis found that the existing 60-second progress reporter appends a runtime notice after that JSON during model projection; this path was not changed by the candidate. The raw timeout result was already observed successfully by the event sink. This is a test observation defect, not evidence that production dropped the timeout receipt. Correct only the lookup: select the exact `replay-wait-1` tool result and parse its first complete JSON value, retaining `timed_out`, `timeout_coalesced` and `wake_reason` assertions and checking the existing trailing notice. The corrected test must be independently reviewed before execution; all other original targets remain frozen.

Independent production review rejected the candidate for three additional lifecycle windows: exhausted active time followed by new pending attention; persistence failure while host release is still pending; and `cancel_agent` returning a terminal result before host release. It also identified a forbidden non-test `expect`. Production is preserved at the candidate above. Acceptance v3.1 explicitly records the three omitted boundaries; new tests-only cases must establish RED before their implementation is repaired. No completed GREEN or delivery is claimed. Original timings and both test-observation and implementation rework remain visible.

### Frozen v3.1 source

The independent reviewer approved the tests-only delta at `9476da583c7499bbc340b17877ab2a270174c6b7`. It contains 28 cases: the original 25 with the single reviewed observation correction, plus the three newly specified failure boundaries. Production is identical to `857de90459e21cd5908120688a5f522eee2a7bf0`. No original call-count, timeout, cancellation or drain expectation was relaxed.

- [Complete 28-case source](https://github.com/puzige/vega/blob/9476da583c7499bbc340b17877ab2a270174c6b7/crates/vega_conversation/src/agent/subagents/execution_bounds_tests.rs): 106,966 bytes, SHA256 `a5d2a4a27d0091f8bfd6b2ec7710640cdb37b5257d620f0dccc87150cc3d8b9c`.
- [Boundary test delta and specification/evidence updates](https://github.com/puzige/vega/compare/857de90459e21cd5908120688a5f522eee2a7bf0...9476da583c7499bbc340b17877ab2a270174c6b7.diff): 23,430 bytes, SHA256 `efc1154ea0bcee426c6f9543cf1679212e9061c634c3542e32893551ca5f6388`. The test-file-only patch is 10,994 bytes, SHA256 `6761defbd5ed446f9b615300de8224b2446b20eb5cffe40d8c4eb4f3b28b75db`.

Both remote inputs were fetched back and verified at 19:08:32 UTC, before production repairs. The earlier full test/support patch plus this delta recovers the complete test setup without relying on the final delivery tree or a temporary CI artifact.

### v3.1 RED result and one fixture correction

[Actions 39837443986](https://github.com/puzige/vega/actions/runs/39837443986) at the frozen head compiled and executed all 28 cases: 25 passed, three failed, zero skipped; format/Clippy and 60 Python tests passed. The original native wait replay now passed with its unchanged behavioral assertions. The B4 persistence/release and W2 cancel/release cases failed at their target assertions. B1 failed earlier: its new question fixture used `late-choice`, but the existing `UserQuestionInput::validate` permits only lowercase ASCII, digits and underscores for question IDs. Its expiry assertion therefore was not reached; this is not valid B1 RED.

The minimal correction changes only that fixture ID to `late_choice`. Production and every behavioral assertion remain unchanged. Independent correction review and a new existing Actions run are required before production repair. The unsuccessful run remains recorded: 19:08:12–19:11:50 UTC (218,000 ms), first observed complete at 19:13:22 (92,000 ms later).

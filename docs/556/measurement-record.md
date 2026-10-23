# Execution efficiency evidence ledger

## Corrective continuation: stage handoff and recovery

The user's 2026-10-23 continuation requires removing the default 20-minute hard cancellation, implementing durable stage handoff and explicit interrupted recovery, then reconciling the existing #522 experiment before completing its comparison. The new [acceptance contract](stage-handoff-recovery.md) supersedes the timer-based claim below. The old timer tests remain historical proof of that implementation; they are not evidence that a useful artifact appeared earlier or that recovery worked.

The first tests/spec publication was `928cf828`, with production unchanged from original baseline `4c0d225d`. One original implementation agent and one original independent reviewer are reused. No new #522 namespace, model trial or installed application has been started for this continuation. Record implementation/review/CI intervals and subsequent research receipts as they occur; unknown start times or durations remain unknown. The original #514 clock and failed/cancelled experiment costs remain included in their respective records.

The first cloud baseline [39883065565](https://github.com/puzige/vega/actions/runs/39883065565) took 168 seconds from creation to workflow completion, with parallel fmt/Clippy and Nextest jobs. It executed 27 Rust cases (11 passed, 16 failed, none skipped) and passed all 60 Python cases; the timer failure was directly observed, while most deeper recovery assertions stopped at missing prerequisites. See the [full provenance](recovery-test-evidence.md#first-cloud-baseline-and-current-policy). The branch subsequently integrated master `f3eab982` as `03c1296a`, retaining the first timer-removal patch and store drafts without conflict. The first production [Actions 39888493764](https://github.com/puzige/vega/actions/runs/39888493764) then took 157 seconds and failed compilation before Rust discovery/execution; 60 Python checks and formatting passed. Independent review overlapped that run and required future-drop/admission-cancellation repairs, which remain recorded as implementation rework.

The combined repair integrated current master `9f9fa092` (#573) as `37b0291f`. That policy corrects #569: no mandatory TDD/freeze, but development Rust tests/support must be archived and removed before master, followed by the final complete empty-suite gate. The 10 #570 development cases were removed upstream; its production read fix remains. This task still verifies its own cases before cleanup. Extra validation/cleanup time and every failed attempt remain part of development wall time; neither a changed policy nor an empty-suite gate demonstrates delivery acceleration.

The stopped repaired head `7873ffb8` passed independent static implementation review. Its [Actions 39891169248](https://github.com/puzige/vega/actions/runs/39891169248) lasted 375 seconds (02:43:56Z → 02:50:11Z); 24 Rust cases actually ran, 23 passed and 1 failed, with 0 skipped, while format and all 60 Python checks passed. Two Clippy comparison warnings and a cross-parent-run name-reservation regression still blocked delivery. The original writer repairs them without changing the 24 test cases or starting another research trial. Review and CI overlapped; neither their durations nor the later status-observation delay are added as separate serial phases. Failed execution and repair time stay in the total.

The unchanged 24 cases then all passed on independently reviewed head `e5c2fbc25f00d19adc5a5692b2206fd66e2b7281`: [Actions 39892557171](https://github.com/puzige/vega/actions/runs/39892557171), 02:59:26Z → 03:02:58Z, 212 seconds wall; actual Rust execution summary 2.728 seconds. Format, Clippy, all 60 Python cases and the complete required gate passed. Development sources and their exact hashes are retained in the [validation ledger](recovery-test-evidence.md#development-green-and-recoverable-source). Policy-required cleanup and the final empty-suite CI remain additional delivery work, not speed gains. There has been no paid native delivery-speed trial for this runtime change.

## Observed case: #514

This is the actual completed observation, not a treatment/control experiment. Native start: 2026-10-22 06:31:57.103 UTC; merge: 14:59:55 UTC; final native terminal: 15:01:33.615 UTC. Both cancellations and all recovery gaps remain included. The observer's later closeout is excluded.

| Measure | Observed value | Interpretation |
| --- | --- | --- |
| First start → merge | 30,477,897 ms | 8h27m57.897s wall time |
| First start → native terminal | 30,576,512 ms | 8h29m36.512s wall time |
| Spawn / followup / wait | 5 / 19 / 215 | Actual native tool calls |
| Explicit wait inputs | 214 × 60000 ms; 1 × 1000 ms | All 215 calls supplied a timeout; a default-only fix misses this pattern |
| Successful model calls | 894 | Parent 483, children 411 |
| All model request interval union | 24,096,778 ms | 6h41m36.778s; includes unknown provider queue time, not pure reasoning CPU |
| Summary interval union | 2,302,245 ms | 38m22.245s; outer/inner events de-duplicated |
| CI created → last job completed, sum | 4,779,000 ms | Ten distinct heads, each attempt 1; concurrent jobs counted once |
| CI completed → first native observation, sum | 3,188,507 ms | 53m08.507s; overlaps other task work |
| Test scope | 19 → 24 → 31 → 38 | Real additional acceptance and prerequisite findings |
| Final delivery evidence | PR #529 merged | Required independent review and CI preserved; Issue remains open for user manual validation |

Public delivery: [PR #529](https://github.com/puzige/vega/pull/529), [final full gate](https://github.com/puzige/vega/actions/runs/39798701301). Detailed native timings come from the user's scoped local observation records, not from the GitHub run alone. No credentials, full conversation export or private workspace paths are published here.

The #532 commit (`e4f18c656aa69d192d095a4338a2108e6c9a3c2c`) changed the skill, execution guidance and injected system/delegation text. Its two Rust-file edits were prompt text, not wait scheduling, execution-budget or history-projection mechanisms. The 20-minute trigger was explicitly a diagnostic instruction, not runtime termination. The observed long stage therefore did not violate an enforced timer: none existed. This is a code-level explanation of why guidance alone could not prevent the pattern, not proof that every observed minute was avoidable.

Important sources of rework were quote/list indentation ownership, wide ordered markers, pending tilde fences, double indentation of quote-first items, misplaced synthetic prose closers and a parser prerequisite blocking UI assertions. These are substantive defects. Removing independent review to suppress their discovery would not be an efficiency improvement.

The user challenged the candidate 64KiB entry / 256KiB request caps before production resumed. Those were heuristic values and have been removed from acceptance v3. Measurement must compare actual projected inputs and provider usage, preserve the model-policy provenance (configured, assumed or unknown), and count any context-limit failures as failures rather than speed wins.

## Prospective matched-pair record

No real model trials have been run for #556. Populate one row for every attempted task, including failed and cancelled tasks. Do not fill missing values with zero.

| Field | Required evidence |
| --- | --- |
| Pair / variant / run order | Predeclared matched task and baseline/treatment assignment |
| Application revision / installed version | Actual native app identity; merged source alone is insufficient |
| Model / effort / permissions | Exact observed settings; keep constant within a pair |
| Starting repository / acceptance revision | Same starting contents and frozen acceptance, or explain mismatch |
| Original start / resumptions / terminal | UTC events; original clock never resets |
| First reviewable artifact | Version/content identity, scope, known gaps and confirmed stopped writer; not a timeout receipt |
| Independent review | Reviewer identity separate from implementer, reviewed version and actionable findings |
| Necessary regression checks / final CI / merge | Exact heads, run/job evidence and separate timestamps; retain unsuccessful attempts without requiring a RED/deletion ritual |
| Wall time / active / human pending | Wall time is primary; activity is a separately defined diagnostic |
| Model / context / summary | Request count, actual input usage when available, known limits and de-duplicated intervals |
| Delegation / no-change wakes / rework | Every spawn/followup, repeated unchanged result, changed acceptance and repair cause |
| Outcome / residuals | Delivered, failed, cancelled or incomplete; user manual validation remains separate |

Report all pairs before aggregating. Three pairs are a small directional pilot, not statistical proof. Report median time to reviewable artifact and median total delivery time only alongside each pair's completion/quality outcome. Budget/context failures count against the treatment until recovery actually delivers. Parallel review/model/CI intervals must be shown as overlapping phases or interval unions.

## Historical #561 mechanism evidence versus speed evidence

| Claim | Required evidence | Current status |
| --- | --- | --- |
| No repeated explicit/default wait timeout loop | Native-path replay of explicit 60000-ms calls, shared positive/zero allowance, target identity and terminal-delivery checks | Frozen 28-case Actions 39839892352 GREEN, including native replay; one initial timeout and one outstanding coalesced wait across four former deadlines |
| Stage stops at active allowance and drains safely | Controlled-time native lifecycle test with real queue transitions and release failure | v3.1 target RED followed by identical-source GREEN for all 28 cases, including the three review-discovered windows |
| Repeated history reduced without dropped user constraints | Provider capture through native followup; large logs, images, authority/source checks, valid large input and configured token admission | Same-source GREEN; no new 64/256KiB quotas, inherited token policy preserved |
| Independent review and CI retained | This PR's test review, RED/GREEN, implementation review, source archive, deletion review and final head gate | Implementation approved and same-source GREEN archived; cleanup/final exact-head evidence tracked in PR #561 |
| Actually faster native delivery | Installed build plus matched real-task observations under the contract | Not run; no speed percentage claimed |

## Development rework in this change

The first production candidate's Actions run lasted 474,000 ms from creation to workflow terminal. Nextest executed 25 cases with 24 passing and one test observation failure; Clippy and 60 Python cases passed. The observation failure was diagnosed against the existing progress-notice projection, without changing the wait mechanism. Independent review found three distinct lifecycle defects and one forbidden panic call. These remain recorded as review value and implementation rework, even after repair.

The three missed windows were consolidated into one explicit v3.1 tests-first round on the preserved candidate. The original implementer and reviewer were reused. There was no repeated review request for unchanged content, no CI retry-to-green, and no hidden reset of the original work clock. One invalid question-ID fixture required a separately reviewed correction; the unsuccessful attempt remains recorded. Corrected Actions 39838505410 demonstrated all three target failures before repair. Actions 39839892352 then passed the identical frozen 28 cases, and the independent reviewer approved the exact implementation head. Test cleanup and the final gate are separately identified in PR #561; real delivery-speed measurement remains outstanding.

# Execution efficiency evidence ledger

## Observed case: #514

This is the actual completed observation, not a treatment/control experiment. Native start: 2026-10-22 06:31:57.103 UTC; merge: 14:59:55 UTC; final native terminal: 15:01:33.615 UTC. Both cancellations and all recovery gaps remain included. The observer's later closeout is excluded.

| Measure | Observed value | Interpretation |
| --- | --- | --- |
| First start → merge | 30,477,897 ms | 8h27m57.897s wall time |
| First start → native terminal | 30,576,512 ms | 8h29m36.512s wall time |
| Spawn / followup / wait | 5 / 19 / 215 | Actual native tool calls |
| Successful model calls | 894 | Parent 483, children 411 |
| All model request interval union | 24,096,778 ms | 6h41m36.778s; includes unknown provider queue time, not pure reasoning CPU |
| Summary interval union | 2,302,245 ms | 38m22.245s; outer/inner events de-duplicated |
| CI created → last job completed, sum | 4,779,000 ms | Ten distinct heads, each attempt 1; concurrent jobs counted once |
| CI completed → first native observation, sum | 3,188,507 ms | 53m08.507s; overlaps other task work |
| Test scope | 19 → 24 → 31 → 38 | Real additional acceptance and prerequisite findings |
| Final delivery evidence | PR #529 merged | Required independent review and CI preserved; Issue remains open for user manual validation |

Public delivery: [PR #529](https://github.com/puzige/vega/pull/529), [final full gate](https://github.com/puzige/vega/actions/runs/39798701301). Detailed native timings come from the user's scoped local observation records, not from the GitHub run alone. No credentials, full conversation export or private workspace paths are published here.

Important sources of rework were quote/list indentation ownership, wide ordered markers, pending tilde fences, double indentation of quote-first items, misplaced synthetic prose closers and a parser prerequisite blocking UI assertions. These are substantive defects. Removing independent review to suppress their discovery would not be an efficiency improvement.

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
| RED / same-test GREEN / final CI / merge | Exact heads, run/job evidence and separate timestamps |
| Wall time / active / human pending | Wall time is primary; activity is a separately defined diagnostic |
| Model / context / summary | Request count, actual input usage when available, known limits and de-duplicated intervals |
| Delegation / no-change wakes / rework | Every spawn/followup, repeated unchanged result, changed acceptance and repair cause |
| Outcome / residuals | Delivered, failed, cancelled or incomplete; user manual validation remains separate |

Report all pairs before aggregating. Three pairs are a small directional pilot, not statistical proof. Report median time to reviewable artifact and median total delivery time only alongside each pair's completion/quality outcome. Budget/context failures count against the treatment until recovery actually delivers. Parallel review/model/CI intervals must be shown as overlapping phases or interval unions.

## Mechanism evidence versus speed evidence

| Claim | Required evidence | Current status |
| --- | --- | --- |
| No default timeout wake loop | Native-path controlled-time test, unchanged dependency across multiple old timeout periods | Pending Actions |
| Stage stops at active allowance and drains safely | Controlled-time native lifecycle test with real queue transitions and release failure | Pending Actions |
| Stage context bounded without dropped user constraints | Provider-request capture through native followup preparation; large logs, images, long tool inputs and stale source cases | Pending Actions |
| Independent review and CI retained | This PR's test review, RED/GREEN, implementation review, source archive, deletion review and final head gate | Pending |
| Actually faster native delivery | Installed build plus matched real-task observations under the contract | Not run; no speed percentage claimed |

# Native execution bounds after #514

Status: acceptance v2, revised before resumed production implementation. Related: #556, #514 / #529, #525 / #532, #522.
Baseline: `871f91f246e90753743a256cee1eec315e74effc`. User authorization: 2026-10-22, implement execution mechanisms, retain independent review and CI.

## Evidence and limits

The observed native task used Vega 0.1.195, gpt-6.1-sol with Max confirmed in the UI, and #532 guidance. From first start to merge it took 30,477,897 ms (8h27m57.897s), including cancellation and recovery. It used 5 spawn, 19 followup and 215 wait calls; 894 model requests succeeded. Summary intervals, after de-duplicating outer/inner events, occupied about 38m22s. One implementation stage lasted 71m11s before interruption; its recovery took 11m32s without additional source changes. Test scope expanded 19 → 24 → 31 → 38, including real parser/layout defects found late.

Ten distinct-head CI runs were observed, each attempt 1. Their created-to-completed intervals totalled 79m39s; completed-to-first-native-observation totalled about 53m08s. These intervals overlap review and other work; neither sum is a serial addition to wall time. The previous 2h41m13s run did not deliver. It is not a controlled speed baseline. Max/provider latency causality is unproven.

Runtime findings: `wait_agent` returns every 30 seconds by default, at most 60 seconds; each result enters history and permits another parent model request. Exact archived tool inputs show that all 215 waits explicitly supplied a timeout: 214 requested 60000 ms and one requested 1000 ms. Changing only the default cannot remove this observed pattern. Acceptance v1 missed that fact; partial production work was stopped and preserved, and v2 requires new tests-first RED before resuming. Followup goes through normal full-history preparation. Stage readiness and the 20-minute diagnostic are only guidance. The separate #522 harness does not inject the native subagent hook.

## Acceptance scenarios

| ID | Observable requirement |
| --- | --- |
| W1 | A default wait remains suspended across the former 60-second limit while dependencies are unchanged; no repeated parent provider request or duplicate timeout result is produced. |
| W2 | A terminal execution is observable as released only after its future drains, terminal persistence succeeds and native host acknowledges release. A completed message alone is insufficient. |
| W3 | Each exact child execution `(agent_id,message_id)` in a parent registry permits one explicitly bounded observation: its first positive timeout retains that deadline, or its first zero returns a snapshot. A later explicit positive or zero wait on the same execution coalesces into event waiting. This deliberately changes repeated-call semantics; omitted timeouts remain event waits. |
| W4 | Parent cancellation/end, new steering and child permission/question attention wake waiting. Readiness does not claim or consume steering; the existing take path consumes it once for its generation. |
| W5 | An attention event is delivered once per execution/request generation. Repeating a default or coalesced wait while the same request remains pending suspends rather than creating a model hot loop; a new request can wake again. Attention and steering do not replenish a bounded-observation allowance. |
| W6 | Claim the bounded allowance atomically after validating targets, keyed by exact execution, not tool-call ID or target array. Different deadlines, reordering, subsets, progress/token/tool activity and intervening list/read calls do not replenish it. A mixed target set gains a finite deadline only from a genuinely new execution; concurrent waits cannot both claim the same allowance. |
| W7 | A previously delivered terminal target cannot repeatedly wake a mixed wait whose other targets are still running. A new released terminal target can wake it; all-terminal queries may return cached results. New spawn/followup message IDs get their own allowance, without resetting elapsed accounting of the original task. |
| B1 | Every native delegated stage has 20 minutes of cumulative active time. Provider requests, stage preparation and tools count; actual permission and user-question pending intervals pause the counter. Model text claiming to wait does not pause it. |
| B2 | On expiry, request cancellation and continue awaiting the original child lifecycle. Never abort/drop the execution or start a replacement while it can still write. No subsequent provider/tool dispatch after cancellation. |
| B3 | Budget expiry is Interrupted and `ready=false`, with cause and bounded actual partial output plus exact agent/message references. No output means no artifact claim. This does not certify review readiness or completion. |
| B4 | Completed work is not retrospectively cancelled. Parent cancellation wins over automatic recovery. Persistence/release failure cannot advertise stopped writes: wake the default wait with an explicit failure and fail closed for all further admissions in that parent registry while existing children drain. No automatic respawn, renewal or failed/interrupted followup. |
| C1 | Followup provider input retains all original user instructions, intervening user steering and the current task in order, including images/reference content, and the last completed stage's final response. Earlier tool logs and thinking are excluded from the new stage. |
| C2 | The selected final response is the last completed response, not the first 16KiB of accumulated progress. Bounded result truncation is explicit and points to the original execution. Tool/assistant content remains lower trust than user/system instructions. |
| C3 | The stage-entry projection is at most 64KiB of text, including instruction, result and reference framing. Oversized user instructions fail before a provider request; do not silently truncate constraints. The model's existing token/image budget also applies. |
| C4 | The current delegated stage has a 256KiB request-text ceiling checked before each provider request: system/reference framing, user and assistant text, assistant tool-call input JSON and tool results all count. Images remain subject to model token/image accounting. Its inherited model capacity/output reserve remains valid, but automatic model summarization is disabled for this bounded stage. Context exhaustion returns an explicit incomplete/error outcome with recoverable history, without summary/retry loops. Ordinary main conversations retain their existing compaction policy. |
| C5 | Stored history, tool-call deduplication authority, credential checks, original permissions/model/system instructions and source-version/fingerprint checks remain intact. Reject stale, externally continued, non-Completed and authority-changed continuations. No context rewrite to the real source. |
| I1 | Tests exercise the native Supervisor/Hook and child entry/pipeline with a scripted provider, not only helper functions. At least one integration case proves native wiring; provider/clock are isolated external dependencies. |
| G1 | No runtime outcome certifies independent review, test GREEN or final CI. Existing review and CI gates remain mandatory on the exact deliverable version. |

## Implementation contract

### Event waiting

`wait_agent` with omitted timeout waits for a new actionable dependency event. For explicit timeouts, the registry atomically claims the one-time bounded-observation allowance of each eligible pending execution when the wait is admitted. If at least one exact execution is fresh, that call honors its requested deadline, including zero; all eligible targets in that call are marked observed. Once consumed, changing the tool-call ID, timeout value, target order or subset cannot restore the allowance. Cancellation or an attention wake does not refund it. A repeated wait with no fresh execution uses event waiting even if it supplies a positive or zero timeout.

Only a new child execution message ID starts a new allowance. Ordinary progress, model/tool activity, list/read calls, new attention and steering do not replenish it. Parent run recreation may create a new registry, but measurement keeps the original task clock and every restart. These are explicit API semantics, including that repeated zero is no longer guaranteed nonblocking; schema and tool descriptions must state them. Use the existing registry notification plus non-consuming steering readiness and actual permission/question request identity. A bounded local readiness check is permissible where no notifier exists; it must not ask a model, consume input, query GitHub or append repeated history. Avoid creating new UI copy for routine waiting.

Successful wait receipts retain `agents` and `timed_out`, and add `wake_reason` (`terminal`, `steering`, `attention` or `timeout`) and `timeout_coalesced`. The latter is true only when an explicit timeout was supplied but no fresh bounded allowance was available; it is false for omitted timeouts and first bounded observations. Cancellation, parent end and release failure stay explicit errors. Wake reasons distinguish a released terminal result, new user input, new attention and an explicit timeout. Tool duration covers the whole actual suspension. Attention acknowledgements are scoped to the parent run and exact child execution/request. Record delivered terminal executions so an old terminal in a mixed running/terminal target set cannot repeatedly wake the parent. All-terminal queries may return cached state. `list_agents` remains a snapshot and cannot reset the allowance; this change eliminates repeated `wait_agent` timeout wakeups, not arbitrary model loops made solely of other snapshot tools. Race checks occur after registering notification interest. Cancellation remains immediate; pending human input cannot be hidden behind an indefinite default wait.

### Stage budget and ownership

The 20-minute value is a conservative initial stage boundary, derived from the prior diagnostic threshold, not a measured optimal duration or promised completion time. The mechanism bounds an unattended stage; it does not guarantee a useful patch. Tasks should produce the smallest reviewable stage before that boundary. A long legitimate task must be explicitly re-scoped after the bounded incomplete report, with original elapsed time retained.

Track monotonic active duration from stage execution/preparation through completion. Observe actual pending transitions rather than testing a boolean once at the deadline. Preserve UI's existing queue receiver; do not steal permission/question notifications. Cancellation, terminal persistence and host release are distinct phases. Hold occupancy until release is acknowledged. Persistence/release failure wakes the parent and blocks every new admission in its registry; there is no role/replaces field capable of identifying a replacement writer more narrowly. Do not loosen followup admission for arbitrary interrupted work.

Expose only factual receipts: budget cause, completion state, known elapsed accounting, actual retained output and execution references. A reviewer named in a task does not gain enforced read-only authority: this change does not claim to add write-path leases or a reviewer permission sandbox.

### Bounded stage context

Use a deterministic projection at the validated continuation boundary. All user instructions and attachments survive; old tool history remains recoverable in the child conversation, but is not repeatedly transmitted. Preserve the final answer's provenance and original message identity. If safe projection is unavailable, fail before the provider rather than silently dropping constraints or reverting to a huge old context.

Apply the same projection to preinsert/postinsert preparation and replay framing. A stage cannot accidentally reintroduce old logs through compaction: native delegated requests use bounded context with automatic summaries disabled in memory. User configuration and ordinary parent compaction are unchanged. Keep text accounting distinct from model token/image accounting. A task stopped for context limits is unsuccessful in speed evaluation until it is actually recovered and delivered.

Original message references allow inspection/recovery through existing conversation details; they do not imply an automatic history retrieval tool or completed recovery. The size limits are initial policy values, not empirically optimal settings.

No new service, persistent telemetry collector, model trial runner, workflow, application installation, real database migration or user-configuration change is in scope. Production dependencies remain unchanged. Temporary Rust test support can use existing approved dependencies and Tokio's dev-only controlled-time feature.

## Validation and delivery sequence

1. Implement acceptance tests before production changes; independently review them. Existing PR Actions must show valid RED (behavior failure preferred; a necessary missing interface is labelled separately).
2. Freeze the test cases and expectations, then implement. The same tests must be GREEN alongside all existing checks. No ignored tests or retry-to-green.
3. Independent review checks cancellation/drain races, request identity, projection provenance and exact final implementation. Review and CI can overlap after the implementer stops writing.
4. Preserve complete test source/patch, full SHA and RED/GREEN run/job evidence in the PR. Delete Rust tests and exclusive support in a separate commit; review production equivalence.
5. The final exact head must pass the entire existing CI: fmt, all-target Clippy, Python and Rust discovery/build with 0 tests labelled `no Rust tests`. Then merge and keep the Issue open/In review for user manual validation.

No local Vega execution, builds or tests. CI status reads follow the five-minute/event rule. Unchanged pending work does not trigger another review, another provider request or another CI run. A change to relevant code/acceptance requires the appropriate renewed checks; elapsed time is not permission to bypass gates.

## How to establish an actual speed improvement

Deterministic CI establishes mechanism correctness, not real-world speed. Replay the observed native pattern of explicit 60000-ms waits: after one initial timeout, repeated waits spanning many former deadlines use one outstanding call and do not keep calling the parent provider. Cover positive/zero sharing, target-set changes, concurrent claims, attention/cancellation and already-delivered terminal targets. Controlled-time cases should also prove: the active clock expires only after its configured allowance and waits for drain; continuation input stays within the declared envelope and excludes large old logs; cancellation and new human input remain responsive. Include heavy old logs, pending attention generations, long provider output, source edits, release failure and budget exhaustion.

After a release containing this change is installed, evaluate actual native tasks with fixed application commit, model/effort, tool permissions, starting repository revision and acceptance scope. Use at least three matched task pairs from the same task classes, predeclare task scope and alternate old/new order. Do not rerun only failures or discard interruptions. The user authorizes any real provider runs; this code task does not silently start a paid batch or repeat #514 against production.

Record these fields per task from original start through final delivery, including every cancellation/recovery:

- Time to first independently reviewable version with declared scope, actual version and stopped writer; an incomplete timeout receipt is not that milestone.
- Time to same-test GREEN, final exact-head CI and merge, separately; all wall clocks include waiting and rework.
- Completion/acceptance outcome, substantive review defects, number of rework cycles and lost/omitted constraints.
- Spawn/followup counts, parent wait completions and repeated no-change wakeups, provider requests/input tokens, summary count and de-duplicated elapsed intervals.
- Activity, human pending, model request, independent review and CI intervals; report overlaps/unions, not an additive fictional total. CI execution and completion-to-observation latency are distinct.

Success requires no regression in completion/acceptance, cancellation safety, independent review or CI, and lower time-to-reviewable plus lower total delivery time on matched tasks. Report every pair and median; a small pilot gives directional evidence, not a general percentage guarantee. If only wakeups/input decline while delivery does not, report that the overhead mechanism works but total speed improvement is unproven. If budget/context stops increase failures, adjust stage design using the recorded causes; do not label faster failure as faster delivery.

#514 is an observational regression case for pathological patterns. Its old 215 waits can motivate a deterministic no-change trace, but replaying timestamps cannot establish the hypothetical merge time of an alternate implementation.

# #307 resumed phase2 TEST-FIRST checkpoint

## Current runtime test checkpoint — latest scope replaces the historical plan below

**READY for independent runtime test review, not implementation or delivery.** Baseline remains `92f281e5a5a56230fa4e4a8a8cf1ab6f20e9670b`. The resumed writer fetched origin and fast-forward checked the tracking branch (already up to date). No commit/push, compilation, tests, application execution or native permission operation was performed.

The frozen spec's top **最新范围裁决** supersedes the former live-recheck / automatic Ready / automatic close / Ready-gated return requirements. All preparation RPCs are false/false, including manual recheck. Static Granted remains live-capture unverified. No native permission bridge is introduced. Manual close/back to task does not authorize or resume actions; normal fresh authorization and observation remain required. The old plan below is retained only as historical evidence, including its former plan-only stop condition; it is not the current acceptance or work instruction. Its unsupported non-prompting live-capture proposal is explicitly withdrawn before implementation and before any tests for that proposal existed.

### Actual patch and scope

- `crates/vega_runtime/src/computer_use/preparation_session_tests.rs`: thirteen focused tests of production session lifecycle, actual `ComputerUseControl::begin`, and existing preparation classification.
- `crates/vega_runtime/src/computer_use/mod.rs`: only `#[cfg(test)] mod preparation_session_tests;` wiring.
- This plan: current scope, frozen interface signatures and expected RED evidence.
- Parent-owned frozen spec, driver blocker and native review are not edited by this writer. No dependency or production behavior changes, no Rust comments, no historical test restoration.

Following/draggable card, retained failure entry, manual return with fresh normal task authorization, visible unverified state and stale UI callbacks remain required. Their actual UI consumers need their own preimplementation tests next; this runtime patch does not claim that coverage or relax those requirements. Native dragging, following, identity/TCC and administrator-dialog stacking remain manual integration checks.

### Frozen real production interfaces (deliberately absent on baseline)

The following is a signature contract, not a standalone Rust implementation. Types live in the existing internal `computer_use::probe` and `computer_use::control` modules and are visible to the sibling test module. No fake decision controller or placeholder production bodies are supplied.

```text
control::OwnershipAuthority: Default + Send + Sync
    async fn cleanup_pending(&self) -> Result<(), ComputerUseError>
ComputerUseControl::with_authority(Arc<OwnershipAuthority>) -> Self

probe::PreparationTransportFactory
    type Transport: PreparationTransport + Send + 'static
    fn open(&mut self) -> Result<Self::Transport, ComputerUseError>

probe::PreparationTransport
    fn check_permissions(
        &mut self,
        prompt: bool,
        probe_direct_capture: bool,
    ) -> Pin<Box<dyn Future<Output = Result<Option<serde_json::Value>, ComputerUseError>> + Send + '_>>

    fn cleanup(&mut self)
        -> Pin<Box<dyn Future<Output = Result<(), ComputerUseError>> + Send + '_>>

probe::PreparationSession<F: PreparationTransportFactory>
    fn open(factory: F, authority: Arc<OwnershipAuthority>, cancel: CancellationToken)
        -> Result<Self, ComputerUseError>
    async fn check(&mut self) -> Result<Preparation, ComputerUseError>
    async fn close(&mut self) -> Result<(), ComputerUseError>
    Drop
```

`with_authority` is test-only injection into the real control constructor/state, not an alternate begin implementation. Production constructors must share one authority for normal runs and preparation, with owner-specific release. Production onboarding must use this same generic session controller with the real embedded daemon/MCP factory; the existing one-shot `detect` is not sufficient integration. The factory is the external component/spawn boundary; synchronous open must not block waiting for daemon readiness. Async transport owns connection/readiness/RPC I/O and reuses the client. The controller owns atomic admission, the cancellation race, terminal state and fixed false/false policy. A successful status check does not release or auto-close the session.

### Pre-RED harness correction after independent BLOCKED review

Original rejected test SHA-256: `6565b8b58cbe33fd8f639e18b65d2a222b2c4f0d264c2b8ed7ef1953e160d3f7`; original plan: `6bdfbb35c400ecd22c4693f34419b88608259ec2d81a1cd59347a3d5ff034985`. This is a TEST-HARNESS/contract correction before RED, not an acceptance change. The former synchronous release contract and first-poll cancellation completion are withdrawn. `probe.rs` Drop only requests termination; even existing reclaim has bounded waits whose timeout is not proof of reaping. Normal run cleanup releases only after backend shutdown succeeds (`mod.rs` cleanup path). Resource object destruction cannot stand in for daemon termination.

The factory/transport own daemon, client and private-directory resources. `cleanup` is an external async boundary: success means the client is shut down, the child has actually exited/been reaped and private resources are reclaimed. Sending kill or timing out is not success. The test ledger decrements `live` only when the externally controlled cleanup channel returns success; Drop only counts object destruction. This tests production ownership sequencing, not native process integration. Native adapter review/manual verification remains required. A factory error must mean no live resources escape; any partially initialized live transport belongs in the async boundary, not an untracked synchronous error. No new dependencies: std, existing Tokio channels/Notify, tokio-util CancellationToken and serde_json only.

`close().await` marks the session terminal, waits for cleanup, and releases its owner-specific lease only on successful cleanup. Successful repeated close is a no-op. Failure returns the cleanup error and retains transport plus lease for an explicit close retry; pending or failed cleanup blocks BOTH a normal run and another probe. A closed/failed/cancelled session returns `Cancelled` on check without another RPC. A check RPC error or cancellation stops that RPC independently of transport cooperation, awaits one cleanup attempt and preserves the original RPC error/Cancelled result even if cleanup fails; failed cleanup stays owned for retry. Thus cancellation need not return on the first poll, and cancellation never authorizes premature resource release.

Drop transfers BOTH transport and lease into a production cleanup manager owned by the same authority (not a test-only cleanup simulation). The host must drive this manager during normal operation/shutdown, using `cleanup_pending` to drain/retry failed cleanup and surface unresolved errors. Dropping an unfinished check or close future cannot lose the transport or release its lease. Drop starts one managed attempt; a pending attempt is awaited, not duplicated. The manager retains a failed transport and its lease in an inspectable/retryable queue; no detached forgotten ownership or spin/retry loop. Production cleanup attempts must bound external waits and return an error on timeout while remaining fail-closed. A timeout is equivalent to the scripted cleanup error in this controller test, not permission to release. Native adapter timeout implementation is not tested with real processes here.

`OwnershipAuthority::cleanup_pending()` is the real host cleanup/drain API, not an ownership shortcut. It waits for in-flight cleanup, or retries an already-reported failed attempt once. A failed background result must be reported once before any retry, even if the task finished before this method was invoked. This persistent result acknowledgement removes scheduling ambiguity: the first drain returns the original failure; the next drain retries. Success removes the entry only after owner-specific release. With no pending entries it returns Ok without touching any current run/probe owner. Dropping the drain future cannot abandon a managed entry. Test synchronization uses channel permits, Notify and explicit future polling, never sleeps, live processes or first-poll reaping assumptions. Repeated old close/Drop/run cleanup or a later empty drain cannot release a subsequent owner.

### Frozen assertions

| Test | Observable acceptance |
|---|---|
| repeated_manual_checks_reuse_one_session_and_never_prompt_or_verify_capture | Three checks, one transport, false/false each time, Granted + NotChecked remains unverified and not a reauthorization request; explicit close cleans once and rejects further calls |
| run_first_rejects_preparation_before_spawning_transport | Real run begin owns admission; probe returns Busy before factory invocation |
| close_pending_blocks_run_and_other_probe_until_cleanup_completes | Close pending retains live resources and blocks run/probe; successful reap permits release; repeated close does not repeat cleanup |
| stale_probe_and_run_cleanup_cannot_release_a_new_owner | Late run release cannot evict probe; repeated closed probe cleanup cannot evict subsequent run |
| component_open_errors_release_admission_without_becoming_busy_or_denied | Missing/incompatible/unsupported/unavailable preserved; no leaked admission |
| transport_error_waits_for_cleanup_preserving_original_result | Original error only after cleanup attempt; pending cleanup blocks admission; explicit new session succeeds after cleanup; old Drop cannot evict retry |
| cancellation_before_open_does_not_spawn_or_acquire | Cancelled before external work, admission still available |
| cancellation_waits_for_cleanup_preserving_cancelled_result | Pending RPC is interrupted but cancellation awaits cleanup; resources and lease remain held until cleanup succeeds |
| rpc_error_and_cancellation_survive_cleanup_failure_without_releasing_admission | Original RPC error/Cancelled survives a distinct cleanup error; resources and admission remain held until explicit successful retry |
| abandoned_check_and_session_transfer_lease_until_background_cleanup_finishes | Dropped check/session transfers transport plus lease to manager, blocks admission until completion; later drain cannot evict a new owner |
| failed_close_keeps_lease_and_transport_for_explicit_retry | Cleanup failure preserves resources and blocks admission; close retry completes reclamation |
| abandoned_close_failure_is_retained_by_cleanup_manager_until_retry_succeeds | Dropped close/session retains managed cleanup; failure is reported without release, subsequent drain retries and completes |
| not_checked_is_unverified_not_a_request_to_reauthorize_granted_tcc | Existing production classifier preserves NotChecked, false readiness and no settings/reauthorization request |

### Intended RED and review handoff

Exact deliberate missing symbols: `control::OwnershipAuthority` (including Default and cleanup_pending), `ComputerUseControl::with_authority`, `probe::PreparationSession` (open/check/close/Drop), `probe::PreparationTransportFactory` (associated Transport/open), `probe::PreparationTransport` (check_permissions/cleanup). Test implementations and calls encode the signatures above. They must be implemented only after parent independent review and valid Actions missing-interface RED. There are no deliberate missing UI APIs in this patch.

Expected initial RED is unresolved production interfaces, **not executed behavioral assertions**. Once interfaces compile, the existing `Preparation::needs_screen_recording_settings` currently returns true for Granted + NotChecked, contrary to the frozen final test. This is a statically identified expected behavior failure, not an observed test result. No current-cycle discovery/execution/pass/fail counts or Actions result have been obtained. Unrelated harness, dependency or runner failures are not valid RED.

Parent reviews exact patch/hashes and uses the existing draft PR Actions (`cargo nextest list --workspace --message-format json`, then existing workflow nextest execution; fmt/Clippy remain workflow gates). The writer does not run these locally or commit/push. Preserve the reviewed tests through GREEN, record original source and RED/GREEN evidence, then separately remove tests/support per repository lifecycle. Local rustfmt and diff checks establish formatting only, not Rust type correctness or passing behavior.

---

## Historical plan-only checkpoint — superseded, not current acceptance

## Status and immutable evidence

**READY for parent interface/plan review; BLOCKED for a complete executable phase2 test patch. No implementation authorization is implied.**

- Inspected HEAD: `92f281e5a5a56230fa4e4a8a8cf1ab6f20e9670b`; worktree initially clean. Parent already fetched/updated; this cycle does not fetch, move HEAD, commit or push.
- User-supplied stage1 evidence: GREEN `790bfb79875b64e870bddd3c832415cc5376fba3`, Actions `39762283151`, 35 passed; deletion `7d855493f019610541e3f7bca4cb3ace4cd3ef89`, Actions `39763079980`, empty success. These runs were not independently queried in this cycle and are not phase2 evidence.
- Local Git inspection confirms the deletion touched only `prepare.rs`, `bundle.rs`, `placement.rs`, `retention.rs` (476 removed lines). No historical tests were restored in this cycle.
- Acceptance remains frozen R1–R22 in `vega-issue-307-cua-onboarding.md`. This is a resumed, pre-implementation cycle for omitted lifecycle, exclusion, presentation and retention boundaries, not retroactive validation of stage1.
- No local compilation, build, test, application run, desktop process, TCC request or native window action. No dependencies, Rust comments, production changes or placeholder implementations added.

## Inspected APIs and concrete obstruction

1. `runtime/computer_use/probe.rs:22–44`: `PreparationProbe` is a unit struct; static `detect` detects components, spawns `Session`, checks and reclaims on every call. There is no owned preparation session to open, repeat-check or close. `Session` directly owns Tokio `Child` and `ManagedClient`; replacing either requires a real external-I/O boundary, not a fake decision algorithm.
2. `probe.rs:108–150`: `connect` starts and initializes another MCP client whenever called. Merely retaining the daemon does not satisfy reuse of the complete session. `reclaim` and `Drop` both perform cleanup; lifecycle assertions must observe resource state, not insist on exactly one idempotent cleanup method invocation.
3. `control.rs:9–16,423–454,759–765`: the global owner is `Mutex<Option<String>>`. `owner_busy` checks and unlocks; `begin` reserves a run ID under the lock; `release` conditionally clears that ID. Probe admission must use the same atomic authority and retain its reservation through cleanup. A separate probe mutex would leave the run/probe race intact.
4. `probe.rs:26–27`: cancelled and busy both become `unknown_preparation(Available)`. Busy needs a separately observable admission outcome, not fabricated component availability or a transport-failure presentation.
5. `probe.rs:99–100`: Inspect sends both flags false; UserInitiated sends both true. `prepare.rs:57–61` treats Granted + NotChecked as needing screen-recording settings. NotChecked is not evidence of failed live capture. Read-only, non-prompting live capture needs its own explicit request intent if used for readiness checks; model connection preflight must remain false/false.
6. `ui/cua_onboarding/mod.rs` exports only bundle, placement and retention. There is no card lifecycle, view projection, action dispatcher or asynchronous result-generation boundary. `retention.rs::retained_entry(error, dismissed)` classifies a supplied error; it cannot prove survival of control removal.
7. `conversation_stream/computer_use.rs:6–24,51–52`: clearing control clears diagnostics and removes the object required by render. `content.rs:1114–1117` also directly clears control on MessageStarted. State fields/initialization live in `core.rs:35–38,462–465`. A helper tested without these consumers would not cover the defect.
8. `vega_ui/Cargo.toml` has no GPUI test-support dev wiring. Do not assume a GPUI test harness is available, change manifests or add dependencies under the current write scope.

**Stop condition:** isolating real probe lifecycle and testing genuine UI decisions requires an ownership/I/O refactor plus a production-consumed card/notice lifecycle. Writing those behaviors now exceeds minimal placeholder wiring and would implement the missing behavior before RED. Writing isolated mirror helpers instead would violate the effective-test rule. Per this task's explicit stop instruction, leave Rust untouched and submit the following concrete seam design before implementation. This does not assert that these APIs are impossible to add; it identifies the coordinated interface decision needed before executable tests are written.

## Proposed minimal stable seams (design, not implemented)

### Runtime session and ownership

- An owned preparation session with open, check and close/drop boundaries. Open returns an explicit busy/component/transport/cancelled outcome, with no fabricated permission result on busy. A check returns the real mapped `Preparation` plus failure information sufficient for retry presentation.
- An internal transport factory/session adapter isolates only component resolution, embedded process/MCP startup, permission RPC and resource teardown. Tests supply scripted external responses and a resource ledger; real lifecycle code decides when to create/reuse/connect/reclaim. Existing Tokio/std facilities suffice; no mocking dependency.
- One ownership authority used by both run `begin/release` and preparation admission. Use token-identified leases so stale cleanup cannot clear a newer owner. For deterministic tests, inject an instance of that same authority into both production decision paths; do not independently emulate run ownership or race against the global singleton in parallel tests.
- Separate check intent from prompt permission: Inspect = no prompt/no direct probe; explicit user request may prompt; a readiness recheck may probe capture without prompting. Exact names are subject to parent interface review. The model tool/preflight API must not gain the prompt-capable intent.
- Preserve embedded host identity using existing `desktop_daemon_arguments`, environment and policy builders. Scripted transport is not evidence of native daemon attribution; adapter review and user integration checks remain necessary.

### UI lifecycle, projection and dispatch

- A production-owned card controller, consumed by the eventual renderer, accepts probe completion, retry, explicit permission action, drag construction and close. Its observable view contains four independent fact rows, typed permitted actions, busy/error/checking/completed state, and removal state. Action dispatch emits typed effects to external probe/settings/Finder adapters; tests assert effects together with resulting visible state.
- Do not define an independent `expected_actions` test algorithm. Exercise controller transitions and the exact projection/action path the renderer will consume. Keep pure `perch/follow_target` as the sole placement decision. Drag construction permanently freezes follow for that card instance; completion still removes it.
- An owned conversation notice lifecycle consumed by `set_computer_use_control`, render and MessageStarted/reset paths. Capture permission failure before removing control; retain its original task identity and actions independently of a live control. Guard callbacks/results with task/generation identity so dismissed or replaced entries cannot reappear from stale work.
- Projection tests alone do not prove handler wiring, keyboard reachability or native behavior. Parent must approve either available GPUI harness wiring within scope or the production-consumed lifecycle extraction with explicit integration review and user keyboard/native checks. Do not silently substitute pure error classification for lifecycle coverage.

## Frozen proposed test matrix

All entries below are **planned, not authored or executed**. Filenames are proposed destinations, not existing evidence.

| ID | Spec | Real decision/input sequence | Observable assertions and expected missing behavior |
|---|---|---|---|
| P01 | R5–R6 | Open; Inspect twice; close, scripted permission results differ | One live daemon/client identity across checks; both results mapped; resources reclaimed and lease released after close. Current static API has no such lifetime. |
| P02 | R5,R9 | Startup/connect/check failure, cancellation before open and during check, explicit close/drop | No leaked child/client/private root or owner; unknown permission facts for probe errors, retry remains possible; no denied fabrication. Include successful reopen after cleanup. |
| P03 | R8 | Actual run begin wins before probe admission | Probe returns Busy, no external spawn, run remains owner; retry after release can open. |
| P04 | R8 | Probe admission wins before actual run begin; second probe competes | Run begin/second probe rejected while lease exists; close admits run; late cleanup cannot release new owner's reservation. |
| P05 | R8 | Barrier-controlled overlapping run/probe admissions using shared authority | Exactly one successful owner and at most one permitted spawn, loser reports Busy; winner remains usable. No sleep-based timing or mocked admission decision. |
| P06 | R7,R9 | Inspect, user permission action, non-prompting live recheck | RPC flags reflect intent; returned facts drive state; Inspect never prompts and cannot invent verified capture failure, live recheck can reach Ready without prompting. Model preflight stays false/false. |
| P07 | R1–R3,R15,R17 | Granted + NotChecked versus Granted + each NotReady reason | Neither reports Ready; NotChecked requests verification without claiming failed capture/re-authorization evidence; verified failure offers appropriate screen-settings recovery. Baseline `needs_screen_recording_settings` returns true for NotChecked. |
| C01 | R1–R3,R9,R15,R17,R22 | A2 six-way matrix: AX denied, screen denied, both denied, granted/capture failed, probe failed, component missing | Distinct row facts and actionable recovery; unknown is retryable and not denied; browser stays NotEvaluated. Include incompatible/unsupported component states and Busy separately. |
| C02 | R7,R15,R17 | Dispatch retry, AX settings, screen settings, invalid/disabled action | Retry is non-prompting; only explicit permission action can request prompt; correct settings destination; invalid action causes no external effect. State changes reflect actual completion/error, not submission success. |
| C03 | R12b,R12d,R16,R18 | Open, drag constructor, frame changes, failed check, ready check | Freeze begins at constructor and never unfreezes within this lifecycle; drag/drop never grants readiness; probes continue; Ready completes/removes even when frozen. New card starts unfrozen. |
| C04 | R9,R18 | Close during check; deliver late Ready/error; reopen | Closed instance stays closed, resources released, stale callbacks cannot affect new instance; capability toggle/permissions unchanged. |
| C05 | R10b,R11–R12c,R19 | Window identity missing; settings frame missing/reappears; multiple windows including smaller auth dialog | No arbitrary window movement; one degradation diagnostic; retain last valid location on transient read loss; largest settings window selected; threshold/clamp use existing production placement decision. Bound initial retry/fallback. |
| C06 | R13–R14 | Valid app executable versus dev/malformed path | Native file payload uses resolved app directory and directory flag; invalid path has no drag source and exposes Finder recovery; failure remains actionable. Actual drag/drop acceptance is manual. |
| N01 | R20,R22 | Permission failure snapshot, detach control, repeated detach/render | Entry still projects original failure and retry/settings actions without control; transport/protocol errors do not become permission denial. Current render returns None after detach. |
| N02 | R20–R21 | Dismiss; late result; new run/task; late old callback | Dismissal persists for that entry; old task results cannot mutate the current entry; task association survives detach and is never attached to an unrelated task. Parent to review new-task coexistence policy without changing R20. |
| N03 | R4,R21 | Retained entry, recheck Ready, reconnect original task | Reconnect becomes available only after readiness; no old observation/target grant reused; action before new observation/authorization rejected; new observation plus new authorization permits normal flow. Exercise actual control safety decisions, not a boolean in the notice. |

Proposed test modules: `runtime/src/computer_use/probe/tests.rs` (P01–P06), `prepare.rs` test module (P07), `ui/src/cua_onboarding/tests.rs` (C01–C06), `ui/src/conversation_stream/computer_use/tests.rs` (N01–N03), with runtime control regression tests for the actual R21 authorization/observation gate. Paths are under their respective `crates/vega_*` roots. Exact interface signatures and handler integration must be frozen by independent review before writing them.

## Stage1 regressions and native/manual boundary

If implementation touches their consumers, restore only the targeted #307 test modules from GREEN `790bfb79875b64e870bddd3c832415cc5376fba3` in `prepare.rs`, `placement.rs`, `bundle.rs`, `retention.rs`, after reviewing each case. Reason: preserve already-established classification, placement/follow, bundle and permission-classification behavior while adding lifecycle coverage. Do not restore whole files, historical unrelated suites, or label restored tests new phase2 coverage. Record exact selected test names/content hashes in the next checkpoint. No restoration has occurred yet.

R10/R10a native panel flags/level, actual cross-Space following, R13 native dragging, R15 keyboard/accessibility, R17 real settings/TCC, and R19 administrator-dialog stacking remain user manual checks. GPUI contract to verify before coding: WindowOptions Floating/is_movable, `on_drag` constructor followed by `external_drag_payload` Files/FileDragPaths directory payload. Do not infer drag completion from `has_active_drag` or synthesize native input locally. This cycle did not inspect registry implementations or execute native checks.

## RED protocol and next step

1. Parent independently reviews the proposed seams, including the shared ownership authority, non-prompting live-capture intent and production consumers of card/notice state. Resolve the exact new-task retention policy and permitted harness wiring before test authorship; do not change R1–R22 silently.
2. Resume test-only writing against frozen interfaces. Minimal missing API RED is allowed, but all missing symbols must correspond to these agreed production interfaces, not accidental API guesses. No behavior implementation to make tests compile.
3. Freeze exact test files/hashes and send to independent test review; parent commits/pushes the reviewed patch to existing draft PR/Actions. This cycle does not authorize those operations.
4. Expected valid RED: missing agreed owned-session/admission/card/retention APIs, explicitly reporting that behavioral assertions did not execute; once APIs exist, the listed lifecycle/presentation assertions must be reached. The existing NotChecked settings decision can produce a direct behavior RED once its test is authored. Unrelated compiler/harness/dependency/runner failure is not valid RED.
5. Do not call this checkpoint a complete test patch, an observed RED, or permission to implement. No current-cycle Actions run, discovery count or test result exists. After valid RED, retain the same reviewed tests through GREEN, then use the repository's separate removal/final-empty-gate lifecycle.

Sole writer ceases writes after this checkpoint and its static hash/status verification. Parent owns the next independent review. No earlier agent context or additional implementation is presumed.

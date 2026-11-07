# MCP configuration-lock bounded recovery

## Evidence and scope

Issue #887. Baseline `a818e320aaa6343e6e9b6f5391007c817dbd9dc6`. Fault-version source `8a949924dfd00f8b63e69f34a015f2e5b2fb5625` (v0.1.388).

Two private diagnostic observations concern MCP/configuration/I/O other: a running request boundary on November 4 and submission launch on November 5. No raw locking error was retained. Standard-library lock contention and other lock errors are collapsed by yaml_config::begin; WouldBlock is then collapsed by runtime diagnostics. A process-wide edit mutex already serializes normal same-process editing. Subsequent successful preparations in the same process contradict permanent mutex poisoning. These facts do not establish the original OS failure or another lock holder. The remote process has since restarted/upgraded. Do not access or mutate remote sessions, configuration, credentials or databases for implementation/validation.

## Acceptance before implementation

| Scenario | Observable result | Verification |
|---|---|---|
| Uncontended | First attempt succeeds, no added sleep | Cloud synthetic regression |
| Real external file lock released in budget | Current configuration reread; one operation completes | Cloud cross-process fixture |
| Lock never released | At most 6 attempts and at most 310ms scheduled delay; the 500ms wall-clock window may stop earlier. Preserve actual attempt/wait diagnostics, no downstream execution | Cloud cross-process regression plus deterministic complete-sequence and deadline tests |
| Cancellation during wait | Stop within one polling interval plus scheduling; no late retry or side effect | Cloud regression |
| Other lock syscall error / malformed YAML / permissions / CAS conflict | Original classification, no busy retry | Cloud regression + static audit |
| Configuration changes while waiting | Reconcile latest bytes and revision; existing revoke/disable rules remain | Cloud real store fixture |
| Active authority / running request | Retry before error invalidation; no model/tool replay; final failure remains fail-closed | Cloud service fixture + independent review |
| Nested startup reads | One shared budget for the readiness operation, not multiplied per call | Cloud regression |
| Backoff resumes at/after deadline with lock released | Reject before acquiring registry/configuration locks; retain busy diagnostics | Cloud deterministic deadline + actual service regression |
| Shared budget exhausted then lock released | Subsequent reads in the same operation retain the exhausted error and cannot reconcile changed configuration | Cloud real-lock service regression |

No local execution/build/test. Existing Actions only. No private data in fixtures or public evidence.

## Design

1. Preserve TryLockError::Error as the source of a content-free lock-acquisition error, retaining its original I/O kind and OS code internally; classify it as `configuration_lock_acquire`, never retry it. Mark only TryLockError::WouldBlock with a typed content-free busy marker. Configuration compare-and-swap conflicts are not that marker. Add an explicitly nonblocking configuration begin for retrying MCP operations; local edit-mutex contention is also typed separately from poisoned state, not disguised as an OS lock error.
2. A run-scoped service clone carries cancellation; an operation-scoped shared retry budget is created for readiness or a current-credential read. At most 6 busy acquisition attempts across the operation, backoff 10/20/40/80/160ms, total scheduled wait at most 310ms, absolute retry window 500ms from first busy. Poll cancellation at most every 10ms. No wait on the UI thread; existing caller boundaries are workers. No busy loop and no retry of configuration reconciliation after it has begun mutating state.
3. Check the shared budget before every acquisition, including after backoff and in nested reads. Once exhausted, retain an immutable terminal error with actual busy count and cumulative scheduled wait; do not cap counters to conceal further attempts. Serialize the precheck/acquisition/busy-accounting transition with the budget mutex, never hold it during backoff. Uncontended initial acquisition starts no deadline and schedules no wait. Acquire/attempt registry and configuration locks in the existing order; on confirmed acquisition busy return before any configuration transaction, release all guards, then wait outside the registry, edit mutex, store transaction and write permit. Reacquire and revalidate on every attempt. Nonbusy failures and final exhausted busy retain existing connection invalidation. Startup secret cleanup obtains its configuration guard through the same budget before executing cleanup once; do not replay cleanup.
4. Preserve opaque busy metadata (attempts and cumulative wait) on CredentialReadError and in preparation/run diagnostics, including a clear UI busy-retry message. Successful recovery emits content-free operational diagnostics. No raw error messages, paths, config or credentials logged.
5. Do not redesign general model/tool retries or remove security checks. No historical call replay, stale credentials, restored revoked leases, automatic configuration edits or lock-file deletion.

## Implementation plan

- yaml_config: typed busy identification, preserved syscall errors, nonblocking edit acquisition.
- mcp_config/mcp_servers: carry acquisition busy distinctly; do not confuse YAML publication conflicts with lock acquisition.
- mcp_settings: operation budget and cancellation, retry outside registry/transaction; readiness and frozen credential readers reuse the correct scope.
- app worker/manual context: supply existing cancellation tokens to run-scoped MCP service clones.
- credential/runtime/store/submission diagnostics and UI/i18n: carry content-free busy details and actionable exhausted feedback; additive migration for run metadata.
- Synthetic development regressions exercise actual file locks and Store coordination; freeze remote source/results, remove Rust tests before final-head required Actions.

## Recovery and compatibility (2026-11-07)

- Original development head: `00b05e1242ca45489e6a0e66a56b3a381825e1b3`. Actions [41973313123](https://github.com/puzige/vega/actions/runs/41973313123): Clippy passed; Rust 9 executed, 1 passed, 8 failed, 0 skipped. All eight service regressions failed at fixture initialization (`reconcile_declarations`, line 25) with InvalidData before reaching contention assertions. The fixture used a macOS temporary path containing a symlink parent; production configuration checks reject it. Canonicalize the fixture root, as the already-verified #903 fixtures do. Do not relax production path checks or original assertions.
- Rebased onto `d4beacac2f01d4c09af7545163393e9d43753fe9`, including #903/#904 and #891. Preserve the fast unchanged-projection read path and only open the existing write transaction after a configuration change is established. Preserve #903 poison/parse/read/unsafe classifications and historical healthy-process continuation. The #887 migration follows the immutable #903 migration as `0060_configuration_lock_diagnostics.sql`; it retains prior diagnostic reasons and adds the new lock-acquisition reason.
- Only typed acquisition-busy marks retry. Other syscall errors keep their source internally and receive bounded stage/kind diagnostics without path, content, raw error text or source serialization. WouldBlock in a generic syscall error or CAS conflict is not the typed busy marker.
- Added regressions cover edit-mutex busy/poison separation, prior-reason migration preservation, invalid MCP declaration repair, changed-invalid configuration after lock release, and nonbusy syscall diagnostic mapping. The original OS-code assertion remains against the retained source.

## Status

Development validation passed at `6007cc4de285825332ccc8cf794eccc62a9766c2`: [Actions 42329042987](https://github.com/puzige/vega/actions/runs/42329042987), Clippy and required check success; Rust 16 discovered / 16 executed / 16 passed / 0 skipped; Python suites 5 / 15 / 40 passed. Earlier recovery runs 42326277870 and 42327490471 each executed 15 tests with 13 passes / 2 failures; original evidence and responsibility-layer corrections are recorded in PR comments. No failed regression was dropped to obtain success.

Recoverable remote tag: `evidence/887-config-lock-tests-6007cc4d`, verified via GitHub contents API. Source SHA-256:
- `crates/vega_conversation/src/mcp_settings/configuration_retry_tests.rs`: `eb110a293bc42369ab2ea475846e8046dd32d60ee7c4b0f9b2d49dd0084fb312`
- `crates/vega_store/src/yaml_config/configuration_lock_tests.rs`: `5ccee49cf96899b99e6ec02a1782a03e7dfed9477b248b3cb408bb2677af80a2`

Synced baseline `922cabfe5c1dbf2b19bff9ee75e8afc3c7ac9d75` (#910/#911 unrelated UI); range-diff confirms all four task commits identical. Rebased development head before cleanup: `f411c4304b9fcf1fabbbf20b009fbbb58a0cd6e6`. Cleanup removes exactly the two test files and two cfg(test) module declarations; containing production files are otherwise byte-identical. No test-exclusive dependencies or manifest changes exist. Final no-Rust-test head required CI remains pending; local formatting/diff checks pass, static Rust test entry scan finds none. No local Vega execution, compilation, Clippy or tests. Original incident remains unproven as real contention. No user runtime data changed.

User explicitly prohibits subagents for this run. No subagent is used, and no independent review result is claimed. The implementation writer's own inspection cannot satisfy independent review. Keep the draft unmerged until a non-writer review of the exact frozen version is available. Development test evidence must be retained before cleanup; final no-Rust-test head still requires full CI. Issue stays In progress, not In review or Done.

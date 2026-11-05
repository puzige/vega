# MCP configuration-lock bounded recovery

## Evidence and scope

Issue #887. Baseline `a818e320aaa6343e6e9b6f5391007c817dbd9dc6`. Fault-version source `8a949924dfd00f8b63e69f34a015f2e5b2fb5625` (v0.1.388).

Two private diagnostic observations concern MCP/configuration/I/O other: a running request boundary on November 4 and submission launch on November 5. No raw locking error was retained. Standard-library lock contention and other lock errors are collapsed by yaml_config::begin; WouldBlock is then collapsed by runtime diagnostics. A process-wide edit mutex already serializes normal same-process editing. Subsequent successful preparations in the same process contradict permanent mutex poisoning. These facts do not establish the original OS failure or another lock holder. The remote process has since restarted/upgraded. Do not access or mutate remote sessions, configuration, credentials or databases for implementation/validation.

## Acceptance before implementation

| Scenario | Observable result | Verification |
|---|---|---|
| Uncontended | First attempt succeeds, no added sleep | Cloud synthetic regression |
| Real external file lock released in budget | Current configuration reread; one operation completes | Cloud cross-process fixture |
| Lock never released | Bounded attempts and elapsed wait, explicit busy diagnostics; no downstream execution | Cloud regression |
| Cancellation during wait | Stop within one polling interval plus scheduling; no late retry or side effect | Cloud regression |
| Other lock syscall error / malformed YAML / permissions / CAS conflict | Original classification, no busy retry | Cloud regression + static audit |
| Configuration changes while waiting | Reconcile latest bytes and revision; existing revoke/disable rules remain | Cloud real store fixture |
| Active authority / running request | Retry before error invalidation; no model/tool replay; final failure remains fail-closed | Cloud service fixture + independent review |
| Nested startup reads | One shared budget for the readiness operation, not multiplied per call | Cloud regression |

No local execution/build/test. Existing Actions only. No private data in fixtures or public evidence.

## Design

1. Preserve TryLockError::Error as its original I/O error. Mark only TryLockError::WouldBlock with a typed content-free busy marker. Configuration compare-and-swap conflicts are not that marker. Add an explicitly nonblocking configuration begin for retrying MCP operations; local edit-mutex contention is also typed separately from poisoned state, not disguised as an OS lock error.
2. A run-scoped service clone carries cancellation; an operation-scoped shared retry budget is created for readiness or a current-credential read. At most 6 busy acquisition attempts across the operation, backoff 10/20/40/80/160ms, total scheduled wait at most 310ms, absolute retry window 500ms from first busy. Poll cancellation at most every 10ms. No wait on the UI thread; existing caller boundaries are workers. No busy loop and no retry of configuration reconciliation after it has begun mutating state.
3. Acquire/attempt registry and configuration locks in the existing order; on confirmed acquisition busy return before any configuration transaction, release all guards, then wait outside the registry, edit mutex, store transaction and write permit. Reacquire and revalidate on every attempt. Nonbusy failures and final exhausted busy retain existing connection invalidation. Startup secret cleanup obtains its configuration guard through the same budget before executing cleanup once; do not replay cleanup.
4. Preserve opaque busy metadata (attempts and cumulative wait) on CredentialReadError and in preparation/run diagnostics, including a clear UI busy-retry message. Successful recovery emits content-free operational diagnostics. No raw error messages, paths, config or credentials logged.
5. Do not redesign general model/tool retries or remove security checks. No historical call replay, stale credentials, restored revoked leases, automatic configuration edits or lock-file deletion.

## Implementation plan

- yaml_config: typed busy identification, preserved syscall errors, nonblocking edit acquisition.
- mcp_config/mcp_servers: carry acquisition busy distinctly; do not confuse YAML publication conflicts with lock acquisition.
- mcp_settings: operation budget and cancellation, retry outside registry/transaction; readiness and frozen credential readers reuse the correct scope.
- app worker/manual context: supply existing cancellation tokens to run-scoped MCP service clones.
- credential/runtime/store/submission diagnostics and UI/i18n: carry content-free busy details and actionable exhausted feedback; additive migration for run metadata.
- Synthetic development regressions exercise actual file locks and Store coordination; freeze remote source/results, remove Rust tests before final-head required Actions.

## Status

Development implementation is ready for independent review and Actions validation. Nine synthetic development regressions cover external process file-lock release/exhaustion, cancellation, no-wait registry/SQLite resources, latest configuration projection, shared budget, startup readiness, malformed/unsafe configuration, CAS conflict, OS-error preservation, diagnostic roundtrips and deferred authority invalidation. These regressions have not run locally. Local `cargo fmt --all -- --check` and `git diff --check` pass; no build, Clippy or tests executed locally. Original incident not dynamically reproduced. No remote machine access during implementation. Issue remains In progress until implementation, independent review and CI complete; after merge In review pending user manual validation.

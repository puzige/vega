# MCP registry re-entry after YAML configuration migration

## Contract and source evidence

- Regression task: #828; introducing PR: #824; reported app: 0.1.355.
- Baseline: `b2c18b2c6d95c146d8abbaafa38cf93818699707`, fetched and fast-forward checked before changes.
- Branch: `feat/828-mcp-registry-deadlock`; independent task worktree.
- User process samples report five agent workers and the MCP settings worker blocked on the shared registry mutex. Samples are user-provided evidence, not reproduced locally.
- Source confirms `step_up_offers` acquires `self.lock()`, collects bound challenges and calls `reject_owner_secret_in_ui` without releasing its guard. That helper calls `store()`, which now acquires the same non-reentrant mutex for YAML/DB declaration reconciliation and revocation. An empty challenge set still calls the helper.
- This shared lock also gates `ready_for_run -> reconcile_pending -> store` and the credential-reader/redaction path through `enabled_known_credentials -> store`. Synchronous mutex wait cannot be interrupted by the surrounding asynchronous cancellation branch.

## Minimal implementation

Release the `step_up_offers` registry guard explicitly immediately after collecting the owned offer snapshot, before sorting and before calling credential rejection. Keep every enabled/deleting/auth-mode/revision/epoch/suspension filter, deterministic sorting and owner-secret rejection unchanged. Keep `store()` reconciliation and its success/error revocation behavior unchanged.

Audit only the service's registry-held paths directly affected by `store()` taking the registry lock: direct store callers, reconciliation callers, UI-secret rejection callers, credential-reader callbacks and registry callbacks. Existing `drop`/scope boundaries remain; do not refactor unrelated locking or IO.

No new dependency, UI change, migration, cancellation redesign or production comment. Do not disable MCP, short-circuit empty offers past security, weaken leak checks, clear running UI state, or access/change real user configuration, credentials or databases. Do not open the running app's MCP settings during this task.

## Acceptance and verification

1. Initial MCP settings load and refresh finish, including zero step-up offers.
2. Sending conversations and tool description/result credential redaction continue after settings queries.
3. Shared-service concurrent workers do not deadlock on this re-entry; cancellation can finish after a real run is cancelled.
4. Owner-held credentials remain rejected at the UI boundary. Configuration changes still reconcile and revoke previous authority.

Local checks are limited to diff, format and static inspection. Do not run Vega, compile, build, package or execute tests locally. Use only the existing GitHub Actions PR workflow for automatic validation. Add narrowly scoped development-only Rust regressions if feasible for bounded settings completion and shared-service credential/readiness progress, preserving security assertions. Pin their accessible source SHA and actual cloud results, then remove all test-only support before merge. Final head must have no Rust tests and pass the existing complete required check `check (fmt, clippy, test)`; empty Nextest means `no Rust tests` (0 discovered/0 executed), not behavioral test success. No workflow changes.

Independent non-implementer review binds the exact stopped implementation content. Latest final head CI must succeed before squash merge. Keep #828 open and In review after merge pending user manual acceptance.

## Manual acceptance and rollback

After installing a cloud-built version containing the merge, the user should load and refresh MCP settings first without OAuth offers, send a normal conversation, run an available MCP tool and verify description/result redaction, run several conversations concurrently, then cancel an active tool/run and confirm it finishes. With a disposable test server/configuration, verify owner-secret rejection and that changing/disabling an MCP declaration revokes old authority. Never paste real secrets into prompts or logs for testing. Record which installed version was tested; CI/merge alone is not installation or user acceptance.

If necessary, revert only this task's production change via the normal PR process; reverting reintroduces the known deadlock. Do not clear or migrate user data as rollback. Restart remains only a temporary operational recovery, not a fix.

## Change record

- Initial specification committed before code changes. Validation and delivery evidence will be recorded in the PR/Issue with exact source versions and Actions links.

# Subagent startup diagnostics delivery

Issue: [#487](https://github.com/puzige/vega/issues/487).
Specification: [frozen acceptance v1.1](vega-issue-487-subagent-startup-diagnostics.md).
Production baseline: `ae35310fe7e1ddac2b79d7ec7bc55bcca2d6af2a`.

## Test-first phase

Status: effective RED reviewed by Codex, followed by production implementation.
The same frozen six tests await Actions GREEN. The first Actions run stopped
at test compilation wiring, before discovery or execution; its correction is
recorded separately below. No historical test suite or dependency was restored.

| Acceptance | Development test | Expected observation |
| --- | --- | --- |
| R1 | `credential_loading_does_not_lock_parent_receipt_persistence` | Real child preparation waits at the credential reader while the real parent terminal event persists successfully. On the baseline failure, an actual competing store transition supplies numeric SQLite codes for the assertion; the reader is released before asserting. |
| R2 | `child_starts_only_after_its_spawn_receipt_is_durable` | The same parent spawn receipt is stored before child preparation and provider entry; the parent waits for one child result and both histories retain completion. |
| R3, R5 | `rejected_spawn_storage_closes_diagnostics_without_starting_child` | A real terminal-update trigger produces SQLite primary 19, extended 1811. Parent and tool diagnostics close truthfully, live/reopened failures agree, and no child preparation/provider starts. |
| R4 | `pre_receipt_stop_and_admission_failure_leave_no_owned_workers` | A stop drains the admitted generation; an ordinary admission rejection remains a failed tool result the parent handles normally. |
| R5 | `strict_spawn_audit_failure_is_safe_and_durable` | A stored audit violation remains an approval-audit failure in live state, diagnostic export and reopened ownership. Raw sentinel text stays out of these surfaces; arbitrary tool prose cannot manufacture a diagnosis. |
| R6 | `schema_upgrade_preserves_unknown_failures_and_rejects_untrusted_runtime_details` | A real schema-27 database upgrades without inventing details for unknown failures; closed metadata and SQLite pairs are enforced by storage and read/export validation. |

The provider substitute is limited to the existing provider boundary. The UI
host adapter records admission and events and supplies explicit stop/reject
cases. The runtime, supervisor, child creation, preparation, event persistence,
SQLite engine, recovery and export are production code. Storage fixtures and
trigger faults belong to each test's temporary directory. R1 uses channel
coordination with bounded failure timeouts; it does not rely on sleep or an
operating-system race to hold the child credential boundary.

The new JSON/storage fields are `runtime_stage`, `runtime_reason`, `sqlite_code`,
`sqlite_extended_code` and `io_kind`. The failure cases require stage
`tool_terminal` with reason `database` or `approval_audit`. Legacy details remain
null. Tests use existing APIs and dynamic JSON lookups so absent detail fields
produce behavioral failures without requiring a new production interface.

## Verification evidence

- Local build, Vega execution, Clippy and tests: not run, per repository rules.
- `rustfmt --edition 2024` on both new Rust files: exit 0.
- `cargo fmt --all -- --check`: exit 0.
- `git diff --check`: exit 0. Static inventory contains six test definitions;
  no new dependencies, test scheduling seams, source-string assertions,
  sleeps, ignored tests or Rust comments were added.
- Initial test-first commit:
  [`430838d76ae94893062b9339237637f3591ca44d`](https://github.com/puzige/vega/commit/430838d76ae94893062b9339237637f3591ca44d).
- Initial source SHA-256:
  `subagents/tests.rs` =
  `8d8939f42e8ff5bc977493711f9a6ff9ab2b0c72f4a5dc49b530c9502c9a71a0`;
  `run_diagnostics/tests.rs` =
  `233b433703b4bc26e8f12f0591cd3b43b57b75e8fef6680bb244338c6cc5f5a4`.
- First Actions attempt:
  [run 39195618444](https://github.com/puzige/vega/actions/runs/39195618444),
  [Nextest job](https://github.com/puzige/vega/actions/runs/39195618444/job/117877626900)
  and [Clippy job](https://github.com/puzige/vega/actions/runs/39195618444/job/117877627016).
  The store test passed `usize` to `pragma_update` and read `usize` from
  `PRAGMA user_version`; rusqlite does not implement the required integer
  conversions. Rust test discovery and execution did not complete, and no
  discovery/execution counts were produced. This is a test-wiring failure,
  not effective RED or root-cause evidence.
- The wiring correction uses checked `i64` conversions for the migration index
  and current migration count, and reads both schema versions as `i64`. The
  six cases, triggers, expected values and acceptance meaning are unchanged.
  Corrected source SHA-256: `subagents/tests.rs` remains
  `8d8939f42e8ff5bc977493711f9a6ff9ab2b0c72f4a5dc49b530c9502c9a71a0`;
  `run_diagnostics/tests.rs` =
  `ca158e55e2c9babecd1454d982b135f573864a96a326d1fc90105295ccc70669`.
  After this correction, rustfmt, workspace format check and diff check each
  exited 0; no local compilation or tests ran.
- Effective RED after the wiring correction, before production implementation:
  commit
  [`eaf77b4b359dacea8b2a1753a4674110a60a9d08`](https://github.com/puzige/vega/commit/eaf77b4b359dacea8b2a1753a4674110a60a9d08),
  [run 39196140806](https://github.com/puzige/vega/actions/runs/39196140806),
  [Nextest job](https://github.com/puzige/vega/actions/runs/39196140806/job/117879223324),
  [Clippy job](https://github.com/puzige/vega/actions/runs/39196140806/job/117879223149).
  Six discovered/executed, one passed, five failed, zero skipped, 14 binaries.
  The parent writer returned actual SQLite primary/extended codes `5`/`5`
  while the child credential reader held the transaction. Child preparation
  independently observed parent spawn state `running`, rather than `success`.
  The storage/audit cases retained generic `Runtime`; migration lacked
  `runtime_stage`. Stop and ordinary admission rejection passed. Format and
  all-target Clippy succeeded; Python checks passed 5 + 15 + 14 = 34 cases.
- Production implementation began only after Codex reviewed those results and
  explicitly authorized phase 3. No test expectation or test source changed.
- Same-test GREEN, independent test cleanup and the final empty Rust gate:
  pending Actions and subsequent reviewed delivery phases.

## Production implementation awaiting GREEN

- Child startup waits on a conversation-private receipt signal. The event
  processor releases the matching, non-reused successful call only after
  persistence and event conversion succeed, immediately before its ACK.
  Cancellation wins the wait and uses the existing durable terminal finalizer.
- Preparation reads owner credentials before acquiring its immediate write
  transaction. Legacy redaction and history validation remain transactional;
  provider-boundary credential rereads remain active.
- Runtime failures carry closed stage/reason/IO categories and numeric SQLite
  codes without retaining SQL, paths, tool bodies or raw exception text. Actor
  and acknowledgement boundaries preserve the original typed failure. Failed
  critical tool events close their diagnostic attempts with that same detail.
- Nullable detail columns use additive migration
  `0028_runtime_failure_details.sql`. Insert, read, export and failure ownership
  use the same fields; legacy unknown rows remain null and cannot acquire an
  invented database diagnosis. Existing token/item column order is preserved.
- Current failed-run messages distinguish busy storage, save failure, audit,
  state, IO and worker failure. SQLite 5/6 asks the user to wait for other tasks
  and retry; other storage errors do not assert disk/permission causes.

Local static review at 2026-10-19 03:59:42 UTC / 11:59:42 Asia/Shanghai checked
the three complete diagnostic SELECT lists: each has 36 columns in identical
order, including unchanged item-count positions 28/29 and new positions 31–35.
Rustfmt, `cargo fmt --all -- --check` and `git diff --check` exited 0. Both
frozen test SHA-256 values still match the corrected test-first source. No
local Vega execution, compilation, Clippy or test execution occurred.

The existing automation usage-persistence API returns a string rather than a
typed cause. This patch replaces that actor error text with a fixed safe
message and retains the generic unknown fallback; it does not parse the string
or invent a SQLite cause. Remaining transactional history/checkpoint work can
still contend with other writers; this patch does not increase timeouts or
retry mutations. Migration numbering must be reconciled if another branch adds
the next schema version before integration.

Historical incident classification remains unknown. The causal RED established
the code-level contention and launch-order defects; it cannot recover the old
run's unrecorded original SQLite subtype. The implementation remains unverified
until the same frozen tests execute successfully in Actions.

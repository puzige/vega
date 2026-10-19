# Subagent startup diagnostics delivery

Issue: [#487](https://github.com/puzige/vega/issues/487).
Specification: [frozen acceptance v1.1](vega-issue-487-subagent-startup-diagnostics.md).
Production baseline: `ae35310fe7e1ddac2b79d7ec7bc55bcca2d6af2a`.

## Test-first phase

Status: tests written; review and Actions RED pending. Production behavior has
not changed. The only existing Rust changes are conditional module declarations
for these new development tests. No historical test suite or dependency was
restored.

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
- Candidate source SHA-256 before main-agent freeze:
  `subagents/tests.rs` =
  `8d8939f42e8ff5bc977493711f9a6ff9ab2b0c72f4a5dc49b530c9502c9a71a0`;
  `run_diagnostics/tests.rs` =
  `233b433703b4bc26e8f12f0591cd3b43b57b75e8fef6680bb244338c6cc5f5a4`.
  Test-first commit remains pending main-agent review/commit.
- Effective RED, causal numeric SQLite evidence, run/job and exact counts:
  pending Actions.
- Production implementation, same-test GREEN, evidence retention, independent
  test cleanup and final empty Rust gate: not started.

Historical incident classification remains unknown. A successful causal
reproduction establishes the code-level contention mechanism and its repair;
it cannot recover the old run's unrecorded original SQLite subtype.

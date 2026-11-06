# Validation evidence

## Development behavior validation

Source commit: `eb11d32e45cf31f201c946971f4cedcee69f1830`.
[Test source](https://github.com/puzige/vega/blob/eb11d32e45cf31f201c946971f4cedcee69f1830/crates/vega_conversation/src/agent/subagents/steering_tests.rs)
was read back via GitHub contents API before cleanup (blob
`8e84e48f295a1f090f4380bb320ee39aff15db2d`). SHA-256:
`f73e95b8f6f8c8f8487050acb7b572579e025f566e6e8c12036479577ef962d1`.
The PR commit remains the recovery source; no dependence on temporary artifacts.

[Actions run 42147169515](https://github.com/puzige/vega/actions/runs/42147169515):

- Clippy job `127661341624`: format and all-target Clippy passed.
- Nextest job `127661341988`: `cargo nextest list --workspace --message-format json`
  discovered 4; `cargo nextest run --workspace` executed 4, passed 4, failed 0,
  skipped 0. Runtime 1.085 seconds in Nextest's summary.
- Required `check (fmt, clippy, test)` job `127662921171`: passed.
- Python suites: 5 + 15 + 40 tests, all reported OK.

Behavior cases:

1. `two_waits_accept_pending_steering_once_before_child_completion`.
2. `three_repeated_waits_accept_steering_with_coalesced_deadlines`.
3. `ineligible_queue_entries_do_not_wake_and_cancellation_still_interrupts`.
4. `no_message_preserves_timeout_coalescing_and_released_terminal_results`.

The first two use the real runtime batch, production wait hook and persisted
QueueSteeringSource. They observe an empty queue before inserting steering,
assert all valid receipts and their coalescing, next-provider-request message
ordering and exactly-once content, claimed state, later pending wait and new
steering. Provider transport is scripted in-process; child is a held registry
fixture, not a real model/command child. The tests assert running/held state and
an uncancelled child token throughout steering. No live UI or external-agent
integration is claimed.

## Failures and corrections (assertions retained)

- `7c6de96b304799bb8aa65159ade810a4014e268c`, run `42144960525`, Nextest
  `127654086563`: 4 executed, 0 passed, 4 failed, 0 skipped before behavior,
  `no such table: projects`. Fixture omitted explicit `Store::migrate`.
- `0ada1f1752eb7b3bcafb92c9fc571be642929c32`, run `42146143839`, Nextest
  `127657972067`: same counts, fixture `capture_fence` failed
  `InvalidTaskIdentity`. PermissionFence requires ULID thread IDs; fixture used
  `parent`. Replaced parent references consistently with a valid ULID.
- Both failures were fixture precondition defects, not behavioral passes or
  evidence that product acceptance failed. All assertions were retained; no
  ignore, retry, deadline relaxation or product-authority bypass was used.
- Integrated upstream `6240623d72f04b1bde2fa17219f4358bf0ffd2e8` without conflict.

## Review and final delivery

Non-writer review of `7c6de96b304799bb8aa65159ade810a4014e268c` found no issues;
review of initialization/integration delta at
`0ada1f1752eb7b3bcafb92c9fc571be642929c32` found no issues.
Final ULID/cleanup delta review and final-head required CI are tracked in PR #898
and Issue #897. No local Vega execution, compilation or test execution occurred;
local checks are format and diff only.

Per current repository policy, the four development Rust tests and their sole
module declaration are removed after preserving this passing source and run.
No test-only dependency or fixture outside that file was added. Production fix
is unchanged by cleanup. Final zero-test CI is separately required and must not
be described as these behavior regressions passing again.

User real-App manual acceptance remains pending; Issue stays open until accepted.

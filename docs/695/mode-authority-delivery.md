# Mode authority implementation and evidence

## Implementation

Base: `9cf826730bc3f6951eb165c1b8e66f75fba3e63d` (includes #700). Production implementation: `79cb831bd6694874748080aab8654e7b8361315a`. PR #715.

Selected permission mode replaces default/thread grant authorization. Legacy grants are ignored on new default/thread reads and cleared on permission saves. Settings saves directly; draft selection remains in memory. Draft authority epoch preserves reset invalidation independently of grants. New execution fences use policy version 2, rejecting old execution recovery while retaining identity, revision and delegated-parent checks. FullAccess Execute approves after capability eligibility without danger confirmation; other modes and independent capability approvals remain unchanged.

Independent non-implementer static review at the implementation SHA found no actionable defects. UI implementation was separately scoped; the reviewer wrote none of the implementation. Static review does not establish native UI or real execution acceptance.

## Cloud development regressions

Recoverable source: [evidence/695-mode-authority-regressions](https://github.com/puzige/vega/tree/evidence/695-mode-authority-regressions), read back at `7125f63771d30cfc11120c11e44731425aab4b55`.

Files: `crates/vega_conversation/tests/mode_authority.rs` and `crates/vega_runtime/tests/mode_authority.rs`. No additional dependency or exclusive production test wiring.

- First run [40630077593](https://github.com/puzige/vega/actions/runs/40630077593): 4 passed / 3 failed. Database fixtures omitted explicit `Store::migrate`; raw error included missing permission_contract_state. Fixed fixture initialization only.
- Second run [40631041060](https://github.com/puzige/vega/actions/runs/40631041060): 6 passed / 1 failed. Fixture attempted non-JSON in a JSON-constrained grant column. Changed to valid JSON `{}` which remains an invalid grant. Assertions and production code unchanged.
- Passing run [40631649670](https://github.com/puzige/vega/actions/runs/40631649670): required summary and Clippy successful; Rust 7 discovered / 7 executed / 7 passed / 0 skipped. Python 5 + 15 + 40 passed.

Coverage: missing/malformed default grant, invalid legacy thread grant, draft non-materialization, stale epoch rejection, direct default save and stale change conflict, old policy rejection, FullAccess approval audit, other mode decisions, Ask/Plan capability rejection. This is not real tool execution or desktop acceptance.

The two development test files are removed after evidence preservation. Final-head required CI must independently confirm no Rust tests (0 discovered / 0 executed); that result is not functional regression coverage. Local checks are formatting and whitespace only, with no local compilation, tests or application execution.

## Delivery boundary

User data and installed app were not changed. Merge is not installation or acceptance. Keep #695 open and In review after merge until explicit user hand-testing. Rollback to older binaries may require selecting permissions again because old grant requirements return. Old execution snapshots must not silently resume under the broader policy.

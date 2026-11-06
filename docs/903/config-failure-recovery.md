# MCP configuration failure classification and safe recovery

## Scope and evidence

Issue #903. Baseline `852d4ebf14983d83e03b16b02e5cf099f60a1e92`. User authorized implementation and classification/recovery tests. #887 remains owned by draft PR #889 at `00b05e1242ca45489e6a0e66a56b3a381825e1b3`; its latest handoff is stopped. Do not edit that worktree, duplicate its retry loop, or claim it is delivered. Its Nextest failure occurs during fixture initialization with InvalidData, before lock-budget assertions; that is not evidence against its retry budget.

The original `mcp/configuration/io=other` observation does not identify poisoning or lock contention. The recorded 314.6 seconds is whole-run duration. This change does not claim to reproduce or cure the unknown original cause.

## Acceptance before implementation

| Scenario | Required result | Evidence |
|---|---|---|
| Valid uncontended configuration | Read actual bytes and succeed without new retry or mutation | Actions store/service regression |
| Panic while edit mutex held in an isolated test process | Next acquisition reports typed poisoned failure, never clears poison or reads cached data; UI asks restart | Actions fault injection + failure mapping |
| Lock open / metadata safety / directory / read / parse failure | Exact bounded stage and original I/O kind retained, no arbitrary error text or credential contents | Actions regressions and diagnostics roundtrip |
| Invalid YAML repaired, or unsafe lock replaced safely | Next explicit acquisition/service check rereads current bytes and validates; no fallback to earlier bytes | Actions recovery regression |
| File replaced by symlink / unsafe identity during recovery | Current safety checks still reject; no partial successful edit | Actions safety regression |
| Runtime and submission diagnostics | New reason values survive validators/storage and project to actionable failure messages; old diagnostic IDs/reasons survive upgrade, unknown reasons remain rejected | Actions roundtrip / migration regression |
| Restored poisoned failure after restart | Current poisoned process remains blocked; unpoisoned process restores Continue through normal fresh validation | Actions history assembly / mutex regression |
| Valid YAML with malformed MCP declaration | Typed parse fault and repair prompt, successful readiness only after actual repair | Actions service regression |
| Existing lock contention and CAS conflict | Not reclassified as poison; no new retry loop or replay of submissions/tools | Static diff; #887 contract remains responsible |

## Minimal implementation plan

1. Add a content-free configuration-failure marker at `yaml_config::begin/read` boundaries: poisoned edit lock, directory validation, lock open, lock validation, configuration read and parse. Preserve `io::ErrorKind`; expose only stage and kind. Never retain arbitrary source errors or paths in the marker.
2. Propagate this marker through `mcp_config` / `McpStoreError` and the direct run reconciliation boundary into typed credential reasons; extend existing diagnostic allowlists and the database CHECK constraint with a data-preserving atomic migration; add no raw content fields.
3. Project the reasons to restart-required, repair-required, and configuration-access failures. Preserve legacy diagnostic compatibility and completed effects. Keep the existing Continue route for explicit revalidation after external repair; do not offer Continue for a poisoned process. Submission failures use the same actionable message.
4. No automatic poison recovery: held operations include filesystem/DB projection work, and consistency after panic is not proven. Fail closed until restart. Repairable filesystem/YAML faults can recover only through a fresh normal acquisition and the existing reconciliation/authority validation path; no cache bypass, side-effect replay, or permission change.
5. Add isolated development Rust regressions at store, runtime/service and failure-projection boundaries. No new dependencies or local execution. Run through existing Actions, retain accessible source/patch and actual results, remove Rust tests/support before final-head required CI per repository policy.

## Coordination with #887

#887 owns `try_begin`, typed busy, syscall error preservation, cancellation and shared retry budget. This branch does not copy that implementation or tests. On integration, both blocking and nonblocking poison paths must use the same marker, and `mcp_config::io_error` must check typed non-busy faults as well as the busy marker before reducing to ErrorKind. No busy retry may match a typed non-busy fault. Rebase/integration must preserve both contracts and their required regressions; do not merge development tests into master.

## Non-goals and validation

No changes to user data/configuration/credentials, no deleting lock files, no clearing poison, no replacing locks to suppress errors, no general retry framework, no #887 takeover. Local checks only formatting/diff/static inspection. Automated runtime tests only in existing GitHub Actions. Independent non-writer review and final required CI are mandatory; user manual acceptance follows merge. No dynamic check has run at spec creation.

## Development candidate

Ten development regressions cover real parse/lock-identity/read/directory failures, explicit repair with fresh bytes, CAS preservation, isolated mutex poisoning, actual MCP readiness recovery, runtime/submission diagnostic storage and UI retry eligibility. Nextest process-per-test isolation is required for the poisoning case; do not run a shared-process libtest suite. Fixtures canonicalize the macOS temporary root, unlike the failing #887 fixture, without weakening filesystem safety assertions. `cargo fmt --all -- --check` and `git diff --check` pass locally; no local build/runtime/test executed. Development head `796b9000e5164fca3c56fe4d3ad143adc98edcd2` run https://github.com/puzige/vega/actions/runs/42175295587: Clippy passed, Nextest 9 executed / 8 passed / 1 failed. The diagnostic roundtrip exposed a product omission in the database CHECK constraint, not a test defect. Follow-up adds migration 0059, preserves old IDs/reasons and rejects unknown values. Independent review found historical restart continuation and declaration parse boundaries; both are corrected with affected regressions. Follow-up development head `f2ae36084ef41d8bf523164f2c387a0e8cb61e22` passed [Actions 42177270889](https://github.com/puzige/vega/actions/runs/42177270889): 10 Rust tests discovered/executed/passed, 0 failed/skipped; Python groups 5/15/40 passed; fmt, Clippy and required check passed. Independent incremental review completed with no new actionable findings. The original incident cause remains unknown; no claim of automatic poison repair or original-site reproduction.

## Final cleanup candidate

Development sources remain accessible at [evidence/903-config-recovery-tests-f2ae3608](https://github.com/puzige/vega/tree/evidence/903-config-recovery-tests-f2ae3608), verified remotely to the full development SHA above. Source SHA-256: store `9a2ccbc477ab8fbd3640d43bb3f04f8704eb6fa49646dd1e68a4982cd628223b`, conversation `27c9c64ca16fb69e79dc7963549b8a867b48f580e31a9afda0d89203452d4a4c`, UI `d67a1db2eec62154afb51032d917c3d09a4a51829ea667786f86c7c1a13a8e7b`.

Synchronized master `c74da033ed5dbbcab8c748078967760c571a4547` (#891 diagnostic availability); range-diff confirms both implementation commits are unchanged. Removed exactly the three development test files and their three module declarations; production bytes are otherwise unchanged from the rebased implementation. No manifest, dependency, Python, workflow or Nextest configuration changes. Local fmt/diff checks pass; static `crates/` scan finds no Rust test attributes or test paths. No local Vega execution, compilation or tests. Final cleanup/base-delta review and full final-head required CI, including Rust 0 discovered/0 executed, remain pending; actual final SHA/results are recorded in PR #904 and Issue #903.

## User manual acceptance after merge

Use a released build containing the merge commit; merging does not install the app. First confirm a normal read-only task can run and existing MCP settings/history remain available. Reopen the original failed conversation after a normal full quit/restart, then use Continue if offered; fresh validation may still block for a real configuration fault, but completed operations must not replay. Old `configuration/other` records cannot retroactively identify the original cause.

For any newly occurring failure, capture the actionable notice, app version and exported diagnostic reason: poison requires restart and no Continue in that process; repairable configuration/declaration faults require actual repair and explicit recheck. Do not deliberately corrupt the live configuration, induce a panic, delete locks/databases or weaken permissions to exercise these branches. Fault-injection coverage is preserved in the development tests above. Report remaining failure evidence without credentials or raw configuration contents; user manual acceptance remains pending.

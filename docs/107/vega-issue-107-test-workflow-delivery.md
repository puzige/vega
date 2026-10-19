# Issue #107 — delivery record

## Pre-change inventory (2026-09-21)

- Baseline master: `95cb42d` (PR #105 concurrent conversations); PR #104 strict tool schemas and PR #102 background lifetime are also ancestors.
- PR #83 MCP: worktree HEAD `c47223f` has the exact same tree as merge `d25ff0d`.
- PR #84 Skills: worktree HEAD `780f009` has the exact same tree as merge `79a20c2`.
- Probe HEAD `19a1416` is already an ancestor of master.
- Four old intermediate MCP/Skills branches are superseded, but not byte-identical to the delivered versions. Their original tips and all history were saved in a verified Git bundle before local worktree/branch removal. They are archived history, not claimed as fully merged code.
- All seven removed worktrees were tracked/untracked clean and had no observed cwd owners. Only generated target symlinks and two dist bundles were ignored. Main checkout untracked IDE/research documents were preserved unchanged. Remote branches were not deleted.
- Still pending at inventory: PR #106 command elapsed display (`7b2d405`) and local #99 test stabilization (`6c4ccaf`). Kept intact pending integration decision; neither is represented as merged.

## Validation contract

See [spec and acceptance matrix](vega-issue-107-test-workflow.md). Developer-tooling changes require owned temporary repo/Cargo E2E, not product UI screenshots or app reinstall. No product Rust source change is in this card.

## Evidence

Persistent local evidence directory label: `test-workflow-2026-09-21` under Vega evidence storage. It contains the inventory, exact old branch snapshots, verified Git bundle, review notes and upcoming command logs. Raw local workspace paths are not published here.

## Implementation and reviewed boundaries

- `scripts/verify.py` computes changed packages and transitive workspace consumers, or tooling/docs gates. Unknown/global inputs require explicit full verification. Hook and CLI share source/config/environment identity, so push can reuse successful logs. Dirty/non-head pushes fail clearly.
- Coordinator uses two build permits, canonical target locks, persistent single-worktree cache ownership and a conservative repository test resource lock. `fmt` bypasses locks. Target and intermediate build-dir are pinned together. A guardian owns locks until Cargo exits; compiler daemons do not inherit descriptors.
- Existing shared targets are not adopted/relinked/deleted. Independent caches begin cold; compiler wrappers remain configured. Direct Cargo and old wrappers are outside coordination and must not be mixed with this workflow.
- `xtask` reads executable paths from successful Cargo JSON output instead of hardcoded legacy paths. No application behavior or installed bundle changes.
- README, AGENTS, execution guide, delivery skill and hooks use the same policy. No tests were deleted, ignored or weakened; no automatic retry-to-green.

## Pre-integration evidence

| Matrix | Evidence class | Result |
|---|---|---|
| T1 | E2E-REAL: two owned Git worktrees, real Cargo build intervals and executed A/B binaries | PASS, intervals overlap, outputs have correct identities |
| T2–T5 | FAULT-INJECTION: executable contention/signal/daemon fixtures through production wrapper | PASS, alias exclusion, capacity, test waiter, fmt bypass, signals and outer-wrapper death |
| T6–T8 | E2E-REAL: tiny Cargo workspace and bare Git remote through real hook | PASS, transitive scope, CLI-to-push reuse, dirty/non-head rejection, evidence invalidation, failed evidence rejection |
| T10 | E2E-REAL: legacy symlink/sentinel preservation, conflicting/nonempty target ownership | PASS |
| T11 | E2E-REAL: real release build reports managed executable and executable runs | PASS for Cargo boundary; parser additionally checked by focused xtask test in final gate |

Coordinator: 9 passed in 12.471 s (`coordinator-guardian-tests.log`). Verifier: 6 passed in 12.229 s (`verification-guardian.log`). No whole-project speedup is inferred from tiny fixtures.

Baseline RED: fmt was rejected by a live repository lock (`red-fmt.log`); scoped verifier was absent (`red-verifier.log`). Development failures remain in evidence, including inherited lock descriptors leaking into a compiler cache daemon. The guardian regression leaves a daemon alive after Cargo exits and verifies the next command acquires permits.

Final T9 gate, xtask clippy/tests and actual push reuse are recorded by the frozen unified runner in local result/log manifests and summarized in PR/Issue #107. Gate output stays outside tracked source so recording it does not change the verified source identity. No unexecuted gate is marked PASS here.

## Limits and rollback

- Legacy test invocations remain serial across tasks; this enables bounded parallel builds, not arbitrary concurrent shared-state tests.
- Each worktree owns its cache; cold compilation and disk cost remain. Retain caches during active work; clean inactive owned caches after delivery.
- Local evidence reuse is an engineering convenience, not a signed security attestation. Unsupported global/config inputs fail closed; task-specific production-root acceptance remains required.
- Roll back by reverting this tooling PR and restoring the previous policy; never run both lock protocols concurrently. Installed Vega and user files remain unchanged.


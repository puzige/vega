# Proposal-only round Fork follow-up validation

## Incident and repair

User acceptance failed on 0.1.371, already containing the checkpoint-origin fix. Read-only incident inspection identified an interrupted historical assistant message with four proposed tool-round IDs and no corresponding persisted calls. An earlier round had one proposed, persisted, correctly owned call. The old round-owner count rejects the former; counting existing calls preserves the latter and rejects existing wrong-owner references. No remote database, checkpoint, application or process was modified.

`ToolRoundCompleted` independently persists proposal IDs before individual admission. The one-line production SQL change in `vega_store/src/thread_fork.rs` ignores absent calls only when building the new fork. It does not claim they executed, synthesize results, mutate source records or alter active-source, terminal, mutation-provenance or transactional guards.

## Development source and cloud result

- Base: `c467be956a558792de81aecd63a59dbfafc07505`.
- Development head: `e2c8db980b3e39ce4b6e27237da4ec5ee214c0e4`.
- Preserved remote tag: `evidence/851-fork-proposal-rounds-e2c8db98`, read back with `git ls-remote`.
- [Original test source](https://github.com/puzige/vega/blob/e2c8db980b3e39ce4b6e27237da4ec5ee214c0e4/crates/vega_conversation/src/fork_round_tests.rs), also verified via GitHub contents API: 7,550 bytes, blob `aa1fb7b99b2d96ff2ca69b7830c252cef116eaac`.
- SHA-256: `9f81ebd6db0c9e268daa709f4fcc0fa117f81674f62342c8b606f18321fff517`.
- [Actions run 41429318974](https://github.com/puzige/vega/actions/runs/41429318974): Clippy, Nextest and required `check (fmt, clippy, test)` all SUCCESS on that head.
- Nextest: **5 discovered / 5 executed / 5 passed / 0 failed / 0 skipped**, per actual run output. Three preserved Python suites ran 5, 15 and 40 tests successfully.
- Public conversation Fork regressions cover mixed admitted/proposal-only rounds, proposal-only rounds without fabricated executions, repeated Fork, preserved source round counts, foreign-message/thread rejection with no destination, and nonterminal-source refusal.
- Independent non-writer static review on the exact development head: no findings. No local Vega code execution, build or tests.

## Final cleanup and gates

Remove only the development test file and its two-line module declaration; no new dependencies or test-only support were added. Production SQL must remain byte-identical to the successful development version. Final local format/diff and static residual checks, independent cleanup-delta review, and final-head required CI remain required after cleanup. Final Rust 0 discovery / 0 execution is `no Rust tests`, not functional test success; successful development coverage above remains the behavioral evidence.

## User acceptance

PR #854 follows up issue #851. Do not close the issue until the user retries their original conversation on a release containing this repair. 0.1.371 and 0.1.372 do not contain it. Confirm that Fork succeeds after interrupted historical work, the new fork can send and fork again, running-state hiding and pending spinner remain unchanged, and no historical tools replay. CI and static inspection do not establish native UI or live integration acceptance. No remote upgrade/restart was performed by the agent.

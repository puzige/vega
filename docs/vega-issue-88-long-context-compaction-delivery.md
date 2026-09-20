# Issue #88 — long-history compaction delivery

Issue: https://github.com/puzige/vega/issues/88  
Spec: [#76 amendment](vega-issue-76-context-compaction.md#issue-88-amendment--long-tool-history-2026-09-20)

## Freeze and plan

- Baseline: `origin/master` at `79a20c2`; new isolated branch `feat/issue88-long-context-compaction`.
- Observed red case: an automatic compaction attempt at approximately 259K estimated input (configured input 300K) fails `too_large` before the summarizer because a completed history prefix with 122 tool calls serializes beyond 128 KiB. Original history and configuration are not modified for this task.
- Sequence: (1) add red reproduction for production conversation-to-runtime path, recording the expected failure; (2) implement bounded staged summarization and accurate error copy without schema/dependency change; (3) add L02–L04 fault/persistence checks; (4) run full workspace gates and test build; (5) native UI + real provider L05, inspect screenshot and preserve evidence outside the worktree; (6) review, squash merge, cleanup and close card.
- Build/test lock: `scripts/cargo-lock.sh`. Full gates: `cargo fmt --all -- --check`, `scripts/cargo-lock.sh clippy --workspace --all-targets -- -D warnings`, `scripts/cargo-lock.sh test --workspace`, plus packaged candidate and native E2E. Do not run concurrent Cargo against the shared target.
- Rollback: revert the #88 squash commit. Checkpoints remain append-only and raw transcript/tool rows remain authoritative; no migration expected.

## Acceptance matrix

The L01–L05 definitions and expected observations are frozen in the linked spec. The red production-path test first failed with `SourceTooLarge` before any summary request (exit 101, 0 passed/1 failed; `l01-red.log`). After implementation, the focused `issue88_` conversation tests passed (5 passed/0 failed): L01 long completed-call history continues in the same run; L02 a single oversized tool input/result is fully and chronologically staged, including an explicit empty-result marker; L03 later-stage failure, cancellation and source mutation do not install a partial checkpoint while preserving observed usage; L04 a restart projects the one durable checkpoint and does not replay a prior tool. The UI copy test distinguishing `too_large` and `over_limit` passed (1/1). L05 remains NOT RUN until an installed real-provider continuation is observed.

## Evidence

Implementation: `8617f6d` on the isolated branch; no dependency or schema change. The effective whole-source 128 KiB dead end is replaced by bounded summary requests, with a 64 MiB planning ceiling and at most 512 stages; the raw transcript remains authoritative and any exceeded safety cap fails explicitly rather than silently dropping data. The agent can resume in the same run after a successful final checkpoint.

Local gates (2026-09-20): `cargo fmt --all -- --check` exit 0; `./scripts/cargo-lock.sh clippy --workspace --all-targets -- -D warnings` exit 0; `./scripts/cargo-lock.sh test --workspace` exit 0, 35 test-result groups, 1,632 passed / 0 failed / 9 ignored. The focused red/green logs, these gate logs and native evidence are retained in the owner-only `vega-evidence/issue-88-2026-09-20.dCZjA0` directory outside disposable worktrees. The red log SHA-256 is `ac4a897feabbcd6ec7a7d66f63181d58ffea7e7c2832e8cc563a007ef646de02`; the focused green log SHA-256 is `f248007dd222f80336b1e4c400930582ad925d73a81c68b1c5aa6e50c8281314`. Packaging, installed native E2E, PR/integration and final screenshot manifest are pending. Private transcript content and paths are omitted here.

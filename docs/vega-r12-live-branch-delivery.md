# R12 B delivery — live sidebar branch suffix

## Freeze

- verified_at_utc: 2026-09-06T04:19:38Z; verified_at_local: 2026-09-06T12:19:38+08:00
- branch: `codex/r12-live-branch`
- git_head (tested implementation): `0a6d5fb816a0ceb16bc5c409de896d8a73fc3f8a`
- source_tree: `0d58a15024ee3e8f33536c9dab0d96b20347f4d0`
- staged implementation diff SHA-256: `103b45a9e9e700baa059957dd250fd5aca54f82f06490426458cef5d1ad06263`
- task_contract: `docs/vega-r12-live-branch.md` and R12 B in `docs/vega-r12-task-navigation.md`
- os_arch: Darwin arm64; rustc 1.98.0 (88d9e12ae 2026-08-18), cargo 1.98.0 (797e8a9bc 2026-08-05), Git 2.55.0.

## Implementation

ProjectsBlock now renders the service's current HEAD suffix and never the registration-time cached branch. It owns one cancellable metadata worker, a coalesced request/result mailbox, exact project ID/path/generation fences, global selection invalidation, actual mounted render probes before request and completion, and a 1-second UI expiry. Expanded visible rows refresh on a 2-second target cadence; larger lists rotate through bounded batches of 128.

The service only opens descriptor-relative, no-follow metadata paths with regular-file and 4 KiB limits. A linked worktree need not register its owner: only fixed `.git/worktrees/<entry>` topology and a reciprocal `gitdir` backlink authorize its HEAD read. No Git subprocess, Git write, migration, dependency, runner timeout, sidebar orchestrator, navigation or window edit was added. Existing registration-time detection is untouched.

## Results

All logs below are retained under `/private/tmp/` using the listed basename. Full logs contain only owned test fixture/build paths, never credentials or real user repository mutation evidence.

| requirement | evidence class | exact command | result | duration / bounded footer | log SHA-256 |
| --- | --- | --- | --- | --- | --- |
| Formatting | STATIC | `cargo fmt --all -- --check` | PASS | exit 0, empty output | `vega-r12-b-fmt-freeze.log`: `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| Strict workspace lint | STATIC | `cargo clippy --all-targets -- -D warnings` | PASS | Finished dev profile in 4.73s | `vega-r12-b-clippy-final.log`: `e1f4ab95969bec6f30e63c8f702d11b34d4a980fd42be1dfb03c8517388decdb` |
| Real HEAD worker and fences | E2E-REAL + UNIT/PROPERTY | `cargo test -p vega_conversation project_branch -- --test-threads=1` | PASS | 3 passed; 0 failed; 0 ignored; 0.22s | `vega-r12-b-service-freeze.log`: `824965d8c9f104f648d6226c7bd1b2f67a973ccd6339a3faf9df7a90ab5ad6c9` |
| Production mounted sidebar handlers and worker | E2E-REAL + UNIT/PROPERTY | `cargo test -p vega_ui sidebar::projects_block -- --test-threads=1` | PASS | 2 passed; 0 failed; 0 ignored; 2.39s | `vega-r12-b-ui-freeze.log`: `4d01c46669239777c46d9dc9678be8c182ca3e16257660dcae019180b5220608` |
| Workspace build | BUILD | `cargo build --workspace` | PASS | Finished dev profile in 5.64s | `vega-r12-b-build-final.log`: `77cee9a176317bb725f1d1d95040a5e62067548f438f5da35f02f7f9a1862411` |
| Runtime dependency direction | STATIC | `cargo tree -p vega_runtime --depth 1` | PASS | No UI dependency | `vega-r12-b-runtime-tree.log`: `6663118e0296dd53f6b7925fae4938ab257a9459058b67dd2dbf94ad09cb7350` |

Production service evidence uses a real file-backed migrated database and actual Git initialization/checkout/worktree commands. It covers main → other, a linked worktree after owner registration is removed, detached HEAD, wrong reciprocal backlink, non-Git/missing directories and cancellation/superseded requests. Security invariants separately reject symlink/FIFO/oversize HEAD, external pointer topology, invalid ref grammar and cancelled reads.

The GPUI E2E opens the actual ProjectsBlock as a test window, waits for the same suffix helper used by render, performs real external checkout and observes periodic update with no reload or injected label. It also exercises production project selection/reload, detached/invalid HEAD, and removal with a real in-flight request. A narrow unit seam verifies path mismatch rejection; it does not substitute the displayed label in the happy path.

## First failures retained

First strict lint run failed on one new `clippy::collapsible_if` in completion consumption. The nested generation check was collapsed without suppressing the lint. Original log: `vega-r12-b-clippy-first.log`, SHA-256 `7a1897f64a4e5b3178b3972f0ce2e9f1e78509ad4cd953c03a258fd54952c421`. Subsequent final source passed. First service and UI test runs passed; they are retained as `vega-r12-b-service-first.log` and `vega-r12-b-ui-first.log` and are not substituted for frozen evidence.

## Residuals

- LIMIT: Kernel filesystem calls can exceed the cooperative 250 ms service deadline; there is at most one worker, and the UI expires the batch after 1 second to clear stale labels. No hard filesystem preemption claim.
- LIMIT: Symlinked roots/metadata, nonstandard separate-git-dir layouts and ambiguous worktree topology intentionally yield Unknown. The fixed ordinary repo and Git-created linked-worktree paths are covered.
- LIMIT: Lists larger than 128 projects refresh in bounded round-robin batches, so a full-list cycle can exceed 2 seconds.
- NOT RUN: Native CUA, integrated navigation/menu acceptance and performance benchmarks. Native acceptance belongs to the integration owner; performance remains deferred.
- NOT RUN: Full `cargo test --workspace` in this card. The integration owner retains the known R11 baseline 925/3/0 Git lifecycle gate and performs the unified workspace run. This card does not claim that gate is fixed or green and does not alter its tests or runner.
- ACCEPTED: Existing `block v0.1.6` future-incompatibility warning remains in build/lint output.
- Spec deviation: none within the approved R12 B metadata-topology and cooperative-deadline contract.

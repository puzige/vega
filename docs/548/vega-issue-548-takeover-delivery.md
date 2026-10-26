# Question-wait takeover and delivery

Issue [#548](https://github.com/puzige/vega/issues/548), existing draft PR [#565](https://github.com/puzige/vega/pull/565).

## Authorized recovery checkpoint

The user explicitly confirmed the old execution stopped and authorized this conversation to take over. The coordination comment uses `question-wait-takeover-20261024-vega`. This is a collaboration marker, not an atomic lock. The original clean worktree and all source commits remain intact.

After fetching and safely updating local master, the parent created isolated branch `feat/548-question-wait-takeover` from original PR head `093c9756a0be9f4ef06a92e0ae18adf323a5d65a` and merged current master `21b9f974215c1b6e3ba2802e9f5699428106feeb` without conflicts. Stopped integrated code is `6978d3bdcb8938d829ab8d96e83e1567b818f16c`. The existing PR branch will receive only non-force, ancestry-preserving updates, retaining its review and cloud history. No other card is being taken over.

The previous Agent scheduling blocker is historical: the new independent read-only review completed READY for stopped code `6978d3bdcb8938d829ab8d96e83e1567b818f16c`, with no blocking production finding. It reviewed the whole feature and the affected upstream UI content/run-activity integration, preserving timing, continuation, stale-owner cleanup, child attention and foreground suppression. The reviewer verified documentation-only head `004992f27ffe1d5d0d61694b268168148ae1c60d` retains identical crates subtree `49a801c083ea362972a5190ea351ec5f15bb3be8`. No reviewer writes were made. This is pre-cleanup static readiness, not final merge or user acceptance.

## Unchanged contract and ordered plan

The acceptance matrix W1–W5 / U1 / N1–N3 and lifecycle clarifications in [the main spec](vega-issue-548-question-wait.md) remain authoritative and unchanged. Current repository risk-based validation policy supersedes historical mandatory RED/TDD wording. This continuation adds no product behavior or new dependency.

1. Independently review stopped integrated production code, preserving indefinite question waiting, accepted-answer timing, same-clock continuation, stale-owner cleanup, child question attention and foreground suppression.
2. Run the existing 28 developmental regressions in the existing draft PR Actions on the current integrated content. Earlier run `39934121113` remains evidence only for its original tested content: 28 executed/passed, 60 Python passed, fmt/Clippy/aggregate succeeded. It is not the current-final-head gate.
3. Only after review and current integrated functional validation, assign one dedicated writer to remove the two Rust test files and exclusive support listed in main spec §5. Preserve production WorkClock/time-source wiring, request-resolution callbacks, lifecycle ownership, UI state, subscriptions and notifications. Remove exclusive feature/dev-dependency/lock edges only. No production-equivalence shortcut, failing-test removal or assertion weakening.
4. Preserve accessible original source, complete commit identities, original cloud output and actual counts; independently review cleanup delta and audit no Rust test/doctest/support residue. Existing Python tests/fixtures, workflows, migrations, resources and benchmarks remain unchanged.
5. Run the complete latest cleanup-head required CI with explicit `no Rust tests` (0 discovered / 0 executed). Merge only after independent review and required success, then keep the Issue open and move to In review for user manual native acceptance.

## Current integrated functional evidence

[Actions 40120165360](https://github.com/puzige/vega/actions/runs/40120165360), PR head `004992f27ffe1d5d0d61694b268168148ae1c60d`, completed successfully:

- [Nextest 120949778205](https://github.com/puzige/vega/actions/runs/40120165360/job/120949778205): **28 discovered / 28 executed / 28 passed / 0 failed / 0 skipped**, across 16 binaries. Actual summary reports 28 tests run, not an empty-suite branch printed in workflow source.
- Python **5 + 15 + 40 = 60** executed and passed.
- [Clippy 120949778262](https://github.com/puzige/vega/actions/runs/40120165360/job/120949778262): fmt and all-target deny-warnings Clippy success.
- [Required aggregate 120950805807](https://github.com/puzige/vega/actions/runs/40120165360/job/120950805807): success.
- Full original log SHA-256: `1b9f9ea8f19cdd66c6415e2e3b4254742ee92dfdd9e3ecbdbaa458457e568fbf`.
- Checkout logs identify actual synthetic PR merge `466216063dc4294232141eb153a96b55f09f29c2` into master `21b9f974215c1b6e3ba2802e9f5699428106feeb`; the PR head tree is `941028ac387c5702f0515abf9da7081c0afed7fc`. Current head already contains that master.

Recoverable original sources were again read back from GitHub at `d2cb0d2df142b97562f3880eb9f271c5a5323798`: [17 conversation cases](https://github.com/puzige/vega/blob/d2cb0d2df142b97562f3880eb9f271c5a5323798/crates/vega_conversation/tests/question_wait_lifecycle.rs), blob `cf068342484052a4c2f0b26f1c38b65eb3ca9f08`, and [11 GPUI cases](https://github.com/puzige/vega/blob/d2cb0d2df142b97562f3880eb9f271c5a5323798/crates/vega/src/window/question_wait_tests.rs), blob `867a42ebbb2320e23b079edd9b943841f1da52f7`. The integrated suites are unchanged from these validated sources. See [earlier raw evidence and source hashes](vega-issue-548-cloud-evidence.md); no original assertion was weakened or omitted.

With both current functional validation and independent review complete, one dedicated cleanup writer completed READY and ceased writes. Its scope was only §5's development tests/exclusive code support and lock edges. Against `004992f27ffe1d5d0d61694b268168148ae1c60d`, the pre-commit cleanup patch (`git diff --binary --full-index ... -- crates Cargo.lock`) is 116,195 bytes, SHA-256 `465d101cc1063487d0bee7dab521059711f0912acdd8e4e0572c129e8f78c896`. Independent final review, final-head required CI and merge remain pending.

### Cleanup inventory and equivalence

The code cleanup changes 15 files, 9 insertions / 2,905 deletions: the two test files, four crate manifests, eight mixed Rust/support files and Cargo.lock. It removes the 17 conversation / 11 GPUI tests, question-test module, notification-unavailable global/init hook, config/startup/Settings test shims, UI label/panel/submission/focus/reconciliation helpers and runtime synthetic event-stream helper. Both test-support features and temporary dev dependencies are removed. Production clock/time-source injection, acceptance callbacks, queue identity/wakeups, pending panel resolution, run/child subscriptions and notification policy remain.

The lock goes from 924 to 914 packages by pruning only test-exclusive packages: `bit-set 0.8.0`, `bit-vec 0.8.0`, `convert_case 0.11.0`, `proptest 1.11.0`, `proptest-macro 0.5.0`, `quick-error 1.2.3`, `rand_xorshift 0.4.0`, `rusty-fork 0.3.1`, `unarray 0.1.4`, `wait-timeout 0.2.1`. The test-support edges from gpui-pre to backtrace/proptest are removed; backtrace remains for other owners. Other dependency-label changes only remove now-unnecessary version qualification. Targeted pruning yields a lock byte-identical to current master, including upstream store tracing; no retained package metadata changes.

The writer's static audit confirmed eight mixed files match their pre-cleanup bytes after removing only the named gated support ranges, 445 other crate files and 70 protected paths remain identical, manifests parse and lock edges resolve. Its corrected lexical scan of 407 Rust files found no executable doctest (only an unchanged `text` fence); an initial comment/string scanner false-positive on a production glob string was corrected in the observation logic, not product code. Parent independently reproduced the cleanup patch hash, successful format/diff checks, zero Rust test/support declaration matches and zero Cargo.lock difference against master. Final dynamic 0-discovered/0-executed confirmation remains pending Actions.

Local `git diff --check` passed. The initial `cargo +1.98.0 fmt --all -- --check` invocation failed because the installed cargo executable does not handle rustup selectors; using the actual toolchain route `rustup run 1.98.0 cargo fmt --all -- --check` succeeded. This was a command-route fault, not a source-format or product defect. No compilation or test was run locally.

## Validation and safety boundary

Local validation is static source, diff and format inspection only. No local Vega execution, compilation, cargo check, Clippy, tests or packaging. All automated execution uses existing GitHub Actions; status polling is no more frequent than every five minutes unless notified. The parent owns Git/index/commits, PR/CI and docs; the dedicated writer owns only the assigned code cleanup, and the independent reviewer writes nothing.

No runtime database/configuration modifications, CUA acceptance, installation or restart. OS banners/permission denial, focus, actual native timing presentation and notification clicks remain user manual acceptance. Merge and cloud success are not installation or user acceptance. Rollback is reverting the eventual task PR; no schema migration or user-data transformation is introduced.

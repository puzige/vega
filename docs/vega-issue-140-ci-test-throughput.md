# Issue #140 — CI throughput optimization

## User decision / scope (2026-09-22)

The user explicitly requested an active goal to optimize four directions: avoid repeated compilation/preparation, improve remaining slow tests, balance shard duration, and measure additional shards. Remove Issue #136 Python shadow selection and its policy exceptions: it has not reduced test execution. Retain the useful #136 split proof tests and virtual title timer. Full PR suite remains mandatory; no impact-based test skipping, new ignored tests, automatic retries, relaxed assertions, paid runners, production timeout/API changes, local hooks or custom scheduler.

This specification supersedes #123 C9 topology and #136 shadow reporting only where explicitly stated below. Existing required check name `check (fmt, clippy, test)` and fail-closed aggregation remain. Main handles review/integration; dedicated subagents implement in separate owned worktrees. CI/tests only: product UI acceptance and application installation are not applicable.

## Acceptance matrix (before implementation)

| ID | Operation / risk | Expected evidence |
|---|---|---|
| T1 | Remove shadow analysis | No Python selector/config/regression step or report in delivered workflow; authoritative policy describes native full-suite execution |
| T2 | Build tests once, transport archive | Native pinned nextest archive, same commit/toolchain/platform on build/run jobs; worker executes archive without recompiling; measure build/upload/download/extract/runtime and archive size |
| T3 | Archive relocatability | Audit compile-time paths/CARGO_BIN_EXE/current_exe; only test-helper corrections permitted; relocated archive inventory and owned MCP/process/source-dependent tests pass without reaching original build artifacts |
| T4 | Remaining slow matrices | Split all scenarios of three runner_mutation loops (post-mutation process faults including exact stdout, pre-mutation failures, exact/overflow stderr) into independent tests; preserve every typed error, real repository state, zero/one attempt, argv and duplicate-authority assertion; no shared mutable repositories or reduced real process deadlines |
| T5 | Reduce avoidable fixture work | Only remove demonstrably redundant test-wrapper processes/immutable-value reads; still delegate real Git invocations and preserve fault-injection phase/authority semantics. Prove correspondence and targeted regressions; leave uncertain changes out |
| T6 | Balance | Native nextest filters/partitions or fixed exhaustive complementary groups; for every candidate topology list runnable identities and prove disjoint complete union. New tests must automatically belong to a group; avoid hand-enumerated test manifests or dynamic selector scripts |
| T7 | More shards | Compare at least current4 and an increased count (prefer5, optionally8 if useful) with same source/test scope; record queued time, critical path and total worker time, not just shortest shard. Keep only measured beneficial default; experiments may change workflow-only parameters |
| T8 | Correct aggregate | Required check depends on quality, build/archive and all test jobs; failure/cancellation/skipping never passes. fmt/clippy/doc-tests retained. All runner resource weights, retries0 and existing ignored inventory retained |
| T9 | Cloud acceptance | Full suite on exact final delivered tree passes. Preserve failed evidence; fix diagnosed causes, never rerun flakes to green. Local targeted tests and assertion mapping complement cloud gate |

## Implementation plan / permitted choices

1. Fetch current base, isolated branch. Baseline previous accepted run35711964493 at8b671e2 (1770 runnable, max runtime270.972s, maxworker491s, overall506s); upstream changes must be identified. Measure current candidate results; do not claim strict causal percentages across differing test inventories or queue load.
2. Delete `.github/ci` shadow selector and workflow steps. Mark #136 historical spec as superseded, update small authoritative references only.
3. Introduce native nextest build-once archive plus artifact upload/download with compression-level0 (archive already compressed), short retention and explicit same-run artifacts. Build and quality may run concurrently; test workers need successful archive build. Continue read-only cargo cache policy for build/quality, no Cargo cache restoration on workers if unnecessary. Worker checkout contains runtime fixtures and `.config/nextest.toml`.
4. A dedicated Cargo `ci` profile inheriting `test` may set debug=0 and incremental=false to reduce compile/artifact overhead, retaining opt-level, debug assertions, overflow checks and production semantics. Do not modify release/default development profiles. Record cold/cache implications rather than hiding cost.
5. Correct only test-owned runtime source/binary resolution if archive relocation requires it. Prefer nextest runtime CARGO_MANIFEST_DIR/CARGO_BIN_EXE with cargo-test fallback. No public production API change solely for tests.
6. Split specified slow matrices and audit wrapper setup overhead. Common private helpers keep assertion bodies mechanically comparable. Explicitly enumerate all original cases, including success boundary cases. Keep existing threads-required2 and full-shard pricing/PTY weights.
7. Prefer inexpensive native balanced construction (e.g. separately partition heavy Git and complementary rest into same workers, if measurements support it) over maintaining test-name manifests. Native nextest priority overrides for known heavy tests are also allowed to start long work earlier, retaining resource weights. This is execution grouping, never skipped coverage. Avoid overcomplication: benchmark simple hash partition after loop splits first, tune only demonstrated imbalance.
8. Support a bounded way to compare4 versus5+ shards on same source, using native workflow parameters/controlled commits and native commands. No persistent experimental duplicate suite in normal PRs. Final default decided by measured critical path including archive transfer/queue.
9. Review exact source/test coverage, acquire cloud full-suite gate, update PR with measured results and limitations, squash merge, verify delivered tree, clean only owned worktrees/branches, write back and close Issue.

## Experiment decision rule

Archive reuse is a hypothesis, not predetermined acceptance. If transfer/serialization makes it slower after reasonable small tuning, preserve evidence and deliver the faster native topology instead. Likewise, more shards must win actual completion time to become default. Each of four directions must have a concrete implementation or measured rejection, not an unsupported promise. No claim of guaranteed zero flaky tests.

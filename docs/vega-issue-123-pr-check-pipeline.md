# Issue #123 — PR check + master build 云端流水线

## 背景与用户决策

现状：仓库唯一 workflow `release.yml` 只在 `v*` tag 与手动 dispatch 触发；所有门禁压在本地 pre-push 钩子（`scripts/verify.py`），PR 页面 `no checks reported`。本地门禁依赖本机性能，阻塞 commit/push。

2026-09-22 用户裁决（本卡口径）：

1. **门禁全部上云**。本地不再有任何强制/封锁：删除 `.githooks/`、`scripts/verify.py`、`scripts/cargo-lock.sh`、`scripts/cargo-coordinate.py`、`scripts/cargo-share-target.sh` 及 `scripts/tests/`。本地 commit/push 不再检查、不排队、不锁 target。
2. **云端用标准 Cargo 流程**，不再复用 Python 验证脚本（云 runner 上 `cargo fmt/clippy/test` 即标准流程）。
3. **Master 只允许 PR merge**，不允许直接 push。远端分支保护由 admin 在 Settings → Branches 手动开启（本卡不通过 API 改仓库设置）。
4. **PR check + Master build 做交付**：PR → check（fmt/clippy/test）；push 到 master（PR merge 后）→ build（打包产物并上传 artifact）。
5. **发布仍由 `v*` tag 触发**（`release.yml` 保留），master build 不发 Release，避免每个 commit 都发版。

## 合同（Contract）

- C1. 新增 `.github/workflows/ci.yml`（2026-09-22 后续拆分为 `pr-check.yml` + `master-build.yml`，见 C7；`ci.yml` 已删除）：
  - `pull_request`（base `master`）触发 `check` job：`macos-latest`，checkout → `dtolnay/rust-toolchain`（按 `rust-toolchain.toml` = 1.98.0）→ `taiki-e/install-action`（nextest@0.9.146）→ `Swatinem/rust-cache` → `cargo fmt --all -- --check` → `cargo clippy --workspace --all-targets -- -D warnings` → `cargo nextest run --workspace`（`.config/nextest.toml`，`retries = 0`）→ `cargo test --workspace --doc`（2026-09-22 后续优化，见 C8）。
  - `push` 到 `master` 触发 `build` job：同 runner/toolchain/cache，`cargo xtask package`，`actions/upload-artifact` 上传 `dist/Vega-macos-arm64.zip`。
  - 同 ref 并发取消（`concurrency`），避免 PR 连续 push 堆积。
  - `permissions` 最小化：`contents: read`。
- C2. 仓库为 **public**：GitHub 托管 runner（含 `macos-latest`）免费、不计分钟配额。订正 `release.yml` 顶部过时的 "PRIVATE / 10x cost" 注释。
- C3. 删除本地门禁与并发调度资产：`.githooks/pre-commit`、`.githooks/pre-push`、`scripts/verify.py`、`scripts/cargo-lock.sh`、`scripts/cargo-coordinate.py`、`scripts/cargo-share-target.sh`、`scripts/tests/test_coordinator.py`、`scripts/tests/test_verification.py`。
- C4. 文档口径同步为「云端 CI 为准，本地无强制门禁」：`AGENTS.md`、`README.md`、`docs/vega-exec-guide.md` §7、`docs/vega-phase1-plan.md` §3.5、`docs/vega-s1-tasks.md` T03、`docs/vega-release.md`、`.agents/skills/vega-kanban-delivery/SKILL.md`。`docs/vega-issue-107-test-workflow.md` 标注被本卡取代。
- C5. 不改产品行为；不改 `release.yml` 触发逻辑（仅注释）。
- C6.（2026-09-22 后续修复，PR #127）`ci.yml` 两个 job 的 `Swatinem/rust-cache` 统一 `shared-key: vega` 且 `save-if: ${{ github.ref == 'refs/heads/master' }}`。原配置因默认把 job 名计入 key，`check` 只找 `-check-` 缓存，而 master 只跑 `build`（`-build-`），导致每个新 PR 都冷编译；且 PR run 的缓存挂在 `refs/pull/<n>/merge`，其他 PR 无法继承，还占 10 GB 配额。共享 key 后 PR 可继承默认分支缓存。
- C7.（2026-09-22 后续拆分，用户裁决）把单文件 `ci.yml` 拆成两条独立 workflow，缓存 key 继续共享：
  - `.github/workflows/pr-check.yml`（`name: pr-check`）：仅 `pull_request`（base `master`）触发 `check` job（job name 保持 `check (fmt, clippy, test)`，与分支保护 required check 一致），跑 fmt/clippy/test；rust-cache 用 `shared-key: vega` + `save-if: false`（只读，不写缓存）。
  - `.github/workflows/master-build.yml`（`name: master-build`）：仅 `push`（`master`）触发 `build` job，跑 `cargo xtask package` 并上传 artifact；rust-cache 用同一个 `shared-key: vega` + `save-if: ${{ github.ref == 'refs/heads/master' }}`（唯一写缓存方）。
  - 两条 workflow 各自 `concurrency`（`group: ${{ github.workflow }}-${{ github.ref }}`）与 `permissions: contents: read`；触发条件互斥（PR 不触发 build，master push 不触发 check），故不再需要 `if: github.event_name == ...` 守卫。
  - 删除 `ci.yml`。
- C8.（2026-09-22 后续优化，用户裁决）PR 的 test 步骤从 `cargo test --workspace` 换成 `cargo nextest run --workspace`（配置 `.config/nextest.toml`）：每个测试独立进程、独立超时，失败直接给出测试名，挂起有界。`retries = 0`（**禁止**自动重试到绿，见「非目标」；flaky 一律走 `#[ignore]` + R52 冻结清单）；`fail-fast = false`（一次报出全部失败）；`slow-timeout = 600s`；线程数保持 nextest 默认（= CPU 数），不超订。因 nextest **不跑 doc-tests**，新增一步 `cargo test --workspace --doc` 保留原有 doc-test 覆盖。runner/toolchain/cache/job 名不变。

## 非目标

- 不引入付费 larger runner / 自建 runner。
- 不在 CI 跑 release 打包之外的发布动作；tag 发布链路不动。
- 不因 CI 放宽、ignore 或自动重试既有 flaky 测试。
- 不通过 API 修改仓库分支保护设置（admin 手动开）。
- 不改产品应用代码。

## 待决策（已拍板）

| 项 | 决策 |
|---|---|
| Runner OS | `macos-latest`（公开仓库免费，覆盖 macOS-only 路径） |
| 触发 | `pull_request`(base master) + `push`(master) |
| 跑什么 | 标准 Cargo fmt/clippy/test（不用 verify.py） |
| 本地 hooks | 全部删除，本地不封锁 |
| 发布 | 仍 `v*` tag 触发，master build 不发版 |
| 分支保护 | admin 手动开（要求 PR + required check） |

## 验收矩阵

| ID | 场景 | 预期证据 |
|---|---|---|
| A1 | PR 打开/更新 | `check` 自动触发并出现在 PR checks；失败可见原始日志 |
| A2 | push 到 master | `build` 触发，`cargo xtask package` 产物上传为 artifact |
| A3 | 二次运行 | `Swatinem/rust-cache` 命中，明显快于冷跑 |
| A4 | 本地仓库 | `.githooks/` 与 `scripts/verify.py`、`cargo-lock*` 不存在；commit/push 无本地检查、无 target 锁 |
| A5 | 文档 | 无残留「安装本地 hooks」「cargo-lock.sh 调度」作为权威口径的说明；历史 delivery 记录除外 |
| A6 | release.yml | 顶部注释不再声称 PRIVATE/10x；tag 触发不变 |

## 实施步骤

1. 新增 `docs/vega-issue-123-pr-check-pipeline.md`（本文件）。
2. 新增 `.github/workflows/ci.yml`（后续 C7 拆分为 `pr-check.yml` + `master-build.yml`）。
3. 订正 `release.yml` 顶部注释。
4. 删除本地门禁资产（C3）。
5. 同步文档口径（C4）。
6. 提交 PR；合并后确认 master build 通过；由 admin 开启分支保护。

## 回滚

`git revert` 本 PR 即恢复本地 hooks/脚本与旧文档口径；删除 `pr-check.yml` / `master-build.yml` 即停用云端 check 与 master 打包。产品与安装不受影响。

## C9 — Parallel cloud checks (2026-09-22 takeover)

User request: substantially reduce test/pipeline wall time, use cross-runner sharding,
and add heavy-test scheduling first to reduce load-sensitive failures.
This section supersedes C1/C7/C8's sequential single-job topology only.

- Four independent `macos-latest` test jobs run identical source/toolchain/nextest
  versions with `cargo nextest run --workspace --partition hash:<shard>/4`.
  Matrix shard values are exactly `[1, 2, 3, 4]`; `fail-fast: false` preserves
  diagnostics from the other shards. No subset/path filters or retries.
- A parallel macOS quality job runs fmt, clippy and workspace doc-tests once.
- Every Rust job restores the existing `shared-key: vega` cache read-only;
  master-build remains the only cache writer. The first implementation retains
  ordinary Cargo builds in each shard; build archives/profile changes are outside
  this slice, since transfer cost and runtime fixture relocation need measurement.
- The required check remains exactly `check (fmt, clippy, test)`. A lightweight
  `always()` aggregate job depends on both quality and the complete test matrix,
  and succeeds only when both dependency results equal `success`. Failure,
  cancellation and skipped dependencies must fail closed. No branch-protection
  edits are necessary and no `continue-on-error` is allowed.
- Add a default-profile nextest override scoped to package `vega_conversation`
  and the `git_workspace::trusted_git::` test namespace: `threads-required = 2`.
  Keep CPU-default global concurrency, retries=0, fail-fast=false, and the existing
  600-second bounded timeout. Weighting reserves scheduler slots; it does not
  allocate CPU affinity or guarantee freedom from flakes. On a 3-slot runner it
  allows at most one such heavy test plus one ordinary test concurrently.
- Preserve every existing assertion, ignored-test inventory, true Git process
  boundary and product source behavior. Revert the failed environment-variable
  experiment before validating this topology. Do not modify Git wrappers here.

### C9 acceptance and implementation plan

| ID | Operation | Required result |
|---|---|---|
| P1 | Parse/lint workflow and nextest configuration | Valid with pinned nextest 0.9.146; heavy override selects the intended namespace |
| P2 | List all runnable tests and each of four hash partitions | Pairwise disjoint partition sets; their union equals the unpartitioned suite; no lost tests |
| P3 | Evaluate aggregate gate with success/failure/cancelled/skipped combinations | Only all-success passes; exact required check name retained |
| P4 | Execute cloud PR check on the delivered head | All four shards plus fmt/clippy/doc-tests pass with zero retries; skipped inventory unchanged except unrelated upstream changes |
| P5 | Compare successful run to baseline 35695291651 | Report total wall time, queue delays, each shard build/run duration and percentage change; target at least 30% lower wall time, not assumed as proven |
| P6 | Verify scope and local regression | No new ignores, loosened assertions, environment injection, local hooks, branch settings, packaging or installed-app changes |

Implementation: coordinator freezes this contract, delegates workflow/config to a
single executor, updates affected authoritative documentation, reviews static
coverage/fail-closed evidence, updates existing PR 131 without force pushing, then
observes one complete cloud run. A failing run is investigated, not retried to green.
Product UI E2E is inapplicable: no product behavior is changed. Real cloud workflow
execution is the integration acceptance for this slice. If runner contention limits
speedup, report the measured bottleneck before adding more shards.

References: https://www.nexte.st/docs/ci-features/partitioning/ and
https://www.nexte.st/docs/configuration/threads-required/ .

### C9 scheduling and deterministic cancellation acceptance

Hosted run `35705471651` completed all 1760 runnable tests (1758 passed, two
failed) in 8m29s from first job start to aggregate completion. Aggregate correctly
failed. Preserve this run; do not retry it to obtain a pass.

| Failure | Evidence | Required correction |
|---|---|---|
| `vega::tests::pricing::pricing_settings_and_agent_preflight_production_e2e` | Bounded test-app polling failed under shard contention; first isolated local execution passed | Exact package/test override `threads-required = "num-test-threads"`, unchanged assertions/timeouts |
| `vega_runtime::agent::tests::loop_tools::cancel_during_a_read_waits_for_it_then_skips_the_next_call` | 2ms cancellation can precede ToolCallApproved; first isolated local execution also failed | Replace time guess with deterministic test-only synchronization; no runtime scheduling override |

Only the named runtime test in `crates/vega_runtime/src/agent/tests/loop_tools.rs`
may change. Preserve every original lifecycle/order/second-call assertion and add
proof of exactly one executed call and real fixture content in the returned read
result. Do not enlarge timeouts, introduce ignores/retries, fake output or change
product cancellation semantics.

There is no existing read-start signal: Running is followed by cancellation checks
both in the agent and at blocking-worker entry, and regular-file fencing rejects
FIFO fixtures. Permit a narrow `#[cfg(test)]`-only private synchronization gate in
`crates/vega_runtime/src/agent/tools_exec.rs` after the worker's child-token check
and before the unchanged real `execute_readonly` call. Capture the one-shot gate
before spawn_blocking on a current-thread test runtime. Registration is thread-local,
scoped and cleaned up on failure. The worker signals started and waits with a
bounded channel receive; the test awaits started, cancels, releases the gate, and
awaits the real file result. All hook types/registration/branches compile only
under cfg(test); no public API or normal-build behavior changes. This seam controls
an otherwise unobservable concurrency boundary without replacing real Tools.

Acceptance: exact pricing-only override selection; runtime targeted test and
related loop_tools regressions; fmt and affected clippy; next complete hosted
matrix all green with zero retries. Preserve the first isolated runtime failure
and successful pricing evidence. No stronger claim of general flake elimination
is made: pricing serialization is mitigation for observed contention.

### C9 real-PTY scheduling correction

Run `35707121554` passed the previously failing cancellation and pricing tests,
but `vega_ui::terminal::tests::production_terminal_input_handler_and_keys_reach_real_pty`
failed its unchanged 8-second UI/PTY polling deadline in an owned child process.
The complete run was 1759 passed / 1 failed, and the aggregate correctly failed.
Add one exact package/test override with `threads-required = "num-test-threads"`
for this child-process test, using the same scheduling mitigation as pricing.
Do not change its 8-second deadline, assertions, subprocess isolation or real PTY.
Validate exact two exclusive-test selections, focused PTY execution, then a new
complete hosted matrix. Preserve both failed hosted runs as evidence.

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

- C1. 新增 `.github/workflows/ci.yml`：
  - `pull_request`（base `master`）触发 `check` job：`macos-latest`，checkout → `dtolnay/rust-toolchain`（按 `rust-toolchain.toml` = 1.98.0）→ `Swatinem/rust-cache` → `cargo fmt --all -- --check` → `cargo clippy --workspace --all-targets -- -D warnings` → `cargo test --workspace`。
  - `push` 到 `master` 触发 `build` job：同 runner/toolchain/cache，`cargo xtask package`，`actions/upload-artifact` 上传 `dist/Vega-macos-arm64.zip`。
  - 同 ref 并发取消（`concurrency`），避免 PR 连续 push 堆积。
  - `permissions` 最小化：`contents: read`。
- C2. 仓库为 **public**：GitHub 托管 runner（含 `macos-latest`）免费、不计分钟配额。订正 `release.yml` 顶部过时的 "PRIVATE / 10x cost" 注释。
- C3. 删除本地门禁与并发调度资产：`.githooks/pre-commit`、`.githooks/pre-push`、`scripts/verify.py`、`scripts/cargo-lock.sh`、`scripts/cargo-coordinate.py`、`scripts/cargo-share-target.sh`、`scripts/tests/test_coordinator.py`、`scripts/tests/test_verification.py`。
- C4. 文档口径同步为「云端 CI 为准，本地无强制门禁」：`AGENTS.md`、`README.md`、`docs/vega-exec-guide.md` §7、`docs/vega-phase1-plan.md` §3.5、`docs/vega-s1-tasks.md` T03、`docs/vega-release.md`、`.agents/skills/vega-kanban-delivery/SKILL.md`。`docs/vega-issue-107-test-workflow.md` 标注被本卡取代。
- C5. 不改产品行为；不改 `release.yml` 触发逻辑（仅注释）。

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
2. 新增 `.github/workflows/ci.yml`。
3. 订正 `release.yml` 顶部注释。
4. 删除本地门禁资产（C3）。
5. 同步文档口径（C4）。
6. 提交 PR；合并后确认 master build 通过；由 admin 开启分支保护。

## 回滚

`git revert` 本 PR 即恢复本地 hooks/脚本与旧文档口径；`ci.yml` 删除即停用云端 check。产品与安装不受影响。

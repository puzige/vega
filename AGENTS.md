# AGENTS.md — Vega 仓库协作准则

Cross-agent instructions for Vega — a native AI agent desktop (Rust + GPUI).

## 最高原则：SDD（Spec-Driven Development）

**Spec 先行，代码不允许先于 spec。** 所有实现工作必须对应 [`docs/`](docs/) 中的具体规格章节。设计文档以本仓库 `docs/` 为准（主索引见 [README](README.md#状态)）。

- [`docs/vega-exec-guide.md`](docs/vega-exec-guide.md) 是**执行宪法**：红线清单、依赖白名单、遇阻上报协议、验收协议。任何 agent 开工前必读。
- 发现 spec 缺陷 → 提 issue 或修改 spec 文档并注明变更记录，**禁止代码先行**。

## 工作流

1. **不在 `master` 上直接做功能开发**——用 feature 分支（`feat/<task-id>-<slug>`，如 `feat/t01-scaffold`）。
2. **动手前先 `git fetch && git rebase origin/master`**。
3. **任务来源**：`docs/vega-s*-tasks.md` 任务卡（如 T01-T08）。一张卡 = 一个 PR。卡外工作先问。
4. **主 agent 角色**：协调、验收、集成；**代码实现委托给专用 subagent**，主上下文不被实现细节污染。
5. **遇阻**：按 exec-guide §6 用 `[BLOCKED]` 格式上报，禁止自创方案绕过。
6. **验收强制 E2E-first**：优先以真实 production 入口、owned temp repo 与真实 controller 的端到端证据验收；test-only seam 仅保留无法由 E2E 稳定证明的安全不变量，证据分级与留存格式见 [exec-guide §7](docs/vega-exec-guide.md#7-验收协议每个任务卡通用)。

## 提交与 PR

- 提交格式：`feat(A2-09): <一句话>` / `fix(A3-07): <一句话>`（功能点 ID 见 [vega-features.md](docs/vega-features.md)）
- 小步提交，一个任务卡 ≤3 个 commit
- PR 必须附：验收命令原始输出 + 与 spec 的偏离说明（必须为无）
- 合并方式：squash merge，合并后删除功能分支（2026-08-29 决策）

## 验收底线（本地 hooks 强制；云端 CI 延后）

```
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
```

门禁由本地 git hooks 执行（`.githooks/`，见 [vega-s1-tasks.md](docs/vega-s1-tasks.md) T03；一次性安装 `git config core.hooksPath .githooks`）。
外加 exec-guide §3 红线检查（`cargo tree` 依赖方向、色值硬编码 grep 等）。

## 架构红线（速记，详见 exec-guide）

- `vega_runtime` 禁止依赖 GPUI/任何 UI crate（headless 可测）
- 跨 crate 共享类型只放 `vega_conversation::types`
- API key 只存 macOS Keychain，永不落文件/日志
- 非测试代码禁止 `unwrap()`/`expect()`
- schema 只增不删，走 `migrations/` 递增文件

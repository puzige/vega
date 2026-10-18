# Composer 预测新 master 集成

Purpose: 同步 #473 后的新 master，保留当前测试与断言，交 Codex 审查并重验 CI。
Type: TASK
Status: DONE
Handoff initiator: Codex
Recipient: composer_predictions_impl
Primary owner: Codex
Reviewer: Codex
Decision entry: [规格 v1.6 §8](vega-issue-469-composer-predictions.md#8-新-master-集成边界)
Date: 2026-10-18
Time base: Asia/Shanghai

## 10-Minute Take-Over Package

本轮是已完成实现的纯集成，不是新行为开发。用户明确 #469 不属于 #473 一次性测试清零范围，并已授权同步 master、解决冲突、保留当前全部测试与断言及必要接线，再跑云端 CI；仍不自动合并 master。

先读 `git show origin/master:AGENTS.md`、执行总纲 §7.2、测试清零/TDD 规则及 repo 的更新后交付技能。用原分支/工作区完成 rebase 和保留接线，最终源码/测试/fixture/Cargo 必须与冻结基线逐 blob 一致。不要新增测试、改行为或追认 TDD。禁止本地运行 Vega、编译、Clippy、测试、构建、打包；仅快速静态与格式检查。不要 push/PR/merge。

## Current Status

- Worktree：`~/Workspace/worktrees/vega-469-composer-predictions`；分支 `feat/469-composer-predictions`。
- 冻结实现：`d67cc73128044f65c009542e1f70a77834acfc78`；备份 ref `refs/codex/vega-469/pre-master-sync` 指向它，禁止移动或删除。
- Codex 已 fetch，`origin/master` 为 `9ac5b79b5762f8b6b57e470132034d45ef5a328b`；原基线 `a608ffa2` 到它仅 #473 一项，没有其它新增业务功能。
- PR #472 OPEN/CONFLICTING，auto merge 为空；Issue #469 In progress。旧 GREEN 为 [39121807642](https://github.com/puzige/vega/actions/runs/39121807642)，不能替代新基线验证。
- 原分支三个任务提交：规格 `814f7993`、实现 `4adde02a`、集成 `d67cc731`。主会话仅将本轮文档 amend 到第三个提交；子 agent 接管时 HEAD 会变，但冻结源码 ref 不变。

## Timeline

- 2026-10-18：初始实现与测试一同交付，五轮 CI 后原 head GREEN。已有审计明确缺少初始先行测试/有效 RED/冻结证据，不声称满足 TDD。
- 2026-10-18：#473 合入 master 导致冲突；用户明确保留本任务当前测试，并授权集成。
- 2026-10-18：Codex 冻结受保护路径清单，记录 Git blob 标识；未改生产和测试。

## Scope

1. 保持任务三个提交，将其 rebase 到已核对的 `9ac5b79b`。不能用删除旧测试或 test-support 解决冲突。冲突内容按待重放原提交显式取源，避免混淆 rebase 的 ours/theirs。
2. rebase 后将所有受保护路径从冻结 ref 完整保留：`Cargo.toml`、`Cargo.lock`、`crates/`、`xtask/`。它们共 559 个 Git blob，包含当前全 workspace 生产/测试、断言、测试接线、fixture 和依赖；master 在这些路径没有新增文件，只有清零/机械调整。最终所有 blob 必须与冻结 ref 相同，不只处理冲突文件而漏掉自动应用的删除。
3. 同步 master 的其它 8 个路径：两份 `.agents/skills/*/SKILL.md`、`.github/workflows/pr-check.yml`、AGENTS、README、执行指南、Issue 工作流和 Rust TDD 规则。README 保留本功能 spec 索引；本轮 spec/handoff 由主会话维护，禁止子 agent 改写。
4. 受保护内容保留后 amend 到最后任务提交，仍保持三个任务提交。此操作只保留原有内容，不将旧测试标为本轮新写或 RED；不生成新依赖版本或自动重建 lockfile。
5. 不修改报告历史，不改其它代码。不 push，不 merge master。若 master 已变化或遇到超出清零接线的业务差异，先交回 Codex。

## Non-Scope

- 新需求、新 Bug、功能/算法/预期调整、新测试、代码清理、删除/ignore/放宽测试、timeout 或 retry 调整。
- 本地运行/编译/Clippy/测试/打包，真实配置/应用操作，修改工作流来绕过失败，清理分支/worktree。
- 初始 TDD 时序缺口不能靠本轮重排提交或追加用例弥补。

## Key Entry Points

- Frozen ref：`refs/codex/vega-469/pre-master-sync`。
- 受保护树记录命令：`git ls-tree -r -z <ref> -- Cargo.toml Cargo.lock crates xtask`。
- 冻结树清单：559 entries，SHA256 `5ca038958892e0e83c11d2ec3b3f84f7214e04a1eb1dbe1fd6774f2ffd47412f`。
- 本地 manifest：`/tmp/vega-469-ci.dhEpAC/pre-master-sync-tree.manifest`；只用于静态比对，不执行 Vega。
- 生产/测试差异验证：`git diff --exit-code <frozen-ref> HEAD -- Cargo.toml Cargo.lock crates xtask`。

## Acceptance

- `origin/master` 是新 HEAD 祖先；GitHub 后续应无合并冲突。
- 受保护路径与 frozen ref 的 blob 清单/内容完全一致；不删除或修改任何已有测试/断言/fixture/接线；生产代码同样不变。
- 新规则文档和 CI 保留 master 内容，README 合并保留双方有效索引。
- 本地 `git diff --check`、`cargo fmt --all -- --check`；Git tree/blob 比对均低 CPU 静态验证，不冒充测试。
- Codex push 后只由 Actions 执行新 CI 的 fmt、全目标 Clippy、三组 Python 和非空 Nextest；分别取得发现/执行/通过/失败/跳过计数。
- 子 agent 返回新 HEAD、三个提交、变更与冲突说明、静态命令/退出码、manifest 比对结果与未运行项，然后停止写入。

## Risks and Human Decisions

- 相对新 master 的 PR diff 会包含保留旧测试与支持的较大差异，这是用户明确的本任务范围；本轮不将一次性清零扩展到此任务。
- 未来行为变化需独立 TEST-FIRST；本轮无行为变化不制造 RED。
- CI 成功后仍只准备 PR，不自动合并 master；Issue 与任务资源保留。

## Result Writeback (2026-10-18)

- Status: DONE（纯集成交接结束，新 head 的 CI 仍待云端）。
- Executor: `composer_predictions_impl`；Reviewer: Codex。
- Delivery: 已重放为三个任务提交，新 master `9ac5b79b` 是 HEAD 祖先；完整保留冻结树，包含非冲突自动删除影响的路径。没有新增测试或行为变更。
- Independent verification: Codex 重新读取并比较 frozen ref、当前 HEAD、同步前与子 agent 交回的 NUL manifest，559 个 blob 完全相同，SHA256 仍为 `5ca038958892e0e83c11d2ec3b3f84f7214e04a1eb1dbe1fd6774f2ffd47412f`。`git diff --exit-code` 比较受保护路径 exit 0。
- Rules/CI: 七份规则/CI 文件与 master 完全一致；README 仅比 master 多本功能索引。备份 ref 未变，工作区干净。
- Static checks: 子 agent 最终格式和差异检查 exit 0；Codex 的保护树/规则差异、master 祖先和完整 PR diff 检查均 exit 0。本地未运行 Vega、编译、Clippy、Python 或 Nextest。
- Next step: Codex 推送最新 head，按五分钟节奏读取完整 Actions，保留非空发现/执行与结果证据；TDD 先行证据缺口仍保留，不沿用旧基线 GREEN，不自动合并 master。

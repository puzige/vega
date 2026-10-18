# Composer 输入预测实现交接

Purpose: 在冻结规格内实现 #469，交回 Codex 审查并走云端 CI。
Type: TASK
Status: DONE
Handoff initiator: Codex
Recipient: dedicated implementation subagent
Primary owner: Codex
Reviewer: Codex
Decision entry: [current spec](vega-issue-469-composer-predictions.md)
Date: 2026-10-18
Time base: Asia/Shanghai

## 10-Minute Take-Over Package

先读 AGENTS、执行总纲、设计守则与 v1 spec，按验收矩阵补测试并实现。本地只做静态检查；不得运行 Vega、编译或测试。只修改 spec §5 的模块与对应测试，不改规格、依赖或 CI，不操作真实用户配置，不 push/开 PR/合并。

## Current Status

- 仓库：puzige/vega。
- 分支：`feat/469-composer-predictions`，从最新 `origin/master` 的 `cf00946e` 创建。
- Issue 已由 Codex 领取，In progress；无本卡 PR 或已存在实现。
- 用户确认默认开，复用当前会话模型；剩余实现裁决在 v1 spec。
- 主检出有其他用户改动，本任务只在独立 worktree 工作，不碰主检出。
- Codex 拥有 spec/handoff/README；子 agent 是本卡代码和测试唯一写入者。

## Timeline

- 2026-10-18：读取 Issue，无他人交接/占用；fetch 并核对基线后建分支。
- 2026-10-18：用户确认默认开；Codex 冻结测试矩阵与 Native/ACP 范围。
- 2026-10-18：基线同步至 `abd5dc67`；规格修订至 v1.3，修正真实 Enter 键位、引用块过滤与内部 UI 广播封装。
- 2026-10-18：子 agent 提交 `30bc1dc6`，停止写入；Codex 完成静态 diff、边界和验收覆盖审查，下一步 PR CI。
- 2026-10-18：PR #472 两轮云端编译分别发现测试宏通配导入和既有夹具缺失字段，按原断言修正；基线同步至 `a608ffa2`（#470/#449 滚动边界）。TextInput 与 Composer 交集自动合并后经静态审查，新 head 重验全部门禁。

## Scope

- spec §5 所列配置、conversation 服务、window 控制器、TextInput、Composer、设置和对应测试。
- 新增跨 crate 共享类型统一走 `vega_conversation::types`。
- 同一原生轮次一次独立请求；真实取消、身份与 revision 栅栏；中文 IME 优先。
- 捕获 usage、保留凭据授权/过滤、失败不影响主会话。
- 交付文档：`docs/vega-issue-469-composer-predictions-delivery.md`，记文件、静态检查退出码、待 CI 覆盖、风险与偏离。

## Non-Scope

- ACP 预测协议、临时预测线程、外部模型/应用 E2E、catch-up、工具调用、埋点、依赖/CI 改动。
- 不回滚别人改动、不清理未知 worktree、不运行本地测试/编译/程序、不操作应用安装。
- 不更改 spec/handoff/README；发现缺口用 `[BLOCKED]` 回报 Codex 并停止相应实现。

## Key Entry Points

- `crates/vega/src/window/agent.rs`：主轮 Finished、`finish_pending_run`、历史投影。
- `crates/vega/src/app_agent.rs`：provider 选择、`OwnerCredentialProvider` 和 pre-attempt guard。
- `crates/vega_conversation/src/automatic_titles.rs`：独立辅助请求和用量持久化参考。
- `crates/vega_conversation/src/threads.rs::composer_history`：有界历史与 privacy 投影参考。
- `crates/vega_ui/src/text_input`：内容/marked range 与绘制；ghost 不进入真实内容。
- `conversation_stream/core.rs` 的输入 observe，`composer_actions.rs` 的开始/结束和键位。
- `settings/preferences.rs::save_preferences`：逐字段补丁与保存后通知。
- `crates/vega/src/tests/title_fixture.rs`：辅助调用不能消耗主轮脚本。

## Acceptance

- 按 spec P1–P12 添加有意义断言，真实按键路径确认接受不提交。
- 本地只允许 diff、格式和静态检查；最终测试/Clippy 只交 Actions。
- 向主 agent 报告所有变更路径、静态检查和退出码、未运行项目、风险、规格偏离及交付文档路径。
- 完成可提交一次实现 commit；不得 push。完成后停止写入，交回 Codex。

## Risks and Human Decisions

- 默认开已由用户明确确认。ACP 排除在本卡范围，使用 Native 当前模型。
- 失败与迟到结果不得改变主轮成功、投到错误线程或覆盖用户输入。
- 不能为绕过辅助调用消费测试脚本而放宽已有测试断言。
- 提供方、缓存与真实中文输入法集成由用户在合并后手测；本地未运行必须如实报告。

## Result Writeback (2026-10-18)

- Status: DONE（本次实现交接完成；Issue 仍 In progress，尚未云端验证或用户验收）。
- Executor: `composer_predictions_impl`，Codex 专职子 agent。
- Reviewer: Codex，已审查生产链路、生命周期、凭据边界、键位与对应断言；类型和运行行为待 CI。
- Deliverable: 实现提交 `30bc1dc6`；[实现交付与静态证据](vega-issue-469-composer-predictions-delivery.md)。
- Verification: `git diff --check origin/master...HEAD` exit 0；子 agent 最终格式检查 exit 0；未运行 Vega、编译、Clippy、测试或打包。
- Next step（按用户 2026-10-18 最新要求更新）: Codex 准备 PR、修复 CI 并确认最新 head 的 required check；保持 PR 开放，不自动合并 master。Issue 仍 In progress，不关闭或清理任务资源。第三轮运行回归修复见[定向交接](vega-issue-469-composer-predictions-ci-repair.md)。

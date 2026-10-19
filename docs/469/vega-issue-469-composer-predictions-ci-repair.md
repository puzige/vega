# Composer 输入预测云端回归修复

Purpose: 修复第三轮 CI 的六个运行失败，交回 Codex review。
Type: REVIEW_FIX
Status: DONE
Handoff initiator: Codex
Recipient: composer_predictions_impl
Primary owner: Codex
Reviewer: Codex
Decision entry: [规格 v1.4](vega-issue-469-composer-predictions.md)
Date: 2026-10-18
Time base: Asia/Shanghai

## 10-Minute Take-Over Package

先按下列原因和动作修复，不进行开放式重构。本地仅快速静态与格式检查，禁止运行 Vega、编译、Clippy 或任何测试。用户本轮只准备 PR，不合并 master。保留全部业务、安全和原主轮断言。

## Current Status

- Worktree：`~/Workspace/worktrees/vega-469-composer-predictions`。
- 分支：`feat/469-composer-predictions`；HEAD `aadea9be`；最新基线 `a608ffa2`。
- Codex 已 fetch，基线未变化且是 HEAD 祖先；worktree 当前仅本次规格和交接文档改动，不回滚。
- [PR #472](https://github.com/puzige/vega/pull/472) OPEN；禁止合并，auto merge 为空。
- [第三轮 CI](https://github.com/puzige/vega/actions/runs/39119487281)：fmt/Clippy success；Nextest 1648 个运行，1642 通过、6 失败，另有 3 个既有 skipped。
- 本地脱色原始日志：`/tmp/vega-469-ci.dhEpAC/third-nextest-plain.log`。

## Timeline

- 2026-10-18：前两轮测试编译修复完成；第三轮通过格式/Clippy 并首次运行完整行为测试。
- 2026-10-18：Codex 读失败日志及生产调用链，形成下列定向修复。

## Scope

1. `crates/vega/src/tests/automatic_reasoning_replay_app.rs` 的两组 SSE 夹具按预测系统提示路由辅助请求，返回合法 `suggestion:null` 的 Chat/Responses SSE。主轮 requests/authorization/脚本 index 和 reasoning/tools/终态断言原样保留；辅助请求不计入主轮数组，另行验证辅助无工具、512 token、一次调用等合适边界。不得通过关闭预测绕过集成或放宽主轮次数断言。主轮完成后预测被误计为第四次，导致首轮等待永不满足；另两例的预测无 reasoning_effort 被主轮 max 断言拦住。
2. `vega_ui/src/conversation_stream/core.rs` 的 permission/question listener 收到队列变化时先 reconcile 预测，再安装卡片。当前 install 在 proposal 未到时写 deferred 后直接 return，不 notify，observe_self 不触发；queue.has_pending 已能读到真实 latch。两种 listener 均修复，保留 permission/question 测试全部断言，补充无卡片时已取消以及解除后旧 fence 仍无效。
3. `vega_ui/src/text_input/state.rs::insert_newline` 在 marked composition 活跃时不插换行、不改变草稿，向原 IME 传播语义。当前 Composer SendMessage 的 composing guard propagate 后，落到输入框 InsertNewline，replace_text_in_range 把整段 marked 文本替换为换行。保留原中文/emoji、marked range 与不提交断言；对 Enter/Shift+Enter 明确覆盖，不改无 composition 时的真实换行行为。
4. `vega_ui/src/conversation_stream/tests/composer_predictions.rs` 区分持续 blocker 与 thread/model 身份变化。两种身份变化仍要求取消、ghost 清空、旧结果拒绝；随后新完成轮应允许新申请，且旧 fence 永不能投递给新 owner。真实 blocker 则继续断言 environment ineligible、begin 被拒绝、结果不复活。采用可读 case 名，不能简单删掉失败断言或使所有变化永久禁用预测。
5. 本子 agent 只更新上述生产/测试文件和原 delivery 的第三轮修复记录；不改规格、README、本 handoff。发现超出原因/接口缺口时先回报 Codex。

## Non-Scope

- 不改依赖、CI、重试次数或测试 timeout，不删除/ignore 测试，不放宽安全与主轮断言。
- 不操作真实配置、安装、应用、master 或其它任务；不 push、开 PR、合并或提交。
- 仅本子 agent 写本次代码，Codex 负责文档和集成。

## Key Entry Points

- `LocalReplayProvider::probe`、optional legacy replay responder：辅助 SSE 与主轮隔离。
- `core.rs` 两个 queue listener；`content.rs::install_pending_permission` 和 `user_questions.rs::install_pending_user_question` 的 deferred 早退。
- `composer.rs::on_send_action`、`state.rs::insert_newline`。
- 规格 P3/P5/P6/P7 与新增 v1.4 身份/阻塞契约。

## Acceptance

- 静态检查：`git diff --check`、`cargo fmt --all -- --check`，必要时 formatter。禁止本地编译或测试。
- Actions 由 Codex push 后执行 `cargo clippy --workspace --all-targets -- -D warnings` 与 `cargo nextest run --workspace`。
- 必须保留主轮 requests 精确 4 次、原 reasoning/max/tool/存储断言，预测独立不消费主轮脚本；保留中文 emoji 和不发送断言；保留每个 blocker 和 route fence 的真实行为覆盖。
- 交回未提交 diff、变更文件、静态检查退出码、未运行项、风险。更新 delivery 后停止写入。

## Risks and Human Decisions

- 用户明确本轮不自动合并。最新 head 必须重验，不沿用旧 head 的成功。
- 真实系统 IME 与模型体验仍待用户手测；进程内 GPUI/SSE 不代表真实外部集成。

## Result Writeback (2026-10-18)

- Status: DONE（本次定向实现交接结束，云端结果仍待最新 head 验证）。
- Executor: `composer_predictions_impl`；Reviewer: Codex。
- Deliverable: 四个列明代码/测试文件及 delivery 的未提交差异，已由 Codex 审查；无其它代码改动。
- Verification: 子 agent 与 Codex 的 `git diff --check`、最终 `cargo fmt --all -- --check` 均 exit 0；没有本地运行、编译或测试。
- Next step: Codex 集成 push，重验最新 head 的 Actions 并补证据到 PR/Issue；保持 PR 开放，不合并 master。

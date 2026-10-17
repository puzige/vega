# #409 自动压缩的持久化来源比较

## 冻结与范围

- 规格版本：v1，2026-10-17，Codex 审查后交 Vega 执行。
- 代码基线：`4eb22a1d776155be00a2e940889f8da15d125f55`；任务分支：`feat/409-compaction-source-comparison`。
- 执行前必读 [AGENTS](../AGENTS.md)、[执行总纲](vega-exec-guide.md)、[看板工作流](vega-issue-workflow.md)、[持续压缩契约](vega-continuous-compaction.md)。
- 只修自动压缩 owned assistant/tool 组的来源误比较。不改全局消息相等语义、provider 回放、压缩触发预算或失败展示。#413 管失败原因传播，#434 管模型输入的历史工具表示；本卡无须等待它们。
- 本次只交付规格，未运行 Vega、编译或测试；所有自动验收均待实现后由 Actions 执行。

## 根因与证据

`agent/loop_.rs` 使用 `assistant_with_tools_and_reasoning` 及 `preserve_reasoning_content.then_some(assistant_reasoning)`，并附加 `response_reasoning`。数据库通过 conversation pipeline 重建的消息不含这些仅内存字段。空思考也会形成 `Some("")` 与 `None` 的差异。

`ConversationCompactionHook::compact_impl` 在 `require_source_fence=false` 且存在 `source_owner_id` 时重建 `canonical_owned`，对两侧调用 `ChatMessage::without_live_tool_arguments` 后直接比较 Vec。该方法仅清除 live 参数 sidecar 和投影标志；`ChatMessage::PartialEq` 仍比较两种 reasoning 字段，正常差异因此成为 `ContextRuntimeError::SourceChanged`。

已记录的 v0.1.128 / v0.1.138 长回合在主模型及最后工具成功后，于摘要 provider 请求前以 `summary_source_changed` 结束。当前基线仍保留上述缺口。诊断未记录具体比较分支或内存快照，不能把每个 SourceChanged 现场都宣称为分支级复现；本规格针对可由代码确定的误判条件补真实 hook 回归。

## 验收用例（全部待实现 / 待 CI）

测试使用进程内 Store、`MockProvider` 和真实 `ConversationCompactionHook`，不启动真实进程、网络或应用。正例必须断言 provider 请求次数、checkpoint、返回投影及原始来源，而非只测比较函数。

| ID | 前置状态与操作 | 可观察结果 / 层级 |
| --- | --- | --- |
| D1 | `continuous_compaction.rs` 的 active assistant 有两个完成工具轮次；持久化字段一致；分别给请求中的 owned assistant 附 `Some("")`、非空 reasoning、非空 response_reasoning（含合成不透明 provider item），自动 compact | 每种差异均调用摘要 provider 一次、安装 checkpoint 并返回可继续的后缀；生产 hook / 存储 |
| D2 | 同上但 reasoning 两侧均为空，或运行时仍有 live 参数 sidecar | 与原正常路径一致；sidecar 不成为来源差异；保留已有 runtime sidecar 相等和投影回归，不扩大生产 public API 只为构造测试 |
| D3 | 带思考字段的同一 active assistant 连续完成两次压缩，随后重开 Store | 覆盖游标前进，工具轮次无遗漏、重复或重执行；重开投影等于实时返回；原始记录不变 |
| D4 | 分别改变 owned 消息 role、正文、图片、tool_call_id、工具 id/名称/审计 input，或调换/缺少/增加工具消息 | 每种真实业务差异仍以 SourceChanged 拒绝，摘要 provider 请求为零，已有 checkpoint 与原始审计不变；用表驱动合成变异覆盖字段及长度/顺序 |
| D5 | 在请求捕获后插入无关 user、修改非 owned 历史或旧工具结果，或篡改 owner/model/source 版本 | 既有非 owned、owner/model 和来源围栏继续拒绝；保留 `issue76_auto_source_fence_rejects_unrelated_history_before_summary_request` 等断言 |
| D6 | 摘要 provider 已返回 usage，安装前来源或 predecessor 改变 | CAS 拒绝安装、旧 checkpoint 保留、usage 按原契约保留；保留 `claude_source_mutation_rejects_checkpoint_and_retains_usage` |
| D7 | 无 reasoning、手工严格围栏、取消、超时、无效摘要及模型切换路径 | 原有行为和错误类型不变，不加自动重试、不绕过校验；持续压缩、timeouts 和历史权限回归全部保留 |
| D8 | 执行上述比较前后，保留请求消息副本并检查持久化/诊断 | 原消息及 provider 回放 reasoning 未被修改，持久化与日志不增加思考内容或 live 参数；全局 PartialEq 对 reasoning 差异仍判不等 |

## 冻结契约

### 持久化相等的精确字段

在 `crates/vega_conversation/src/agent/compaction.rs` 增加私有 `durable_messages_equal(expected: &[&ChatMessage], actual: &[&ChatMessage]) -> bool`（可等价使用引用迭代器，行为不变）。先比较长度，再按原顺序逐消息比较且仅比较：

1. `role`、`content`、`images`、`tool_call_id`；图片用现有类型的完整相等语义，不只比数量。
2. `tool_calls` 的完整 Vec 相等：每项 id、name、input 以及顺序均保留现有相等语义；不得拿 live 模型参数替换审计 input。

不读取或比较 `reasoning_content`、`response_reasoning`、私有 live 参数 sidecar、`model_projection`。不 clone 后清空原始消息，不改变 `ChatMessage::PartialEq` 或 `without_live_tool_arguments` 的全局行为。比较字段以显式白名单实现；后续新增持久化字段须同步此契约及变异回归。

### 修改边界

仅替换 `compact_impl` 中 `expected_owned != actual_owned` 这一 owned 比较：两侧保留现有 `is_owned` 过滤与顺序，收集引用并调用上面的私有函数。canonical 重建、临时电脑使用脱敏和组归属逻辑原样保留。

`request_without_owned_group`、`normalize_completed_live_groups` 与 persisted_history 的验证不改。严格请求的 exact source 判断、自动请求版本单调性、source guard/fingerprint、owner/model 冻结、predecessor、最终事务 CAS 均不改。任何真正 SourceChanged 仍终止并保留旧 checkpoint；不通过吞错、重试、关闭 reasoning 或清除历史取得成功。

## 执行顺序与文件

1. 在 `crates/vega_conversation/src/agent/tests/continuous_compaction.rs` 复用 `append_round` / `repeated_partial_compaction` 的存储样本，先补 D1/D3 生产 hook 正例、D4 字段变异反例和 D8 不变性断言。新增测试名使用 `automatic_compaction_...` 等业务名称，不新增 Issue/任务编号命名。
2. 在 `crates/vega_conversation/src/agent/compaction.rs` 加上述私有比较函数并替换唯一 owned 判定。不得扩大为整个 runtime 消息规范化重构。
3. 按需在 `agent/tests/history_permissions.rs` 补 D5/D6 缺失组合，保留已有断言；仅允许为本次缺口补测试，不重命名其他历史测试。
4. 必要时在 `vega_runtime/src/provider.rs` 现有测试模块补全局 PartialEq 不变性测试；该文件的生产代码不在允许修改范围内。
5. 回报 diff、规格偏离（应为无）和待 CI 矩阵。没有 schema、依赖、配置、预算、UI 或错误类型改动。

## 验证、手测与回滚

本地仅 `git diff --check`、差异审阅和格式静态检查；不编译、不执行任何 Vega 测试。以下命令交 Actions，不能作为本地执行指令：

```sh
cargo nextest run -p vega_conversation -E 'test(automatic_compaction_) | test(continuous_compaction_) | test(model_switch_partial_cursor_) | test(issue76_auto_source_fence_) | test(claude_source_mutation_)'
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
```

最终门禁使用现有 `pr-check.yml`，包括其 Python 脚本检查和 required check `check (fmt, clippy, test)`；不改 CI、不新增真实 E2E、不 ignore 或放宽断言。预期全部通过；当前状态为未运行 / 待实现后 CI。

用户手测（合并发布后）：在开启思考回传的 native 模型中继续一个有多轮工具的长任务，触发自动压缩后应继续生成；同回合第二次压缩及重开后不应重复工具或丢失目标/进度。真实历史变化、超时等真实失败仍可显示；它们的文案由 #413 验收，不要求本卡隐藏错误。不要求用户编辑数据库或运行特殊工具。

回滚：revert 本卡实现提交可恢复原比较；无迁移、用户数据或配置回滚操作。不得回滚为关闭全部源保护。

## 交接与停止条件

- 方案提交在本任务分支；Issue 评论发布固定 commit 链接后才置 Ready。当前执行者待领取，下一执行者 Vega。
- Vega 领取前 fetch，复用 `~/Workspace/worktrees/vega-409-compaction-source-comparison` 的干净任务分支；保护未知修改，按规则将该分支更新到当时 origin/master。基线变动若触及本比较或测试契约，先回报 Codex，不自行改 spec。
- 本卡可独立实施；与 #413/#434 无执行依赖。仍按单一写入者串行取卡，不在共享文件上并发覆盖。
- Vega 只按本规格实现和补回归，不开 PR、不合并、不关闭卡。完成或遇阻均回报 #409，列保持 In progress，当前执行者待领取、下一执行者 Codex，并停止写入。
- 回报包括任务分支/commit、文件、静态检查原始退出码、未执行的 CI 项、限制、真实耗时、规格偏离。遇到字段/安全/API 矛盾依执行总纲 §6 用 `[BLOCKED]` 交回 Codex，不能通过放宽 source/CAS 或删除断言继续。

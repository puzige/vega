# Issue #401：插话消息立即投影到对话区

来源：[Issue #401](https://github.com/puzige/vega/issues/401)。2026-10-14 用户确认范围为「完全对齐 ChatGPT 桌面客户端的操作」，并明确选择方案 A：只对齐手感，不改 steer 消费语义。本规格先于实现，取代 #299 中「已提升为插话的消息继续留在待发条」的展示方式，消费时机与并发约束不变。

## 1. 问题与依据

#299 交付后，运行中点待发条的「插话」只把该行 `behavior` 改为 `steer` 并绑定当前 run 的 generation，消息仍留在待发条，直到 `loop_.rs` 的边界被 `RuntimeEvent::SteeringAccepted` 消费后才由 `PendingMessageDelivered` 落成对话区条目。用户观察到的现象是：点了「插话」要等本轮工具批次结束才会出现在对话区。

参照实现（本机 `ChatGPT.app` 解包核对，仅记录公开行为，不复制私有资产）：

- 桌面客户端 `turn/steer` 的调用方在发 RPC **之前**先向当前 turn 的 `items` 插入一条 `{type: 'steeringUserMessage', status: 'pending', clientUserMessageId, targetTurnId}`，RPC 成功改成 `accepted`，失败按原 id 回滚移除。
- 队列条上的「引导」按钮 tooltip 是 `composer.queuedMessage.sendNowTooltip`＝「提交，但不中断模型运行」；即它不是新的消费时机。
- 消费时机由服务端 `turn.rs` 的 provider 调用之间 drain 决定，与乐观投影无关。

结论：桌面客户端「点下去立刻到对话区」是**客户端投影**，不是更快的 steer。Vega 缺的正是这一层，runtime 一行不需要改。

## 2. 行为契约

### 2.1 投影规则

一条待处理消息属于「已提交给当前 run 的插话」当且仅当同时满足：`behavior == Steer` 且 `generation` 等于当前活跃 run 的 `queue_generation`。满足时：

- 从待发条移除，改为在对话区末尾出现一条用户消息块，包含文字与图片块。
- 该条目使用**本地 identity**（无 `message_id`、无 `sequence`），因此不进入消息锚点/导航，也不写入 composer 历史。
- 不显示额外的状态文案、徽标或说明；与已交付用户消息使用同一气泡与排版。

不满足时（队列项、运行结束后的陈旧插话、无活跃 run）保持现状：留在待发条。

### 2.2 转正与回滚

- 边界消费后 `RuntimeEvent::SteeringAccepted` → `ConversationEvent::PendingMessageDelivered` 到达时，按 `pending_id` 找到该乐观条目，**原位**替换 identity 为 `durable_entry_identity(message_id, "user-text", seq)`（图片块为 `"user-images"`），不追加第二条，不重复写 composer 历史。
- 重复投递同一 `pending_id` 只转正一次。
- run 结束（完成、取消、失败、轮次上限）时若尚未投递，下一次投影不再满足 §2.1 条件，乐观条目被移除，消息回到待发条并沿用现有 `recover_cached` / `继续队列` 恢复路径。
- `promote` 失败时不产生乐观条目，消息留在待发条，保留现有错误提示。
- 投影可重复计算：同一 `steering` 集合重复应用不产生重复条目；会话重新绑定或历史重载后按当前投影重建。

### 2.3 不变项

- steer 的消费时机、`claim_steer`、`user_message_seq` 校验、`mark_delivered`、`preserve_model_input`、tool-call / tool-result 配对、取消与 turn-limit 约束全部不变。
- `Queue` 行为、待发条叠层几何、`插话` 提升按钮、编辑/删除/继续队列、全局默认设置不变。
- 不新增依赖、数据库迁移或运行时代码注释；`vega_runtime` 不改动。

## 3. 先行验收矩阵

自动回归使用 GPUI 测试上下文与进程内 store，全部交给云端 PR Check；本地只做快速静态检查。

| ID | 需求/风险与前置状态 | 操作 | 预期可观察结果 | 层级 |
|---|---|---|---|---|
| S01 | 队列项在运行中提升为插话 | 点「插话」，投影回来 | 该 id 离开待发条，对话区末尾出现一条本地 identity 的用户块 | UI 投影 |
| S02 | 边界消费 | 投递 `PendingMessageDelivered` | 同一条目原位获得 durable id 与 seq，条目数不变 | UI 投影 |
| S03 | run 结束未消费 | 投影不再匹配 generation | 乐观条目移除，消息回到待发条 | UI 投影 |
| S04 | 提升失败 | `promote` 返回错误 | 无乐观条目，消息仍在待发条，错误可见 | app |
| S05 | 默认插话提交 | 运行中发送且默认＝插话 | 持久化成功后立即进入对话区，不进待发条 | app + UI |
| S06 | 默认排队提交 | 运行中发送且默认＝排队 | 仍在待发条 | app + UI |
| S07 | 待发条不残留已提交插话 | 存在 steering 集合时刷新投影 | 待发条只列未提交项 | UI 投影 |
| S08 | 重复投影/重载 | 同一 steering 集合应用两次、切走再切回 | 不产生重复条目 | UI 投影 |
| S09 | 带图片插话 | 附件随插话提升 | 文字块与图片块都转正一次 | UI 投影 |
| S10 | 消息锚点 | 乐观条目存在 | 不出现锚点项；转正后出现 | UI 投影 |
| S11 | 重复投递 | 同一 `pending_id` 投递两次 | 只转正一次 | UI 投影 |
| S12 | #299 回归 | 现有 steer/queue 测试 | 语义与断言不变 | 既有套件 |

用户手测：运行中发消息进队列 → 点「插话」应立即出现在对话区；模型在下一个边界回应该消息；运行中取消后该消息回到待发条且可继续队列。

## 4. 实现计划与归属

1. `vega_conversation::types::PendingQueueProjection` 增加已提交插话集合；app 层按 §2.1 在投影时拆分，`vega_ui` 不接触 generation。
2. `vega_ui::conversation_stream` 在应用投影时对账乐观条目（增/删），并在 `PendingMessageDelivered` 路径原位转正。
3. 主 agent 负责规格、审查、集成、PR 与合并；实现委派给专用子 agent，独占 `crates/vega_conversation/src/types/pending_messages.rs`、`crates/vega/src/window/pending_messages.rs`、`crates/vega_ui/src/conversation_stream/pending_messages.rs`、`core.rs` 及本卡测试文件。
4. 本地不编译、不构建；只做差异与格式快速检查，功能点测试与全量门禁交云端 `pr-check`。

### 4.1 落地方案（2026-10-16 实现记录）

- 投影只传**一个代际标记**：`PendingQueueProjection.committed_generation`，由 app 层在 `project_pending_queue` 中按「活跃 run 且未取消」写入。`committed_message_ids()` 是唯一的判定入口，`vega_ui` 不比较 generation 字符串以外的运行态。
- 乐观条目用**本地 identity**（`key` 前缀 `route:<len>:<id>:pending:<len>:<pending_id>:kind:<kind>`，`message_id: None`，`pending_id: Some(..)`）。因此它天然不进消息锚点、不进 composer 历史、不写 `message_id` 索引。
- `ConversationStream.pending_echoes: Vec<PendingEcho { pending_id, indices }>` 记录每条乐观条目占用的 entry 下标。用**显式下标**而非每次扫描 identity，避免历史分页/工具合并等增删后下标漂移。
- 所有会移动 entry 下标的路径都要维护该记录：删除走 `shift_entry_indices(removed, count)`，插入走 `grow_entry_indices(inserted, count)`；两者同时维护 `active_agent_message` / `last_finished_agent_message` / `injecting.entry_index` 与 echo 下标。`active_response_start` 是字节偏移、`run_notices` 是每次渲染重建的派生映射，均不在下标维护范围。
- 乐观条目在 `PendingMessageDelivered` 路径**原位转正**：`pending_echoes` 中按 `pending_id` 找到条目，按下标把 identity 换成 `durable_entry_identity(message_id, kind, seq)`，不追加第二条、不重复写 composer 历史。
- app 层只在 **DB `promote` 成功后**（`Ok(messages)` 分支）调用 `apply_pending_promotion`，失败路径不产生乐观条目。
- 转正顺序：`append_pending_user` 先去重 durable `message_id`，再尝试 `promote_pending_echo`，最后才走既有的「新建条目」路径。
- 用户可见文案与控件零新增：待发条按 §2.1 过滤已提交项，其余（暂停、继续队列、错误提示）沿用现有渲染。

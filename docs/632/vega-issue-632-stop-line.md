# 主动结束对话缺少停止提示

## 问题与根因

用户主动停止一轮运行后，会话流里没有任何可见结果；参考图（ChatGPT）在该轮用户消息下方显示一行灰字 `You stopped after 0s`。用户裁决：**任意停止都要有这一行固定提示**，位置在**对话流内、该轮用户消息之后**。

现有停止表现完全依赖 `RunActivityGroup` 的阶段标题行（`ui.run_stopped_duration` / `ui.stopped`，次级文字色，23px 行高）。三个缺口使它不出现：

1. **直播零输出停止**：`ConversationStream::finish_run_activity` 只在 `run_activity_groups` 里已有该消息的分组时渲染；停止发生在任何助手内容、任何分组之前时，该分支直接返回，会话流无终态。
2. **重载后丢失**：`hydrate_history_entries` 的 `terminal_runs` 只接受「有耗时」「有活动记录」或「续跑」的助手行。零输出停止行没有耗时，重开会话即不重建分组。
3. **准备阶段停止**：`agent/entry.rs` 在运行时启动前取消时，`finish_prepared_failure` 把助手行写成 `failed` 且无耗时，并且**不发出任何 `ConversationEvent`**。这既让界面无提示，也让重载把它误读成「运行失败」。

## 验收

- 任意停止（含零输出、运行时启动前停止）在该轮用户消息之后显示**一行**固定提示，文案沿用既有键：有耗时为「停止前用时 Xs」，无耗时为「已停止」。
- 该行是既有阶段标题行（次级文字色、无边框、无图标、无按钮），不是错误卡片，也不使用警示色；用户停止不伪造错误。
- 停止行与轮次一一对应：同一消息重复到达终态不重复追加；已存在分组时不新增第二行。
- 重新打开会话后停止行仍在；准备阶段停止的助手行以 `interrupted` 落库，不再以 `failed` 呈现为运行失败。
- 该轮已显示停止行时，发送记录不再并排显示「已取消 / 重试」，避免同一结果两种表达。
- 行高估算与实际渲染一致（沿用 `estimated_header_height` 的 23px），虚拟列表与消息锚点不漂移。

## 实现方案

1. `vega_conversation/src/agent/pipeline.rs`：`finish_prepared_failure` 改为 `finish_prepared_terminal(…, status, …)`，由调用方决定终态，停止传 `interrupted`，真实失败仍传 `failed`。
2. `vega_conversation/src/agent/entry.rs`：
   - 运行时启动前取消：以 `interrupted` 收尾，并 `forward_pipeline_error_with_duration(Cancelled)` 发出 `ConversationEvent::Interrupted`。
   - 运行时启动前的第二次检查：用户取消走 `RuntimeEvent::Interrupted` 并发出 `Interrupted` 事件；`mark_primary_started` 失败保持原有 `RuntimeEvent::Error` 语义不变。
3. `vega_ui/src/conversation_stream/content.rs`：
   - `finish_run_activity` 在无分组且状态为 `Interrupted` 时调用新增的 `push_terminal_stop_entry`，用同一 `RunActivityGroup` 渲染一行终态提示；以 `durable_entry_identity(message_id, "run-activity", None)` 去重。
   - `terminal_runs` 接受无耗时的 `Interrupted` 助手行，使重载同样重建该行。
4. `vega_ui/src/conversation_stream/ordinary_submissions.rs`：`ordinary_controls` 在该提交对应助手分组已是 `Interrupted` 时不渲染发送行。

## 验证与交付

本地只做 `rustfmt --check` 与 `git diff --check`，不编译、不运行 Vega、不跑测试；完整检查交现有 required Actions。本轮不新增 Rust 测试（master 不放测试）。合并后由用户手测：零输出立即停止、运行中停止、准备阶段停止、重开会话、深浅主题。CI 与合并不等于原生界面验收。

# Issue #146 — 执行耗时与中间过程折叠交付报告

## 覆盖的规格项

- 计时从 agent runtime 启动前开始，使用单调时钟；权限等待、工具执行、provider 续轮和上下文压缩共用一个耗时。
- 成功、失败和取消终态将毫秒耗时与 assistant turn 持久化；前置失败、旧历史及未持久化终态的恢复不合成时间。
- 历史投影将耗时只附到最终 assistant 段；空成功轮次保留终态入口，但不创建空 assistant 文本行。
- 运行期间显示展开的状态入口及 thinking/工具活动；用户折叠状态不被增量重置；成功、失败、取消后自动折叠，最终回答保持在主时间线。
- 带持久化耗时的完成历史恢复为默认折叠状态；无耗时的旧历史不显示推测耗时。
- 活动段保留文本、thinking、工具和 artifact 的提议顺序，复用 #103/#70 有界滚动；权限边界与工具分组行为保持有效。

## 改动文件

- `crates/vega/src/app_agent.rs`
- `crates/vega/src/tests.rs`
- `crates/vega/src/tests/agent.rs`
- `crates/vega/src/tests/composer_actions.rs`
- `crates/vega/src/tests/context_compaction.rs`
- `crates/vega/src/tests/plan.rs`
- `crates/vega/src/tests/r69.rs`
- `crates/vega_conversation/src/agent/entry.rs`
- `crates/vega_conversation/src/agent/events.rs`
- `crates/vega_conversation/src/agent/persistence.rs`
- `crates/vega_conversation/src/agent/pipeline.rs`
- `crates/vega_conversation/src/agent/tests/mcp.rs`
- `crates/vega_conversation/src/agent/tests/plan_approval.rs`
- `crates/vega_conversation/src/agent/tests/stream_persistence.rs`
- `crates/vega_conversation/src/git_workspace/trusted_git/tests/codec_topology.rs`
- `crates/vega_conversation/src/history.rs`
- `crates/vega_conversation/src/plans.rs`
- `crates/vega_conversation/src/types/events.rs`
- `crates/vega_conversation/src/types/tests_conversion.rs`
- `crates/vega_conversation/tests/s7_acceptance_e2e.rs`
- `crates/vega_conversation/tests/stream_estimate_e2e.rs`
- `crates/vega_conversation/tests/todo_e2e.rs`
- `crates/vega_store/migrations/0014_execution_duration.sql`
- `crates/vega_store/src/lib.rs`
- `crates/vega_store/src/messages/mod.rs`
- `crates/vega_store/src/messages/tests.rs`
- `crates/vega_store/src/permissions.rs`
- `crates/vega_ui/src/conversation_stream/composer.rs`
- `crates/vega_ui/src/conversation_stream/content.rs`
- `crates/vega_ui/src/conversation_stream/core.rs`
- `crates/vega_ui/src/conversation_stream/mod.rs`
- `crates/vega_ui/src/conversation_stream/model.rs`
- `crates/vega_ui/src/conversation_stream/render_rows.rs`
- `crates/vega_ui/src/conversation_stream/run_activity.rs`
- `crates/vega_ui/src/conversation_stream/tests/composer_actions.rs`
- `crates/vega_ui/src/conversation_stream/tests/composer_counter.rs`
- `crates/vega_ui/src/conversation_stream/tests/context_control.rs`
- `crates/vega_ui/src/conversation_stream/tests/core_flow.rs`
- `crates/vega_ui/src/conversation_stream/tests/e2e_variable_height.rs`
- `crates/vega_ui/src/conversation_stream/tests/hover_copy.rs`
- `crates/vega_ui/src/conversation_stream/tests/hydration.rs`
- `crates/vega_ui/src/conversation_stream/tests/issue146_run_activity.rs`
- `crates/vega_ui/src/conversation_stream/tests/issue148_long_session_scroll.rs`
- `crates/vega_ui/src/conversation_stream/tests/issue151_latest_activity.rs`
- `crates/vega_ui/src/conversation_stream/tests/issue70_tool_activity.rs`
- `crates/vega_ui/src/conversation_stream/tests/mod.rs`
- `crates/vega_ui/src/conversation_stream/tests/skills.rs`
- `crates/vega_ui/src/conversation_stream/tests/thinking.rs`
- `crates/vega_ui/src/conversation_stream/tests/timeline.rs`
- `crates/vega_ui/src/conversation_stream/thinking.rs`
- `docs/vega-issue-146-execution-duration-collapse-delivery.md`
- `docs/vega-issue-146-execution-duration-collapse.md`

## 基线与提交

- 主干基线：`f6a8c5a5f8d7eeb802cc5af28b24de2c2bae8617`（含 #204；本轮 rebase 基线）。
- 冻结规格提交：`5d6ade1`。
- 实现提交：`7281ad8`；本轮定向验收代码 HEAD：`7281ad8`。
- 本轮更新旧测试是为了覆盖新增的运行状态 header / 分段活动行以及“不创建空 assistant 行”；混排顺序、权限边界、工具选择器和展开行为断言仍保留并通过。
- Review follow-up：空 runtime failure 在终态事件前保持 `usize::MAX` active entry；`append_empty_terminal_segment` 创建仅含安全 failure reason 的 assistant entry 并更新 active index，渲染行保持可见。代码已符合规格，本轮补充 GPUI 可见性回归，无需 production 修复。

## 定向 Nextest

本轮 rebase 到 `f6a8c5a` 后重跑了原始 #146 过滤命令；下方其余补充回归记录来自前一轮 `548db948` 基线验收。

原始 #146 过滤命令：

```text
cargo nextest run -p vega_store -p vega_conversation -p vega_ui -p vega -E 'test(issue146_)'
```

原始结果：

```text
Nextest run ID 58cbbc1d-baba-4179-97b1-db34a9f88fdb with nextest profile: default
Starting 9 tests across 15 binaries (1360 tests skipped)
PASS [   0.020s] (1/9) vega_ui conversation_stream::tests::issue146_run_activity::issue146_duration_display_rounds_up_and_changes_units_at_thresholds
PASS [   0.027s] (2/9) vega_conversation history::issue146_tests::issue146_history_attaches_duration_only_to_final_assistant_segment
PASS [   0.041s] (3/9) vega_ui conversation_stream::tests::issue146_run_activity::issue146_empty_runtime_failure_keeps_reason_visible_without_assistant_text
PASS [   0.042s] (4/9) vega_ui conversation_stream::tests::issue146_run_activity::issue146_text_only_run_has_no_empty_live_row_and_shows_total_duration
PASS [   0.044s] (5/9) vega_store messages::tests::issue146_terminal_durations_are_persisted_and_projected_by_page
PASS [   0.048s] (6/9) vega_ui conversation_stream::tests::issue146_run_activity::issue146_terminal_failure_cancel_and_hydration_restore_truthful_statuses
PASS [   0.048s] (7/9) vega_ui conversation_stream::tests::issue146_run_activity::issue146_long_run_activity_is_bounded_and_keeps_its_scroll_position
PASS [   0.051s] (8/9) vega_ui conversation_stream::tests::issue146_run_activity::issue146_live_activity_folds_once_and_keeps_final_answer_visible
PASS [   0.145s] (9/9) vega_conversation agent::tests::stream_persistence::issue146_runtime_duration_includes_permission_wait_and_tool_continuation
Summary [   0.147s] 9 tests run: 9 passed, 1360 skipped
```

空 runtime failure 专项命令：

```text
cargo nextest run -p vega_ui -E 'test(issue146_empty_runtime_failure_keeps_reason_visible_without_assistant_text)'
```

原始结果：`Nextest run ID ba1c066c-9e90-48c3-8b03-f60735f41742`；`1 test run: 1 passed, 491 skipped`。测试证明 failure reason 使用安全本地文案、空 Markdown 仍占一条可见 failure row，且 sentinel active index 在终态后已替换。

追加 UI 回归过滤命令：

```text
cargo nextest run -p vega_ui -E 'test(issue70_) | test(issue151_) | test(i61_) | test(r70_) | test(issue146_) | test(hydrated_) | test(scroll_up_page_prepends_and_keeps_streaming_turn_on_target) | test(durable_entry_identity_survives_prepend_and_rebuild) | test(completed_plan_replaces_streaming_assistant_after_older_plan_refresh) | test(issue117_) | test(ten_k_mixed_items_trunk_e2e) | test(issue148_)'
```

原始结果：`Nextest run ID 19e83862-f9f7-4d0d-8eaa-3b08fd53ffd4`；`59 tests run: 59 passed, 428 skipped`。覆盖 #70 工具 selector/权限边界、#151 展开行为、thinking 与工具混排顺序、历史重载、页面锚定及 #146 状态入口。

Vega 根层终态折叠回归命令：

```text
cargo nextest run -p vega -E 'test(i61_newest_live_thinking_block_is_expanded_by_default) | test(i61_provider_reasoning_reaches_live_ui_without_persisting_as_answer)'
```

原始结果：`Nextest run ID c89e621c-205d-4a42-8b05-b2581c984772`；`2 tests run: 2 passed, 209 skipped`。

MessageStarted 不创建空 assistant 行的回归命令：

```text
cargo nextest run -p vega_ui -E 'test(durable_assistant_events_require_exact_active_message)'
```

原始结果：

```text
Nextest run ID 93e7d476-a08e-428d-8a93-5518ad2371c3 with nextest profile: default
Starting 1 test across 1 binary (486 tests skipped)
PASS [   0.032s] (1/1) vega_ui conversation_stream::tests::core_flow::durable_assistant_events_require_exact_active_message
Summary [   0.033s] 1 test run: 1 passed, 486 skipped
```

递增 migration / 表计数回归命令：

```text
cargo nextest run -p vega_store -p vega_conversation -E 'test(migrate_creates_exactly_the_twenty_five_tables) | test(migrated_store_is_wal_at_user_version_14) | test(schema_has_twenty_five_tables_at_current_user_version) | test(persists_messages_tool_lifecycle_and_zero_cost_usage) | test(two_call_tool_journey_matches_synthetic_invoice_with_zero_error) | test(finds_every_seeded_todo_with_real_tools_and_persists_the_run)'
```

原始结果：`Nextest run ID 7921dc73-2561-47e8-b71d-3e19c04819aa`；`6 tests run: 6 passed, 660 skipped`。

## 其他检查

- `cargo clippy -p vega_ui --all-targets -- -D warnings`：通过（退出码 0；仅有现有 `block v0.1.6` future-incompatibility 提示）。
- `cargo fmt --all -- --check`：首次因 rebase 后 `tests/mod.rs` 模块声明顺序不符合 rustfmt 而失败；修正顺序后复跑通过（退出码 0）。
- `git diff --check`：通过。
- 未运行 workspace 全量 Nextest；未运行真实 provider 或桌面端端到端验收。

## 偏离与风险

- 偏离：无；冻结规格未修改。
- 未解决风险：云端 PR required checks 仍是合并门禁；本报告仅记录本地定向验收。

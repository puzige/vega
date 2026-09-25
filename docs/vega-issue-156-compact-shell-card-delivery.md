# Issue #156 交付记录 · Shell 收起行隐藏命令正文

关联 [Issue #156](https://github.com/puzige/vega/issues/156) 与
[冻结规格](vega-issue-156-compact-shell-card.md)。

## 冻结与范围

- 验证时间：2026-09-25 12:49 UTC / 20:49 CST
- 分支：`feat/156-compact-shell-card`
- 环境：macOS arm64；Rust / Cargo 1.98.0；Git 2.55.0
- 生产代码与回归测试 diff SHA-256：`592b18bd5d4ea7d0d3ad90a3c2db11a75e9d6c9c224c6a045db89521f83dae2c`
- 规格偏离：无。无新增 API、依赖、token、执行或审批行为。

## 验收矩阵

| ID | 需求/风险 | 实际操作 | 预期可观察结果 | 测试层级 | 结果 |
|---|---|---|---|---|---|
| R156-1 | 状态真实且不泄漏命令 | 检查待批准、运行、成功、非零退出、拒绝、取消、失败状态 | 收起文案只含通用「运行命令」动作、真实状态与允许的元数据；不含命令正文 | `ToolCard` 单测 | PASS |
| R156-2 | 路径和参数泄漏 | 使用长绝对路径、带引号参数及多行命令 | 收起行不出现路径、参数或命令行内容 | `ToolCard` 单测 | PASS |
| R156-3 | 展开详情保真 | 展开包含引号与多行的 Shell 卡片 | `$ ` 后完整命令逐字保留；既有输出及终态仍在详情中 | `ToolCard` 单测 | PASS |
| R156-4 | 元数据回归 | 检查耗时、非零 exit、截断和复用 | 耗时、非零 exit、截断、复用沿用既有展示规则 | `ToolCard` 单测 | PASS |
| R70 | 运行计时与行布局回归 | 运行受影响的 live elapsed、分组、长命令布局和单 Shell GPUI 测试 | 计时边界、元数据位置、展开箭头和紧凑行布局通过 | GPUI 测试上下文 | PASS |

测试先行：实现前运行 `cargo nextest run -p vega_ui bash_compact_summary`，新增的两条验收测试 **0 passed / 2 failed**。两项都证明旧收起行仍含命令正文（路径、参数和折叠后的多行内容），与预期通用文案不符。实现后定向测试转绿。

## 实现摘要

`ToolCard` 的 Bash 收起行现在只呈现通用动作和真实状态：等待批准、运行中、已运行、拒绝、取消或失败。非零退出码仍覆盖成功状态并呈现失败；已有耗时、exit code、截断和复用字段继续由原有逻辑处理。移除仅用于收起文案的空白折行 helper。展开详情仍从原始投影显示完整 `$ <命令>`、输出和终态。

## 变更文件

| 文件 | 说明 |
|---|---|
| `crates/vega_ui/src/tool_card.rs` | 隐藏收起行中的命令正文，补全六类状态、非零退出、命令泄漏、展开保真及元数据断言 |
| `crates/vega_ui/src/conversation_stream/tests/issue70_tool_activity.rs` | 按新标题更新计时、分组、长命令几何与单 Shell 行的既有 GPUI 回归断言 |
| `docs/vega-issue-156-compact-shell-card-delivery.md` | 本交付记录 |

## 定向 Nextest 原始输出

命令：`cargo nextest run -p vega_ui tool_card::tests`

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 7.82s
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
────────────
 Nextest run ID 46724021-7755-4fcc-9667-d340128d727f with nextest profile: default
    Starting 21 tests across 1 binary (457 tests skipped)
        PASS [   0.020s] ( 1/21) vega_ui tool_card::tests::late_approval_clears_expanded_bash_output_to_fixed_corrupt_card
        PASS [   0.020s] ( 2/21) vega_ui tool_card::tests::issue90_hydrated_bash_validation_card_is_safe_and_actionable
        PASS [   0.020s] ( 3/21) vega_ui tool_card::tests::expanded_bash_command_preserves_quoted_spaces_and_multiline_whitespace
        PASS [   0.020s] ( 4/21) vega_ui tool_card::tests::bash_compact_summary_hides_multiline_command_and_expanded_detail_preserves_it
        PASS [   0.020s] ( 5/21) vega_ui tool_card::tests::corrupt_success_is_content_free
        PASS [   0.020s] ( 6/21) vega_ui tool_card::tests::bash_output_starts_collapsed_and_metadata_is_structured
        PASS [   0.021s] ( 7/21) vega_ui tool_card::tests::invalid_terminal_uses_typed_projection_only
        PASS [   0.021s] ( 8/21) vega_ui tool_card::tests::identical_terminal_only_is_idempotent_and_late_events_are_corrupt
        PASS [   0.022s] ( 9/21) vega_ui tool_card::tests::issue112_canonical_external_edit_card_preserves_target_and_count
        PASS [   0.025s] (10/21) vega_ui tool_card::tests::bash_compact_summary_shows_generic_action_and_real_state_without_command
        PASS [   0.015s] (11/21) vega_ui tool_card::tests::lifecycle_replays_are_idempotent_but_regressions_fail_closed
        PASS [   0.016s] (12/21) vega_ui tool_card::tests::mutation_audit_projection_rejects_each_corrupt_numeric_and_shape_class
        PASS [   0.016s] (13/21) vega_ui tool_card::tests::strict_success_truncation_shape_rejects_impossible_mutation_metadata
        PASS [   0.018s] (14/21) vega_ui tool_card::tests::mutation_success_projection_rejects_each_corrupt_output_field
        PASS [   0.018s] (15/21) vega_ui tool_card::tests::long_bash_command_stays_one_compact_row_and_detail_keeps_the_full_command
        PASS [   0.018s] (16/21) vega_ui tool_card::tests::mutation_terminal_allowlist_and_invalid_projection_fail_closed
        PASS [   0.018s] (17/21) vega_ui tool_card::tests::malformed_skill_receipt_or_reference_stays_content_free_corrupt
        PASS [   0.018s] (18/21) vega_ui tool_card::tests::nonzero_bash_exit_is_presented_as_failure_without_changing_status
        PASS [   0.015s] (19/21) vega_ui tool_card::tests::tool_activity_leading_visual_is_category_owned_and_neutral_for_every_state
        PASS [   0.019s] (20/21) vega_ui tool_card::tests::skill_load_and_reference_cards_show_success_without_reference_body
        PASS [   0.012s] (21/21) vega_ui tool_card::tests::write_success_hides_fingerprint_and_checkpoint_ref
────────────
     Summary [   0.045s] 21 tests run: 21 passed, 457 skipped
```

命令：`cargo nextest run -p vega_ui issue70_e70_`

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.28s
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
────────────
 Nextest run ID 0e7cc7fb-ee4e-4b4f-8e33-b3958ad447cf with nextest profile: default
    Starting 4 tests across 1 binary (474 tests skipped)
        PASS [   0.028s] (1/4) vega_ui conversation_stream::tests::issue70_tool_activity::issue70_e70_long_bash_keeps_running_and_terminal_duration_visible
        PASS [   0.039s] (2/4) vega_ui conversation_stream::tests::issue70_tool_activity::issue70_e70_non_bash_running_and_hydrated_bash_do_not_invent_elapsed
        PASS [   0.044s] (3/4) vega_ui conversation_stream::tests::issue70_tool_activity::issue70_e70_group_owns_no_time_and_children_have_independent_elapsed
        PASS [   0.128s] (4/4) vega_ui conversation_stream::tests::issue70_tool_activity::issue70_e70_live_bash_elapsed_uses_running_clock_and_terminal_duration
────────────
     Summary [   0.128s] 4 tests run: 4 passed, 474 skipped
```

命令：`cargo nextest run -p vega_ui issue70_t70_1_single_shell_is_one_surface_free_compact_row`

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.27s
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
────────────
 Nextest run ID a112914d-ad44-48fd-a106-16cd6c380aa6 with nextest profile: default
    Starting 1 test across 1 binary (477 tests skipped)
        PASS [   0.017s] (1/1) vega_ui conversation_stream::tests::issue70_tool_activity::issue70_t70_1_single_shell_is_one_surface_free_compact_row
────────────
     Summary [   0.018s] 1 test run: 1 passed, 477 skipped
```

命令：`rustfmt --edition 2024 crates/vega_ui/src/tool_card.rs crates/vega_ui/src/conversation_stream/tests/issue70_tool_activity.rs`；`git diff --check`

结果：两个命令退出码均为 0，无输出。

## 剩余限制

- LIMIT：本提交的 GPUI 测试上下文验证了生产工具卡/会话渲染路径；桌面应用中实际运行只读命令、收起/展开的 Computer Use 验收由主控在集成候选版本上执行，本记录不冒充已完成。
- LIMIT：Cargo 提示既有依赖 `block 0.1.6` 将来可能遇到 Rust 兼容性拒绝；本卡未修改依赖。
- 本地未跑 workspace 全量测试、Clippy 或真实外部进程测试，遵循仓库本卡定向 Nextest 门禁。

# Issue #157 交付记录：Sidebar 任务行悬浮快捷操作

## 冻结

- verified_at_utc: 2026-09-25 13:16 UTC
- verified_at_local: 2026-09-25 21:16 CST
- branch: `feat/157-sidebar-hover-actions`
- base: 最新 `origin/master` 与冻结后的 #157 规格
- os_arch: macOS arm64
- rustc: `rustc 1.98.0 (88d9e12ae 2026-08-18) (Homebrew)`
- cargo: `cargo 1.98.0 (797e8a9bc 2026-08-05) (Homebrew)`
- git: `git version 2.55.0`
- tracked implementation diff SHA-256: `4371a9361b2c1be76bab7c32cc773f00ebac1f76d9f614fd9dffe22662b16293`
- initial frozen spec SHA-256: `0b57a2ec8dec587638e1401cc863b51a4c3a592a92d47f5ff5674884e3725338`
- frozen spec SHA-256 after EOF whitespace cleanup: `fac9a4f653c5161b46fe7a058734f506b9a9894c74a62a047425636ed765943c`

## 范围与实现

- 覆盖 Pinned、Recents、项目任务和归档任务行。静止时隐藏快捷按钮并维持固定尾栏；悬浮时显示 24×24 置顶/取消置顶与归档/恢复按钮。标题列和尾栏在状态切换时保持原位。
- 隐藏的完整菜单触发器继续保留键盘焦点入口。鼠标悬浮显示快捷按钮时将触发器收为零尺寸以避免与最右侧快捷按钮重叠；键盘聚焦后隐藏快捷按钮并恢复菜单触发器。菜单打开时保留该行焦点语义。
- 快捷按钮的无障碍名称、提示、图标和动作根据当前任务状态更新。点击复用既有 pin/status mutation 路径，阻止事件传播，不会打开或重命名任务。
- 任务行右键打开既有完整菜单；重复右键关闭，外点关闭沿用菜单行为。标题编辑期间右键不会覆盖编辑内容。
- 修改 R33 交互回归并新增 GPUI 覆盖，未引入依赖、持久化字段、操作种类或公开 API。#77 的悬浮归档需求由 #157 一并覆盖。

## 验收矩阵

| 项目 | 证据 | 结果 |
|---|---|---|
| 静止、悬浮与稳定命中区 | `r157_task_hover_shortcuts_keep_geometry_and_persist_without_navigation` | 通过；静止无快捷按钮，悬浮显示两个 24×24 按钮，标题位置和 72×28 尾栏保持稳定 |
| 键盘入口在悬浮时仍可用 | `r157_task_right_click_reuses_full_menu_and_keyboard_actions` | 通过；悬浮期间完整菜单触发器保留 focus handle，聚焦后快捷按钮收起，Enter 打开菜单，Escape 关闭 |
| Pin/unpin、archive/restore 持久化且不导航 | `r157_task_hover_shortcuts_keep_geometry_and_persist_without_navigation` | 通过；重新打开数据库读取状态，行操作后当前打开任务不变，菜单关闭 |
| 右键菜单及其既有操作 | `r157_task_right_click_reuses_full_menu_and_keyboard_actions`；`task_menu_keyboard_reaches_unread_and_escape` | 通过；右键打开，重复右键/外点关闭，菜单键盘操作归档目标任务且不切换当前任务 |
| 编辑态、运行指示与侧栏交互 | `right_click_during_rename_keeps_editor_and_menu_closed`；`issue150_row_indicator_does_not_displace_the_row_or_trigger`；`issue150_running_row_shows_no_resting_timestamp`；`r26_sidebar_projects_each_task_once_and_reveals_contextual_actions`；`r33_production_task_rows_are_quiet_and_keep_stable_actions` | 通过；编辑内容、运行指示和既有行布局/焦点行为保持正确 |

## 定向测试证据

命令：

```text
cargo nextest run -p vega_ui -E 'test(/r157_|right_click_during_rename_keeps_editor_and_menu_closed|r33_production_task_rows_are_quiet_and_keep_stable_actions|r26_sidebar_projects_each_task_once_and_reveals_contextual_actions|task_menu_keyboard_reaches_unread_and_escape|issue150_row_indicator_does_not_displace_the_row_or_trigger|issue150_running_row_shows_no_resting_timestamp/)'
```

原始输出：

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.28s
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
────────────
 Nextest run ID 37b663e0-c3db-4de6-b12a-e9a69706a7f3 with nextest profile: default
    Starting 8 tests across 1 binary (473 tests skipped)
        PASS [0.033s] (1/8) vega_ui sidebar::threads_block::task_action_tests::right_click_during_rename_keeps_editor_and_menu_closed
        PASS [0.054s] (2/8) vega_ui sidebar::threads_block::task_action_tests::task_menu_keyboard_reaches_unread_and_escape
        PASS [0.090s] (3/8) vega_ui sidebar::threads_block::organization::tests::issue150_row_indicator_does_not_displace_the_row_or_trigger
        PASS [0.090s] (4/8) vega_ui sidebar::threads_block::organization::tests::issue150_running_row_shows_no_resting_timestamp
        PASS [0.168s] (5/8) vega_ui sidebar::threads_block::organization::tests::r33_production_task_rows_are_quiet_and_keep_stable_actions
        PASS [0.340s] (6/8) vega_ui sidebar::threads_block::organization::tests::r26_sidebar_projects_each_task_once_and_reveals_contextual_actions
        PASS [0.461s] (7/8) vega_ui sidebar::threads_block::organization::tests::r157_task_right_click_reuses_full_menu_and_keyboard_actions
        PASS [0.494s] (8/8) vega_ui sidebar::threads_block::organization::tests::r157_task_hover_shortcuts_keep_geometry_and_persist_without_navigation
────────────
     Summary [   0.494s] 8 tests run: 8 passed, 473 skipped
exit_code=0
```

`git diff --check` 退出码为 0；未运行 workspace 全量测试。Nextest 输出中的 `block v0.1.6` future-incompatibility 警告与本卡修改无关。

## 首轮问题与修正

- 初次编译使用了 gpui-kit 未提供的 `IconName::Archive` / `GenericRestore`，编译失败。换成已提供的 `Inbox` / `Undo2` 图标后通过。
- 首轮 hover 布局暴露尾栏高度缩小；生产尾栏固定为 28px 后通过。
- GPUI debug-bounds 不返回 deferred 菜单面板；改为验证菜单状态、触发器和实际键盘操作。
- 额外复测发现透明菜单触发器与右侧归档快捷按钮重叠，导致归档未响应。悬浮时将触发器收为零尺寸并保留其焦点句柄后，鼠标归档和键盘聚焦入口分别通过定向测试。

## 残余

- `ACCEPTED`: 真实桌面窗口中的视觉、鼠标 hover/click 和完整菜单仍需协调 Agent 在集成构建上用 Computer Use 复核；此工作树记录的是 GPUI 自动交互测试证据。
- 首轮冻结规格末尾有一个多余空行；仅移除此 EOF 空白并记录前后 SHA-256，需求文字未改。
- 与冻结规格的偏离：无。

## 静止指针 hover 回归修复

- 基线：`origin/master` `90d7eba`；分支：`feat/157-hover-refresh`。
- 在冻结规格中新增 Pin/unpin 分区重排、Archive/restore 移除与恢复时静止指针的验收条款。
- 成功的 pin/status 持久化操作会立即清除任务行 hover，并抑制列表重排后 GPUI 因旧指针命中区重新激活 hover；真实鼠标移动到任务行后恢复正常 hover。失败的持久化操作不更改 hover 状态。
- 两个 GPUI 测试走快捷按钮、SQLite 写入和组织列表刷新路径。覆盖 pin/unpin、archive/restore、静止指针下快捷按钮和 row state 的清除，并检查打开任务、选中项目、未读投影、菜单状态和焦点状态保持不变。
- 改动文件：`docs/vega-issue-157-sidebar-hover-actions.md`、`crates/vega_ui/src/sidebar/threads_block.rs`、`crates/vega_ui/src/sidebar/threads_block/organization/tests.rs`。

### 先红后绿

测试命令：

```text
cargo nextest run -p vega_ui -E 'test(r157_pin_reorder_clears_stale_hover_after_stationary_projection_change) | test(r157_archive_restore_clears_stale_hover_after_stationary_projection_change)'
```

首次运行的原始结果（run ID `163546c8-b7b0-4093-b03b-e8670c027b29`）：

```text
Starting 2 tests across 1 binary (487 tests skipped)
FAIL r157_archive_restore_clears_stale_hover_after_stationary_projection_change
FAIL r157_pin_reorder_clears_stale_hover_after_stationary_projection_change
Summary: 2 tests run: 0 passed, 2 failed, 487 skipped
```

两个失败都观察到目标 thread 仍是 `hovered`；Archive 用例还确认展开归档列表后快捷按钮重新出现。

只在 mutation 后清空 `hovered` 的中间实现仍失败：列表重新布局时 GPUI 会按静止指针的旧坐标重新触发 hover。最终实现增加“鼠标实际移动前抑制行 hover”，相同命令原始结果（run ID `8a70178e-2fd2-494c-b88f-72aadd14cbeb`）：

```text
Starting 2 tests across 1 binary (487 tests skipped)
PASS r157_pin_reorder_clears_stale_hover_after_stationary_projection_change
PASS r157_archive_restore_clears_stale_hover_after_stationary_projection_change
Summary: 2 tests run: 2 passed, 487 skipped
```

### 最终定向验证

命令：

```text
cargo nextest run -p vega_ui -E 'test(/r157_|right_click_during_rename_keeps_editor_and_menu_closed|r33_production_task_rows_are_quiet_and_keep_stable_actions|r26_sidebar_projects_each_task_once_and_reveals_contextual_actions|task_menu_keyboard_reaches_unread_and_escape|issue150_row_indicator_does_not_displace_the_row_or_trigger|issue150_running_row_shows_no_resting_timestamp/)'
cargo fmt --all -- --check
git diff --check
cargo clippy -p vega_ui --all-targets -- -D warnings
```

最终 Nextest 原始结果（run ID `44341e32-b7d7-451a-8806-c3447395acd5`）：

```text
Starting 10 tests across 1 binary (479 tests skipped)
PASS r157_pin_reorder_clears_stale_hover_after_stationary_projection_change
PASS r157_archive_restore_clears_stale_hover_after_stationary_projection_change
PASS r157_task_right_click_reuses_full_menu_and_keyboard_actions
PASS r157_task_hover_shortcuts_keep_geometry_and_persist_without_navigation
PASS r26_sidebar_projects_each_task_once_and_reveals_contextual_actions
PASS r33_production_task_rows_are_quiet_and_keep_stable_actions
PASS right_click_during_rename_keeps_editor_and_menu_closed
PASS task_menu_keyboard_reaches_unread_and_escape
PASS issue150_row_indicator_does_not_displace_the_row_or_trigger
PASS issue150_running_row_shows_no_resting_timestamp
Summary: 10 tests run: 10 passed, 479 skipped
```

`cargo fmt --all -- --check`、`git diff --check` 和 `cargo clippy -p vega_ui --all-targets -- -D warnings` 均退出码为 0。未运行 workspace 全量测试。

### 偏离与待验收

- 与本次补充的冻结规格偏离：无。
- 尚待用户安装集成版本后，用 Computer Use 在桌面窗口复验静止指针 Pin/Archive 操作；GPUI 测试已覆盖自动交互路径。
- 其他未解决风险：无。

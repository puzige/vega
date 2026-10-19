# Issue #347 交付记录 · 项目任务列表懒加载

规格：[vega-issue-347-project-task-lazy-load.md](vega-issue-347-project-task-lazy-load.md)
分支：`feat/347-project-task-lazy-load`（合并基线 `origin/master` = `66365475`；开发起点为 `b63bc109`）

## 变更

- `crates/vega_ui/src/sidebar/threads_block/organization.rs`：`Organization` 用
  `project_threads_visible` / `project_threads_total` 取代原来的 `more` 与
  `project_threads_expanded`；新增 `PROJECT_TASKS_PAGE`、`sync_project_task_windows`、
  `visible_project_ids`、`has_lazy_list_growth`、`grow_lazy_lists`。
  `has_lazy_list_growth` 与 `grow_lazy_lists` 共用同一个 `visible_project_ids`，避免
  「判断有增长」与「实际增长」的可见项目集合分叉导致重复调度。
- `crates/vega_ui/src/sidebar/threads_block/organization/render.rs`：项目级
  「展开更多」控件改为「一次激活追加一页」的 `Control::More`，不再有「收起」分支；
  项目任务行按 `project_threads_visible` 取窗口渲染，窗口见底时控件不再渲染。
- `crates/vega_ui/src/sidebar/mod.rs`：侧栏底部/视口填充回调改调
  `has_lazy_list_growth` / `grow_lazy_lists`，与既有 Recents 窗口共用同一增长入口。
- `crates/vega_ui/src/sidebar/threads_block.rs`：删除 `grow_recents`。
- `crates/vega_ui/src/sidebar/threads_block/organization/menu.rs`：归档过滤切换时重算项目任务总数。
- `crates/vega_ui/src/sidebar/threads_block/organization/projections.rs`：去掉无调用方的
  `render_project_projection` 对旧 `more` 计数与旧「显示更多」控件的引用，改为渲染全部行。
- `crates/vega_ui/src/sidebar/threads_block/organization/tests.rs`：新增短视口懒加载、
  控件逐页追加、高视口填充、加载完毕停止四组用例，并更新原有项目任务展开用例。

## 接手时修正的问题

- 非测试渲染路径曾出现 `unwrap()`（红线：非测试代码禁止 `unwrap`/`expect`），已改为
  `if let` 分支。
- 新用例的折叠/再展开断言与实现的「折叠不增长、展开沿用旧窗口」语义不一致，已按契约修正。
- 新增/改写代码不含注释（仓库 2026-09-23 裁决）；仅同步了一处因方法改名而失效的既有
  doc 链接指向。
- 用例函数名不再携带 issue/任务编号（Issue #338）。

## 本地已执行

| 检查 | 命令 | 结果 |
|---|---|---|
| 格式 | `cargo fmt --all -- --check` | PASS（exit 0，无输出） |
| 差异空白 | `git diff --check` | PASS（无输出） |
| 编号残留 | `grep -rn "issue347\|#347\|347-" crates/` | 无匹配 |
| 新增注释 | `git diff -U0 \| grep "^+" \| grep -E "^\+\s*//"` | 无新增（仓库 2026-09-23 裁决） |
| 非测试 unwrap | 变更的非测试文件 `grep "\.unwrap()\|\.expect("` | 无匹配 |

## 云端验证

- 本卡功能点测试未在本地运行：worktree 的 `target` 为空缓存，任何 `cargo test` 都会触发冷编译，
  违反 Vega 本地验证限制（2026-10-13 用户裁决）。改由云端 `pr-check` 执行，本地不标为通过。
- PR #399：首轮 `Nextest` 仅本卡新增用例失败（`2708 tests run: 2707 passed, 1 failed`），
  原因为短视口下第二个项目表头被滚出可视区导致点击未命中，属用例自身缺陷，既有用例无回归；
  修正后 Clippy / Nextest / `check (fmt, clippy, test)` 全部通过。
- PR #399 合并（squash → `69b75e7f`）后用户手测反馈：项目任务列表下方的「展开更多」入口消失。
  首版按原规格移除了该控件，用户裁定保留控件并要求「一次激活只追加一页」，即本次补充提交。

## 手测反馈的规格修订

- 保留项目级「展开更多」控件；不再引入「收起」，窗口只增不减，加载完毕后控件自动消失。
- 控件与「滚到底部自动追加」共用同一个 `project_threads_visible` 窗口，两条路径行为一致。

## 偏离

无。

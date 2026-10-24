# Issue #588：侧边栏视图真实分派（按项目 / 扁平列表）

- Issue：[#588](https://github.com/puzige/vega/issues/588)
- 看板：Vega Desktop，Status=Backlog，Size=M
- 基线：`a100f730`（origin/master）
- 规则：[AGENTS](../../AGENTS.md)、[执行总纲](../vega-exec-guide.md)、[设计守则](../vega-design-guidelines.md)
- 状态：规格已冻结，待实现。本地不编译、不测试。

## 1. 问题

用户提问「项目里的 session 会不会出现在下方最近任务」，静态核对结论为**不会**，二者硬性互斥。对照 ChatGPT 桌面版后确认：ChatGPT 默认同样不混排，但提供「按项目 / 按列表」切换；Vega 具备同等数据模型却未接线。

### 1.1 现状证据

| 事实 | 位置 |
|---|---|
| 最近任务只收独立任务且未置顶 | `vega_ui/src/sidebar/threads_block/organization/render.rs:545-557` |
| `is_standalone()` 即 `project_id.is_empty()` | `vega_conversation/src/types/thread.rs:101` |
| SQL 层同样二分（`IS NULL` / `= ?1`） | `vega_store/src/threads.rs:115`、`:139` |
| 组织渲染固定三段 | `vega_ui/src/sidebar/threads_block/organization/render.rs:341-342` |
| 三个投影渲染函数无调用点 | `vega_ui/src/sidebar/threads_block/organization/projections.rs:4`、`:151`、`:362` |
| 文件整体 `#![allow(dead_code)]` | 同上，文件头 |
| `Control::View` 写入 `preferences.view` 但无人读回 | `organization/render.rs:76` |
| 偏好字段已存在 | `SidebarPreferences{ view, project_view, sort, manual_thread_order }` |
| 移除项目会置空归属 | `vega_store/src/projects.rs:164` |
| 分页常量 | `RECENTS_PAGE=10`、`PROJECT_TASKS_PAGE=5`（`organization.rs:140-141`） |

### 1.2 ChatGPT 对照

解包位置：`/Users/puzige/Workspace/worktrees/loom-vega-handoff-writeback/.scratch/codex-extract/`

分组主函数 `uLi`（`webview/assets/app-initial-b21bd554b363.js`）：

```js
case `list`:    return e.projectKey == null || !_.has(e.projectKey) && !y.has(e.projectKey);
case `project`: return e.projectKey == null || !b.has(e.projectKey);
```

默认 `mode = project`（`dMi.mode = "project"`，`hSr = e(lSr) ?? oSr`，`oSr = "project"`）。菜单 `HSn()` 生成 `By project` / `In one list` / `By connection`。

结论：默认行为一致，差异在切换能力。

## 2. 目标

1. `render_organization` 按 `preferences.project_view` 分派：`ByProject` → 现有三段；`Timeline` → 扁平列表。
2. 扁平列表下项目会话与独立任务同列，顺序沿用 `sorted_threads`。
3. 默认仍为 `ByProject`，默认观感不变。
4. 消除「能点但无效果」的视图切换与死代码投影。

## 3. 验收矩阵

| ID | 前置 | 必须观察 |
|---|---|---|
| A01 | 全新配置：1 个项目（5 任务）+ 3 独立任务 | 仍为 项目 / 最近任务 两段；最近任务 3 条；项目任务在项目行下 |
| A02 | 切到 `Timeline` | 项目会话与独立任务同列；顺序符合 `sorted_threads`；无重复行 |
| A03 | 切回 `ByProject` | 恢复 A01；偏好持久化，重启保持 |
| A04 | 移除项目 | 其任务归属置空后出现在最近任务（现有语义，不回归） |
| A05 | 扁平列表任务数 > 10 | 首屏 10 条，可继续展开，不丢行 |
| A06 | 存在置顶与已归档任务 | 置顶仍归「已固定」；归档仍受现有开关控制，不混入扁平主序 |

## 4. 非目标

- 不引入 ChatGPT 的 `connection`（按远程主机）分组。
- 不改数据库 schema、不新增迁移。
- 不改 `projects::remove` 的置空语义。
- 不改分页常量与排序规则。

## 5. 开放问题

- `Timeline` 是否应改名为 `List`/`In one list`，与 #589 的 i18n 文案一并处理？建议在 #589 落地时统一。
- 扁平列表是否应包含置顶任务？本卡按 A06 保持独立「已固定」段。

## 6. 约束

- 本地不编译、不测试、不打包，仅静态检查；验证交云端 PR Check，原生观感交用户手测。
- 代码改动走 worktree + PR，不直接改 master。

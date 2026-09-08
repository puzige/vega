# R15 — PI Desktop 侧栏模型与两层任务导航

2026-09-08 · 用户决策规格 · supersedes [R13](vega-r13-sidebar-organization.md) and [R14](vega-r14-sidebar-folder-fix.md) 的自定义分组 IA。

本轮把侧栏收敛为 PI Desktop 的两个产品对象：真实文件夹对应 `Project`，持久对话对应 `Task/Session`。旧分组表和旧组织字段保留用于兼容读取和升级，不再作为产品模型、投影或交互入口；升级不得删除任务、消息、工具调用、费用或权限记录。

## 产品契约

- `Project` 必须绑定一个已存在的真实文件夹。项目身份使用规范化绝对路径去重，项目行显示文件夹名，路径和 branch 放到 tooltip 或菜单。
- `Task/Session` 是一段持久对话，项目绑定可选。绑定项目的任务只显示在该项目下；没有项目绑定的任务只显示在顶部 `SESSIONS`；一个任务在侧栏中只出现一次。
- `SESSIONS` 的加号创建 standalone task。项目行的加号在该文件夹中创建 task。全局新建任务在有 active project 时归属于该项目，否则创建 standalone task。
- standalone task 存储为真正的 `NULL project_id`，不创建可见或共享的 synthetic project；工具运行使用 `Application Support/ai.vega/scratch/<thread-id>` 独立 scratch root。项目、Git、branch、Review 入口在 standalone 状态禁用或引导打开文件夹。
- 打开文件夹要 canonicalize、注册或复用唯一 Project，选中并展开该行；重复打开同一路径不得创建重复项目。取消 picker 或注册失败不改变当前选择、任务或草稿。

## IA 与视觉

```text
SESSIONS                                      [sort] [+]
  ○ standalone task
  ○ another task

PROJECTS                                     [folder-plus]
  ▾ [folder] project name                 [+] [… ]
      task
      task
  ▸ [folder] another project               [+] [… ]
```

- 侧栏默认宽度 275px，窗口默认 1200×760，最小 960×600；section label 12px、内容 13px，行高 28–32px。
- `SESSIONS` 和 `PROJECTS` 是普通弱化 label，不使用选中胶囊。Sessions 最多展示 5 个 28–32px 行，超出只在 sessions 区内部滚动；Projects 占剩余高度并独立滚动。
- Project 行由统一 vector disclosure chevron、13–14px Folder、文件夹名组成。整行切换展开；展开任务缩进约 22px。hover/focus 显示固定 hitbox 的 vector Plus 和 More，不能因出现按钮发生布局跳动。
- 所有交互图标必须使用已有 `vega_ui::icons` 的 vector 绘制；禁止 Unicode `▸`、`▾`、`⌃`、`…` 和文本加号作为交互图标。折叠态使用统一 ChevronRight 或对 ChevronDown 做旋转。
- 删除/隐藏：新建分组、未分组、颜色、自定义组、移入/移出组、Groups/Projects 切换、Timeline 投影、Collapse All。旧表仍可存在，但不出现在产品 UI。

## 数据与兼容

- 新增递增 migration 将 `threads.project_id` 从必填改为 nullable；SQLite 重建表时保留 FK、消息、工具调用、费用、权限和现有行。旧任务继续绑定原项目。
- Store 只返回 primitive rows；conversation 层继续提供共享 Thread/组织快照，UI 不直接读写 SQLite。组织快照同时返回 standalone tasks 和 project tasks。
- Store 的 nullable SQL 读取在 primitive 兼容层暂映射为 empty string；所有产品路径通过 `Thread::is_standalone` / `project_binding` 封闭这个兼容投影，导航、Git、Review、文件索引和权限不会把它当成 project id。
- 旧 `sidebar_groups` / `sidebar_memberships` / 组织偏好不删除；R15 读取时忽略旧分组投影，并保证同一任务只从其真实 project binding 或 standalone 列表选取一次。
- standalone workspace fence 以 `thread_id` 为身份；切换项目或 standalone task 时，旧 workspace/branch/review 结果不得提交到新任务。未绑定项目的 project action 返回可见错误，不伪造项目路径。

## 验收

使用 owned temporary database 和 owned temporary roots，禁止触碰用户真实 DB。至少验证：旧 schema 升级无数据损失；standalone create/restart/scratch isolation；project create/task nesting；任务单一出现；standalone/project switching workspace fence；真实 pointer/keyboard 展开、project +、session +；960×600 和 1200×760 mount。运行 `cargo fmt --all -- --check`、相关 `cargo clippy -- -D warnings`、相关真实 E2E 与 `cargo test --workspace --no-fail-fast`；旧无关失败保留首次输出并说明。性能 benchmark/soak 延后。

## 变更记录

- 2026-09-08：用户要求参考 PI Desktop，明确只保留 Project 与 Task/Session 两个概念，并将 R13/R14 自定义分组 IA 标记为 superseded。

# #308 — 侧栏项目与会话上下排序

2026-10-19 用户修订：当前排序规则与本轮交接以 [v2.5：新消息置前与手动拖动统一顺序](vega-issue-308-message-order.md) 为准，取消三种排序模式。下文保留初次实现契约与历史记录，时间排序条款不再用于本轮执行。

版本：v1.1，2026-10-18。状态：已实现，集成最新基线后待 Actions 重验与用户手测。
Issue：https://github.com/puzige/vega/issues/308
原始基线：`75da678a`；当前集成基线：`cf00946e`。分支：`feat/308-sidebar-reorder`。

v1.1 只更新集成记录，以下产品契约及 A1–A10 验收目标不变。原实现提交 `9190b003` 已通过 Actions（3160 passed、5 skipped），但 master 并发合入 #465 的历史测试/标识清理后出现合并冲突。本卡 rebase 到 `cf00946e`，保留上游清理以及本卡 3 项 service、6 项 GPUI 测试；原有 CI 结果仅作历史证据，新 head 必须重新通过 required check。矩阵中的待验证项以本轮 CI 与后续手测结果回写为准。

## 需求与范围

用户确认“上下调整顺序”。本卡支持当前侧栏的项目列表、已固定会话、每个项目内的会话及最近任务分别调整上下顺序，保存后刷新和重启仍保留。拖放不改变项目归属、置顶标记或自定义分组，不在不同区域间移动会话。维持现有行高、缩进、分页、菜单和唯一投影。

本卡属于 Feature，由 Codex 冻结规格并委派专职子 agent 实现。以下用例先于实现计划冻结。本地不运行 Vega、不编译、不执行任何测试；只做快速格式、差异与静态检查，自动验证只交 GitHub Actions。

## 验收矩阵

| ID | 前置状态与操作 | 可观察结果 | 层级与证据 | 当前状态 |
|---|---|---|---|---|
| A1 | 至少三个项目，拖动末项到首项上半部，再拖首项到末项下半部 | 分别移至开头/末尾；其他项目相对顺序、展开状态及当前会话不变 | GPUI 输入→生产服务→文件 DB；用户手测 | 待实现/CI |
| A2 | 项目内、最近任务、已固定区域各至少三个会话，逐一区域向上/向下拖动 | 精确插到目标行之前/之后，包含首尾；不打开/归档/置顶会话，不改变其他区域相对顺序 | GPUI 生产路径与 service 回归；用户手测 | 待实现/CI |
| A3 | 时间排序模式下先拖一次，再发生会话更新时间变化、组织刷新与重启 | 第一次拖动以当时顺序为基础切换手动排序；后续时间更新不覆盖手排；重开 DB 读回一致 | service 存储与 UI 投影回归 | 待实现/CI |
| A4 | 有手动顺序，菜单依次选更新时间、创建时间、手动排序 | 两种时间排序保留现有语义；切回手动恢复已保存顺序，不改任务时间和内容 | service/UI 菜单回归 | 待实现/CI |
| A5 | 拖到自身、其他项目内会话、已固定/最近跨区域、空白或取消拖动 | 无持久化变更；无误点击、无选择/展开/草稿/未读变化；非法区域无可放置反馈 | GPUI 输入与 service 非法参数回归 | 待实现/CI |
| A6 | stale revision、源/锚点不存在、跨区域锚点、保存失败或已在保存 | 原子拒绝且不展示未保存的新顺序；已有错误入口可见，刷新后能重试 | service 事务断言与 UI 失败态回归 | 待实现/CI |
| A7 | 展开更多/最近任务惰性加载/已固定内部滚动；拖动到已显示边界行 | 仍按完整区域顺序插入，不遗漏或丢失未渲染项；分页继续可达 | UI 分页与 service 完整顺序回归 | 待实现/CI |
| A8 | 手动模式中新建、置顶/取消置顶、归档/恢复、删除会话 | 新会话可发现；已有会话相对顺序不受时间更新影响；唯一投影、内容、绑定、归档语义保持 | service/UI 回归 | 待实现/CI |
| A9 | 键盘进入项目/会话操作菜单，选择上移或下移 | 与拖放走同一持久化动作，首尾不提供无效移动；Esc 及原有菜单操作有效 | GPUI 键盘回归 | 待实现/CI |
| A10 | 浅/深主题、窄窗口，开始拖动/掠过有效与无效目标/释放/取消 | 拖动预览和插入位置清楚；行几何不跳变，不常驻增加说明；普通单击及尾部按钮仍正常 | GPUI 可观测状态/绘制断言与用户手测 | 待实现/CI |

## 当前代码证据

- `crates/vega_ui/src/sidebar/threads_block/organization/render.rs::render_organization` 固定渲染 `render_pinned_pi`、`render_projects_pi`、`render_recents_pi`。
- 当前项目行 `render_pi_project` 和这些区域的会话行没有组织拖放接线。`organization/projections.rs` 中旧项目/分组投影保留 `OrganizationDrag`、`on_drag/on_drop`，但不是当前侧栏入口。不能通过只修旧投影宣称完成。
- 当前会话投影调用 `sorted_threads`，`SidebarTaskSort` 仅 `Updated` / `Created`。即使只改内存行次序，下次刷新仍会被时间排序覆盖。
- `SidebarOrganizationAction::MoveProject` 和 `sidebar_project_order` 已有项目手排持久化。现有 `MoveThread` 是自定义分组成员动作，不可用它实现本卡会话排序。
- `sidebar_organization::apply` 已有 expected revision 事务边界；`organization_request` 已有后台请求、单一在途、generation/metadata fence 与错误呈现，继续复用。

以上为基线代码分析，不是本地运行复现结论。

## 行为与数据契约

### 排序

1. 项目继续使用现有 `project_order` 与 `MoveProject`，未手排项目仍按现有 fallback 追加。
2. 会话排序菜单增加“手动排序”，保留“更新时间”和“创建时间”。排序偏好仍沿用当前全侧栏共用偏好。
3. 会话第一次在时间模式下移动时，按当前时间排序为完整 snapshot 的会话建立基准顺序，执行本次同区域移动，并原子切到 Manual。这样其他区域相对顺序保持不变。后续 Manual 移动沿用保存顺序。再次从时间模式拖动时，以当时显示排序重新建立基准，避免跳回旧手动顺序。
4. 用一个全局会话 ID 顺序保存稳定 rank，投影时按区域过滤。区域由当前真实元数据确定：pinned 优先属于已固定；其余 project binding 非空属于该项目；其余属于最近任务。同一区域内才可移动；不持久化伪造的项目归属。
5. Manual 下，尚未出现在保存顺序中的新会话按更新时间倒序、稳定 ID 打破同时间平局，显示在已排序会话之前；已有记录按保存 rank。置顶/取消置顶只改变所属投影，沿用稳定 rank；归档隐藏不丢 rank，恢复返回该排序位置，删除 ID 在下一次组织写入中清理。既有其他行相对顺序不变。
6. 上/下半行分别表示插入目标之前/之后；计算“之后”的锚点必须基于完整区域顺序，不能只用已渲染分页。允许移至首/末；自身及无效目标不发 mutation。区域间、类型间拖放不转化为其他动作。

### 存储与服务

- 在共享 `SidebarTaskSort` 中增加 Manual；在 `SidebarPreferences` 增加带 `serde(default)` 的 `manual_thread_order: Vec<String>`。复用已有 preferences JSON 字段，无 DDL、迁移或新依赖；旧 JSON 仍解码为原时间模式和空手动顺序。
- 增加独立 `ReorderThread { thread_id, before_id }` 组织动作，`None` 表示移到源所属区域末尾。服务验证源存在、非自身、锚点存在且与源在同一区域。使用现有 revision 与事务，错误时整笔不写入。
- 新排序动作是修改 manual order 的入口；`SetPreferences` 只选择模式及原有视图，保留服务当前的手动顺序，不能用 stale 偏好覆盖它。保存前去重并过滤已删除 ID，受现有 10000 会话快照上限约束。
- UI 和服务的排序必须采用同一生产算法，避免不同时间模式下看到的位置与实际落库不一致。共享类型继续位于 `vega_conversation::types`，服务逻辑位于 `vega_conversation::sidebar_organization`。不为测试扩张公共 API。
- 不写入 Thread 时间、标题、内容、project_id、pinned、状态、消息或 runtime；不碰真实用户数据库/配置。沿用后台持久化，成功才应用新快照，失败保留已提交次序并显示可重试错误。

### 交互与可访问性

- 当前四种行接入拖动，使用现有 GPUI 拖放机制；不同于旧分组 payload 的新 reorder payload 可单独命名以避免误路由。
- 预览使用当前标题与主题 token；有效目标显示上下插入线或等价明确位置反馈，无效目标不显示可放置态。用覆盖绘制或固定槽位，不改变行高。
- 拖动释放、取消与 drop 不能触发导航、项目展开、未读清除或尾部动作；普通点击、右键、键盘激活保持原行为。检查 GPUI 消费 drag payload 与 mouse-up 回调顺序，不能仅凭 `has_active_drag()` 假设已阻止误点击。
- 项目/会话既有操作菜单加入有目标对象的“上移”“下移”，首尾不提供无效方向。键盘沿用菜单的焦点/箭头/Enter/Esc 路径，复用相同服务动作。
- 不新增常驻帮助段、成功 toast 或拖动手柄列。已有分页、滚动、显示更多及焦点操作可继续使用；本卡不新增自动滚动引擎或恢复旧分组/时间线 UI。

## 实现顺序与允许文件

1. `crates/vega_conversation/src/types/sidebar_organization.rs`：冻结共享排序字段及新动作；在 `sidebar_organization/` 完成共同排序、区域验证与原子移动，补兼容、存储、错误与生命周期断言。
2. `crates/vega_ui/src/sidebar/threads_block/organization/{render,menu,projections}.rs`、`organization.rs`：当前四区真实入口接线、完整顺序与拖动反馈；共享行/会话菜单需要时最小修改 `threads_block.rs` 及其既有菜单文件；几何如需新常量仅放 `vega_theme`。
3. 为真实 GPUI 行事件→服务→owned 文件数据库补输入回归；保留既有 hover、置顶、归档、分页、焦点及草稿断言。新增测试按行为命名，不在源码中添加任务编号、注释或无关重构。
4. 仅为新枚举/字段必要的旧 struct literal、exhaustive match 与测试 fixture 编译适配可修改其他相关调用点；不修改 CI、依赖、runtime 或发布流程。

专职实现者只写本卡 worktree，同范围单写入者。遇 API/契约冲突向 Codex 上报，不擅自扩大范围或改本 spec。

## 验证、手测与回滚

本地允许 `cargo fmt --all -- --check`、`git diff --check` 及静态阅读；禁止 cargo check/clippy/test/nextest/build、启动应用或打包。

Actions 使用现有 PR gate：

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo nextest run --workspace`
- required check：`check (fmt, clippy, test)`，以待合并最新 head 为准。

用户合并后手测：在各区域准备至少三个会话/项目，分别上拖、下拖、拖首尾；刷新/重启后核对；切换三种排序；跨区域或 Esc 取消无变化；菜单键盘上移/下移；检查分页、置顶/归档及当前草稿。应用更新/安装状态与代码合并分开报告。

回滚：通过 PR 撤回本卡代码；用户先在排序菜单切回更新时间/创建时间再使用旧版本，避免旧枚举无法识别 Manual。新增 JSON 字段可由旧版本忽略，不删除用户会话或更改项目。Agent 不直接修改用户配置/数据库实施回滚。

交付时报告文件/commit、静态检查退出码、待 CI 项、实际限制与规格偏离。最新 head 的 required check 通过后 Codex squash merge，Issue 保持开放并置 In review；等用户手测明确通过才清理本卡资源与关闭。

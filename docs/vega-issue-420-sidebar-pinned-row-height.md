# Issue #420：置顶任务超过五条时保持固定行高

- 版本：v1，2026-10-16，Codex 审查后交 Vega 执行。
- Issue：[置顶任务超过五条后行高被压缩](https://github.com/puzige/vega/issues/420)。
- 代码基线：[origin/master](https://github.com/puzige/vega/tree/e2568518)；任务分支：`feat/420-sidebar-pinned-row-height`。交接提交以 Issue 中的实际提交链接为准。
- 工作规则：[AGENTS](../AGENTS.md)、[执行总纲](vega-exec-guide.md)、[Issue 工作流](vega-issue-workflow.md)及[设计守则](vega-design-guidelines.md)。
- 当前证据：用户截图测量、生产代码及布局库的静态分析；尚未运行 Vega 或自动测试，不代表修复已验证。

## 1. 问题、根因与边界

用户截图中有 7 条置顶，相邻标题中心约隔 45–47 图片像素；项目内任务约隔 64 图片像素。两区字号相当，置顶区行距约小 29%。公开资料只保留几何结果，不上传原截图、真实任务标题或用户数据；安装版本未记录。

生产调用链：

`ThreadsBlock::render_organization → render_pinned_pi → render_pinned_row → render_row_at_height`

1. `crates/vega_theme/src/lib.rs` 定义 `Typography::SIDEBAR_LINE_HEIGHT = 32.0`；置顶、项目内任务和最近任务共用该目标高度。
2. `crates/vega_ui/src/sidebar/threads_block/organization/render.rs::render_pinned_pi` 为置顶内容使用纵向 Flex、`max_h(32 × 5)` 和 `overflow_y_scroll()`。
3. `crates/vega_ui/src/sidebar/threads_block.rs::render_row_at_height` 设置 `.h(px(row_height))`，没有禁止行本身收缩。GPUI 默认 `flex_shrink = 1.0`；指定高度并不阻止 Flex 在主轴上压缩。
4. 7 行因而能被一起压进 160px 的视口，未形成预期的内容溢出。静态计算 `160 / 7 = 22.857…`，按图中约 2 倍比例为 `45.714…` 图片像素，与截图吻合。运行态结论仍须 Actions 和用户手测确认。

保留 [R29](vega-r29-sidebar-progressive-lists.md) 的五行置顶视口，以及 [R31](vega-r31-sidebar-typography-revert-pinned-leading.md) 的 13px 字号、32px 导航行高。此次补足固定行高的布局约束，不改上述设计决定。

### 数值验算

所有布局数值均为逻辑像素；图片像素仅用于截图交叉验证。

| 置顶数 N | 单行高度 | 内容高度 32×N | 视口高度 min(32×N,160) | 内容超出视口 |
|---:|---:|---:|---:|---:|
| 0 | 无任务行 | 0 | 不渲染置顶区 | 0 |
| 1 | 32 | 32 | 32 | 0 |
| 5 | 32 | 160 | 160 | 0 |
| 6 | 32 | 192 | 160 | 32 |
| 7 | 32 | 224 | 160 | 64 |
| 20 | 32 | 640 | 160 | 480 |

例如 7 条修复后是 `7×32−160=64` 的滚动内容，不能变为 `160÷7` 的压缩行。

## 2. 验收矩阵（先于实现计划）

以下均为待实现、待验证；已有测试只作为保留回归，不继承它们在其他提交上的 PASS。自动证据来自当前实现提交的 GitHub Actions，用户界面证据来自合并后用户手测。

| ID | 前置与操作 | 可观察结果及必须保留的断言 | 层级 / 证据 / 当前状态 |
|---|---|---|---|
| A01 空态与少量 | 隔离 fixture 中分别有 0、1、5 条置顶，挂载真实 Sidebar | 0 条时置顶区及标签均不存在；1/5 条时内容自然高 32/160，每条任务行高 32，相邻行起点差 32，无多余空白或错误提示 | GPUI 挂载布局 / Actions 输出；待 CI |
| A02 超出五条 | 分别准备 6、7、20 条置顶，挂载真实 Sidebar | 视口高 160；滚动前后可观测任务行均高 32，相邻行起点差 32；全部任务保留，不能通过只渲染五条、移除高度上限或减小字号规避 | GPUI 实际 bounds / Actions 输出；待 CI |
| A03 首尾可达 | 在 7/20 条场景，把 ScrollWheelEvent 发到置顶视口内部，滚到底再滚回顶 | 底部最后一条完整进入置顶视口且可点击打开正确任务；回顶后第一条完整可见；单行和标题不裁切，不要求系统滚动条常驻显示 | GPUI 事件与 bounds、OpenedThread / Actions 输出；待 CI；原生滚动待用户手测 |
| A04 数量变化与持久化 | 同一 fixture 从 5 条增加到 6 条，再取消该条置顶回到 5 条；经现有服务刷新并重新读取临时 Store 快照 | 三个阶段任务行均高 32；视口分别为 160/160/160；6 条时可滚到新增任务，回到 5 条后无多余滚动空白；每个任务只出现在正确投影一次，选中任务 ID 与置顶持久化值正确 | 现有服务 + 临时 Store + 挂载 Sidebar / Actions 输出；待 CI |
| A05 任务状态 | 在超过五条的 fixture 中切换选中、hover、行内操作菜单、运行→停止；操作目标先滚到视口内 | 行高、标题起点、行宽保持稳定；操作、选中背景、真实运行指示可见且不被行高压缩裁切；菜单与键盘焦点的现有断言保留，不新增状态文案 | GPUI 挂载状态 / Actions 输出；待 CI；原生键盘与外观待用户手测 |
| A06 普通行回归 | 同时保留项目内普通任务、最近任务；复核 Light/默认宽，Dark/最小侧栏宽及 Light/最大侧栏宽的代表场景 | 共享任务行仍为 32；13px 字号、标题缩进、尾部操作槽、分组间距、排序与唯一投影不变；保留 R31、R35、R37、R48、R50 与现有运行状态/动作回归 | 既有及必要增补的 GPUI 断言 / Actions 输出；待 CI |
| A07 用户验收 | 合并并通过云端发布安装后的实际版本，手动检查 5→6→7 条置顶、滚动与取消置顶，浅深色及窄侧栏各检查代表状态 | 两区行距一致；超过五条只增加滚动内容；首尾任务可操作，状态与标题不裁切；重开后置顶结果仍正确 | 用户纯手动验收与版本记录；待用户手测 |

无新增外部失败通路：本卡不调用 Provider、网络、外部程序或新存储接口；既有错误和权限呈现不能修改或隐藏。

## 3. Vega 实现计划

### 3.1 允许修改的文件和符号

1. `crates/vega_ui/src/sidebar/threads_block/organization/tests.rs`：基于真实挂载 fixture 和现有服务补本卡回归。
2. `crates/vega_ui/src/sidebar/threads_block.rs::render_row_at_height`：在 `.h(px(row_height))` 附近给共享任务行加入 `.flex_shrink_0()`，固定行的主轴高度。
3. 本规格只由 Codex 修订；Vega 回报写同一 Issue，不另改规范。

共享函数覆盖置顶、项目任务、最近任务和保留的任务投影，均使用既有固定导航行高。用一个行级约束保证同一契约，不新增仅为置顶准备的布尔参数或额外包裹节点。

`render_pinned_pi` 的五行 max_h、内部滚动及现有横向裁剪补偿保持原样；先通过任务行约束产生正确溢出。若该单一约束仍不能满足矩阵，按执行总纲在 #420 上报具体反证并交回 Codex，不自行重写滚动或扩大布局改造。

### 3.2 执行顺序与自动回归要求

1. 阅读交接提交的本规格及三个工作规则入口，核对分支、版本、占用与基线；先领取并将卡改 In progress。复用本卡干净 worktree，或从同一远端任务分支创建本卡独立 worktree；同一实现范围只允许一个写入者。
2. 先补 A01–A06 所需测试，再给共享任务行加入禁止收缩约束。预期旧实现会在超五条的 bounds 断言失败；未实际执行前不得报告 RED/GREEN。
3. 复用 `Fixture`、`fixture_with_size`、`sessions`、`bounds`、`persisted_thread` 与 `snapshot`；通过 `conversation::create_thread/create_standalone_thread/set_thread_pinned` 准备合成任务，刷新使用 `ThreadsBlock::refresh_organization`。不得访问用户数据库。
4. 新测试名统一 `issue420_` 前缀，建议围绕以下独立行为组织，允许合并共享准备以免重复挂载：固定行高与数量边界、首尾滚动可达、5→6→5 动态变化、超过五条时状态几何。用 0/1/5/6/7/20 覆盖边界；主题和宽度使用代表组合，不增加笛卡尔矩阵。
5. 几何断言必须测实际挂载的 `pinned-thread-row-{id}`、`organization-pinned-scroll` 等 bounds，比较行高、相邻起点差和视口；不能只检查 style 参数、源文件字符串或纯计算函数。屏幕外节点的 bounds 若不可观测，应滚到对应位置再断言，不把缺少 bounds 当作任务已删除。
6. 滚动复用本文件已有 `VisualTestContext::simulate_event(ScrollWheelEvent)` 模式，position 取 `organization-pinned-scroll` 的视口中心；断言滚动前后首尾位置发生正确变化及最后任务可点击。不要新增生产公共 API、全局滚动句柄或真实外部 UI E2E。
7. 状态复用现有 `set_thread_running` 和既有 hover/菜单/焦点入口，真实状态来源不变。保留原 R31/R35/R37/R48/R50 及运行状态/操作断言，不删除、ignore、放宽断言或重试换绿。
8. 只做快速静态检查，提交本卡实现和回归后回报 #420；仍为 In progress，阶段“等待 Codex 审查”，当前执行者“待领取”，下一执行者“Codex”，并停止写入。Vega 不开 PR、不合并、不关闭 Issue、不改 In review/Done。

### 3.3 不改范围

不调整 `SIDEBAR_LINE_HEIGHT`、字体、五行视口、分组间距、缩进、Sidebar 尺寸、排序、分页/懒加载、唯一投影、置顶/归档/重命名/删除语义、运行状态来源、存储格式、权限和 Provider。无新增依赖，无新增产品文案。遵守禁止代码注释规则。

## 4. 检查、云端测试与回报

本地仅做快速差异和静态检查：`git diff --check`、核对修改范围；如需格式检查只使用不会编译的快速格式命令。禁止本地运行 Vega、编译、构建、打包、Clippy、cargo check、test/nextest。

以下命令仅供 GitHub Actions。当前未启动，均为待 CI；不新增工作流来执行它们。

| 云端命令 / 门禁 | 预期 |
|---|---|
| `cargo nextest run -p vega_ui -E 'test(issue420_)'` | 供需要时定位本卡用例；所有新增用例通过，不冒充完整 required check |
| `cargo fmt --all -- --check` | 格式通过 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 零警告 |
| `cargo nextest run --workspace` | 本卡与保留范围内全部自动回归通过 |
| 现有 `.github/workflows/pr-check.yml` 的 `check (fmt, clippy, test)` | 待合并最新 head 的 Clippy/Nextest 两个执行 job 均成功；既有 Python 脚本测试步骤也保持原样 |

Vega 回报须包含：实际文件与 commit/分支链接、验收项映射、静态检查命令与退出码、所有尚未执行的 CI/手测项、实际耗时和阻碍、规格偏离（应为无）。不要把“提交实现”写成“验证通过”。

Codex 审查后开 PR，关联本 Issue 和规格但不用自动关闭关键字；测试只交 Actions，启动后每 5 分钟查询，通知或用户即时请求除外。仅最新 head required check 通过后 squash merge，卡才进入 In review 等用户手测；保留本卡分支/worktree，合并不代表已安装。

## 5. 用户手测与回滚

1. 用户在云端发布并安装后的实际版本上记录版本；准备可辨认的测试任务，分别保留 5、6、7 条置顶，对照项目任务行距。
2. 在超过五条时把指针放在置顶区滚动，确认末条完整可见且打开正确任务，再滚回第一条；检查选中背景、运行状态、hover 操作和键盘入口。
3. 取消新增置顶回到五条，确认空白和几何恢复；切换浅深色、窄侧栏做代表检查；重开应用后检查置顶结果。用户自行操作真实界面，Agent 不为验收启动 Vega。
4. 失败：记录版本与可观察结果，卡回 In progress 由 Codex 先分析，修订方案后重新交 Vega。
5. 回滚：通过新任务分支及 PR 还原本卡实现与新增回归的集成变更，经相同云端门禁合入；没有迁移或数据回滚。保留本规格与 Issue 历史，不覆盖其他卡的修改。
6. 用户明确通过后，Codex 回写证据、安全检查并完成仅本卡分支/worktree 清理，再关闭 Issue 并读回，最后置 Done 并读回；此前不得关闭、Done 或清理。

## 6. 交接约定

- 下一执行者：Vega。Codex 发布可访问的 v1 规格提交并改 Ready 后，停止本卡实现，等待 Vega 自行领取；发布交接不表示 Vega 已开工。
- 分支：`feat/420-sidebar-pinned-row-height`；worktree 目录名：`vega-420-sidebar-pinned-row-height`，位于仓库规定的工作树目录下。以 Issue 最新有效交接确认占用和提交，不覆盖已有修改。
- 领取前 fetch，保护脏改动和分叉，按仓库规则更新基线；若新基线改变本卡假设或接口，先交回 Codex 修订，不借机扩大范围。
- 回报位置：#420。根因反证、API/测试能力缺口或依赖问题都按执行总纲 §6 用 `[BLOCKED]` 写同卡，保持 In progress，当前执行者“待领取”、下一执行者“Codex”，并停止写入。

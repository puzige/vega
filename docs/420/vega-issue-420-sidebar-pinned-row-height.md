# Issue #420：置顶任务超过五条时保持固定行高

- 版本：v2，2026-10-19；替代 [v1](https://github.com/puzige/vega/blob/fb3122e3a189a27d48231cab8b982c5cecf29503/docs/vega-issue-420-sidebar-pinned-row-height.md) 的失效测试前提，不改变产品验收含义。
- Issue：[#420](https://github.com/puzige/vega/issues/420)。用户已明确授权当前主会话接管方案修订、审查与 PR/CI 职责；代码实现由专职子 agent 承担。
- 生产基线：`39b706e2`；独立分支：`feat/420-sidebar-pinned-row-height-v2`。旧 v1 分支及 worktree 保留，不 force-push。
- 规则：[AGENTS](../../AGENTS.md)、[执行总纲](../vega-exec-guide.md)、[设计守则](../vega-design-guidelines.md)、[Rust 测试开发/交付生命周期](../vega-rust-test-delivery-lifecycle.md)。
- 状态：同一冻结测试的 Actions RED/GREEN 已取得，独立删测与最终空集门禁阶段；用户手测仍待进行。完整证据见 §7 与 [PR #496](https://github.com/puzige/vega/pull/496)。

## 1. 根因、证据与边界

生产链路：`Sidebar → ThreadsBlock::render_organization → render_pinned_pi → render_pinned_row → render_row_at_height`。

最新基线仍共用 `Typography::SIDEBAR_LINE_HEIGHT = 32.0`。`render_pinned_pi` 的纵向 Flex 容器设定 `max_h(32 × 5)` 并启用纵向滚动；共享任务行只设 `.h(px(row_height))`，没有禁止主轴收缩。GPUI 默认 Flex 收缩使七条任务可能一起压进 160px，不能形成预期溢出。

原截图只保留匿名几何数据：七条置顶相邻标题约 46 图片像素，项目任务约 64 图片像素。静态推算 `160 ÷ 7 = 22.857…` 逻辑像素，约二倍缩放得到 `45.714…` 图片像素，与截图一致。截图中的安装版本未知，不从代码推断其版本；静态分析不等于运行验证。

保留五行视口、13px 导航字号、32px 行高、现有唯一投影、排序与持久化。无新增产品文案、依赖升级、存储格式、权限、Provider 或运行状态来源变更。

### 数值验算

下表单位为逻辑像素；视口不包含上方分组标题。

| 任务数 N | 单行高度 | 内容高度 32×N | 视口高度 | 内部滚动距离 |
|---:|---:|---:|---:|---:|
| 0 | 无任务行 | 0 | 不渲染置顶区 | 0 |
| 1 | 32 | 32 | 32 | 0 |
| 5 | 32 | 160 | 160 | 0 |
| 6 | 32 | 192 | 160 | 32 |
| 7 | 32 | 224 | 160 | 64 |
| 20 | 32 | 640 | 160 | 480 |

例如七条修复后应为 `224−160=64` 的滚动内容，不是每行 `160÷7`。

## 2. 冻结验收矩阵

先冻结以下可观察结果，再编写测试与生产实现。A01–A06 用本轮新建的最小进程内 GPUI/临时 Store 测试覆盖；不存在的历史 R31/R35/R37/R48/R50 测试不作为前提，也不恢复旧集合。A07 及真实像素/原生系统集成交用户手测，不用 Agent 自产截图代替。

| ID | 操作与前置 | 必须观察的结果 |
|---|---|---|
| A01 空态与少量 | 同一真实 Sidebar 路径分别准备 0、1、5 条置顶 | 0 条无置顶分组/标题；1/5 条视口自然高 32/160，每行实际高 32、相邻起点差 32，无额外空白或错误 |
| A02 超过五条 | 分别挂载 6、7、20 条置顶 | 视口实际高 160；滚动前后可观测行高与相邻起点差均为 32；所有任务保留，不能只渲染前五条、去掉视口上限或减小字号 |
| A03 内部首尾可达 | 七条和二十条时固定预期任务顺序，点击首条→置顶视口中心滚到底→点击末条→滚回顶→再次点击首条 | 每次打开预定 ID；实际内部位移分别为 64/480，滚回顶后归零；末条/首条完整进入视口；内层可消费滚动时外层不动；逐段核对全部任务身份及 32px 行距，标题和行内内容处于完整行盒内 |
| A04 数量变化与持久化 | 保持同一 Sidebar/Window 和明确任务顺序，在临时 Store 中五条→置顶第六条→内部滚到底→取消第六条，等待真实服务刷新/重绘 | 每阶段行高 32、视口高 160；六条时实际内部位移 32、末条完整可见且新增任务可达；回五条后偏移归零、五行完整占满视口而无底部空白；从新 Store 连接核对置顶持久化值及排序/唯一投影，打开任务 ID 正确 |
| A05 状态与操作 | 超过五条时选中、悬停、行内菜单/焦点、运行→停止；目标先滚入视口 | 行高、宽度、标题起点稳定；选中背景、运行指示和操作入口不被压缩裁切；行内操作命中正确对象；真实状态来源不变；原生键盘与像素外观仍待用户 |
| A06 共享行回归 | 同时有展开项目内普通任务和最近任务；代表组合为 Light/默认宽、Dark/最窄宽、Light/最宽宽 | 普通/最近任务同为 32px；标题缩进、操作槽、字号、分组间距不改，置顶任务不在其他投影重复出现 |
| A07 用户验收 | 发布安装后记录版本，检查 5→6→7、取消置顶、首尾滚动、浅深色和窄侧栏，重开应用 | 两区行距一致，超五条只增加内部滚动内容，任务/标题/选中/运行/操作完整，置顶结果重启保留 |

几何比较允许至多 0.5px 的浮点布局误差，不能容忍 32px 变成约 23px。对视口外行，先滚入再观察；缺少 bounds 不能当成任务已删除或跳过断言。内层仍能消费滚动时核对外层不动，不把到边界后的额外事件传播误报为失败。

A05 按现有显示优先级分别测试静止、运行、悬停/操作、菜单焦点和停止：操作组出现时会替换运行指示，不要求二者同时显示。标题、状态和操作槽测实际 bounds 与裁剪区域；选中背景若记录为自动验证，应观测实际绘制 quad。菜单浮层允许超出行盒，不能要求整个菜单被包含在 32px 内。原生文字像素与键盘体验仍交 A07。

## 3. 新建的最小测试支持与允许文件

### 3.1 测试补丁（生产行为保持基线）

1. 新建 `crates/vega_ui/src/sidebar/pinned_rows_tests.rs`，只写 A01–A06 所需 fixture、GPUI 测试和断言。行为命名，不将 issue/task 编号写入非文档代码。
2. `crates/vega_ui/src/sidebar/mod.rs` 接入 `#[cfg(test)] mod pinned_rows_tests;`。测试作为 sidebar 子模块读取私有 `Sidebar.sessions_block`，不新增 getter 或生产公共 API；使用当前 facade 的 `#[gpui_kit::test]` 宏及测试 API，不添加未使用的别名导入。
3. `crates/vega_ui/Cargo.toml` 仅增加开发阶段必需的 dev dependency：当前锁定 `gpui_kit = 0.6.1` 的 `test-support` feature，以及白名单内的 `tempfile`。允许该 feature 必需的传递测试依赖；不另加 GPUI 直接依赖，不升级既有包。`Cargo.lock` 仅记录这些测试 feature 所需解析边，并逐项审查。
4. 几何观测只添加于现有节点：`sidebar/mod.rs` 的外层滚动容器及 `threads_block/organization/render.rs` 的分组/置顶视口使用固定 `debug_selector`；动态行在 `threads_block/organization/reorder.rs::render_reorder_row` 最终返回 `AnyElement` 前以 `#[cfg(test)]` 接入 `.test_support()`，保持原 `Interaction` 参数类型。`threads_block.rs` 的标题、状态、操作槽及必要时 `sidebar/row_helpers.rs` 使用同类测试观测。通过 `Window::find` 得到实际 prepaint bounds/可见性；完整可见另断言行被视口包含。视口不用 `.test_support()` 包裹，避免改变 scroll fade 所需类型。不得新增改变布局的 wrapper、固定高度、最小高度或收缩约束；观测支持必须不改变生产行为，GREEN 后一并删除。
5. 测试在 GPUI `TestAppContext::open_window` 的 960×600 窗口中挂载 `component::Root → Sidebar::new`，用真实侧栏渲染链路测实际 bounds、滚动事件与 `OpenedThread.id` 终态。fixture 预先固定 ID 顺序，可经现有 ReorderThread 服务准备，不调用生产排序函数计算期望。不得用源码匹配、只看 style 参数、独立仿制 Flex 容器或镜像几何算法代替。
6. fixture 使用独立临时目录中已迁移的文件 Store 和合成任务，设定测试所需 globals；organization 后台会按 database_path 新开连接，必须等待真实 snapshot/刷新完成及绘制后再断言。经现有 conversation/sidebar organization 服务创建任务、置顶与刷新。不调用 `sidebar::init` 读取用户数据库，不读取/写入用户配置，不启动 Provider、网络、shell、真实 Git 或应用。GPUI 初始化只加载现有组件/主题支持；必要时允许 `sidebar/projects_block.rs` 中仅新增 `#[cfg(test)]` 的外部分支探测关闭入口，供 fixture 隔离无关文件探测，不能替代被测渲染/排序/滚动，GREEN 后删除。
7. 将边界参数放在数量、首尾滚动点击、5→6→5、状态与普通行四组行为测试中，避免主题×宽度×数量的笛卡尔扩展。排空就绪任务并完成绘制，不无限推进 100ms/2s 周期时钟；每组释放窗口/实体。每个行为测试必须有明确可失败结果；同轮全部验收测试在修复前写齐。

这些支持是为本轮行为新建，不是从历史 ref 整文件恢复。测试代码与接线需主会话审查后提交，再建立 draft PR 取得 Actions RED。无关编译、接线、依赖或 runner 失败不算有效 RED，先留证修正并重跑；不能借口测试设施缺失直接修生产代码。

### 3.2 最小生产实现（仅有效 RED 后）

在 `crates/vega_ui/src/sidebar/threads_block.rs::render_row_at_height` 的 `.h(px(row_height))` 邻近加入 `.flex_shrink_0()`，使置顶、项目、最近等共享导航任务行遵守固定高度。

不新增仅置顶的布尔参数、额外容器或 API。不改 `render_pinned_pi` 五行上限、内部滚动、横向裁剪补偿、渐隐、数据来源和排序；不改主题 token 或间距。若单一行约束不能满足冻结矩阵，先记录反证并修订独立验收/测试周期，不扩写布局规避。

### 3.3 GREEN 后交付清理

先固定带测试提交和同一测试的云端 GREEN，然后用独立提交删除本轮 Rust 测试、测试模块、debug selector/`.test_support()` 观测、dev dependencies/features 及独占锁文件边。保留全部真实控件 ID、无障碍、焦点、事件、滚动和绘制行为。

最终生产代码差异应只有共享任务行新增的一条禁止收缩约束；另保留本规格和必要证据文档。最终树不保留 Rust 测试，原测试从 PR 的不可变提交/源码链接找回。

## 4. 提交、Actions 与证据

顺序：规格冻结和测试补丁提交 → draft PR Actions 有效 RED → 最小修复提交 → 同一测试 GREEN → 独立删测提交 → 最终空集 head 完整门禁 → 正式 PR/合并。主会话审查并集成；实现子 agent 不自行开 PR、push、合并或修改规格。

本地只做快速差异、格式和静态检查，例如 `git diff --check`、`cargo fmt --all -- --check`（仅 rustfmt）；禁止运行 Vega、编译、构建、打包、cargo check、Clippy 或测试。测试全交现有 `.github/workflows/pr-check.yml`，不改 workflow 或新增触发基础设施。

| 云端阶段 | 现有命令/证据 |
|---|---|
| RED/GREEN | `cargo nextest list --workspace --message-format json` 后执行 `cargo nextest run --workspace`，保存真实布局断言失败/通过、发现/执行/通过/失败/忽略/跳过数 |
| 每轮完整门禁 | `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、既有三组 Python 测试及 Nextest |
| 删测后 | 同一完整门禁；Rust 清点为 0 后 `cargo nextest run --workspace --no-tests=pass`，明确 `no Rust tests`、0 发现/0 执行，不称空集为测试通过 |
| required check | 最新 head 的 `check (fmt, clippy, test)` 成功，且 Clippy/Nextest 两个 job 成功 |

CI 启动后每五分钟查询一次，不使用短轮询/watch；完成通知或用户即时请求除外。保留失败原始日志，不自动重试到绿。测试自身接线修正先留证审查，不能改变矩阵、放宽断言或 ignore。

PR 必须链接生产基线、冻结测试源码/内容标识、RED run/job 与原因、后续实现提交、相同测试 GREEN、独立删测和最新空集门禁；不可访问或未执行如实标待 CI，不把静态检查当运行通过。禁止 force-push 丢弃唯一测试证据。

## 5. 用户手测与回滚

合并及发布后由用户记录实际版本，按 A07 检查；主会话不为验收在本地运行或安装 Vega。代码合并不等于应用已安装，不等于用户已验收。

仅最新 head 门禁通过后 squash merge，Issue 保持开放并置 In review，保留任务分支/worktree；用户明确通过才执行收尾、关闭和 Done。回滚为经 PR 撤销本卡行级约束；不清除用户置顶数据或修改存储格式。

## 6. v1 → v2 变更记录

- 明确 v1 旧 fixture、模块和几何入口已被 #476 清除，不再要求复用或保留已不存在的历史集合。
- 列明新建的最小挂载测试、临时观测支持与允许文件，保留 A01–A07 产品含义及单行生产修复边界。
- 将 `issue420_` 命名替换为行为命名；改为云端有效 RED 才修复，并采用最新 GREEN 后删测、最终空集门禁的交付生命周期。
- 用户明确授权当前主会话接管原等待 Codex 的方案/审查/集成工作，旧交接、分支和证据不覆盖。
- 冻结前独立审查补齐 A03 首条再次点击、明确 64/480px 内部位移；A04 固定同一挂载实例和先滚到底再取消置顶，明确 32→0px 偏移恢复；同时明确 A05 显示优先级与文件 Store 刷新等待。
- 当前 API 核查确认可从零挂载；测试模块放在 sidebar 子模块，采用动态行 `.test_support()`/Window::find 与固定视口 selector，避免静态 selector 限制和 fade 类型冲突。不需要恢复旧 fixture 或新增生产 API。
- 首轮 [Actions 39238731168](https://github.com/puzige/vega/actions/runs/39238731168) 在测试编译阶段失败，不计有效 RED；`Bounds<ScaledPixels>` 不能调用 `Bounds<DevicePixels>::to_pixels`，应按 scale factor 逐分量转换。当前 facade 宏不需要 `gpui_kit as gpui` 别名，删除该未使用导入。此为测试接线纠错，不改变冻结验收或断言数值；原始提交与日志保留在 PR #496。

## 7. 本轮验证证据

PR：[固定行高修复 #496](https://github.com/puzige/vega/pull/496)。所有执行均来自现有 GitHub Actions，本地未运行 Vega、编译、构建、打包、Clippy 或任何测试。

| 阶段 | 提交与原始证据 | 实际结果 |
|---|---|---|
| 初始先行补丁 | [`c11a2743eb25c56b3b4c7befba0528ba05fe0309`](https://github.com/puzige/vega/commit/c11a2743eb25c56b3b4c7befba0528ba05fe0309)，[run 39238731168](https://github.com/puzige/vega/actions/runs/39238731168) | 测试 API/未用导入导致编译失败，行为断言未执行，不计 RED；保留原始失败 |
| 不改验收的接线纠错及有效 RED | [`344a94f0be7eb4573fbc2375a42d658fbc012500`](https://github.com/puzige/vega/commit/344a94f0be7eb4573fbc2375a42d658fbc012500)，[run 39239881782 / Nextest](https://github.com/puzige/vega/actions/runs/39239881782/job/118017837024) | 4 发现、4 执行、0 通过、4 失败、0 跳过；真实行高六条 26.5px、七条 23px，期望 32px。Fmt、Clippy 和 Python 5+15+14=34 个通过 |
| 有效 RED 后最小生产实现与同一测试 GREEN | [`c56ff793c1e4cf9ea3d56ede44f82743163943e3`](https://github.com/puzige/vega/commit/c56ff793c1e4cf9ea3d56ede44f82743163943e3)，[run 39240961416 / Nextest](https://github.com/puzige/vega/actions/runs/39240961416/job/118021360033)，[required check](https://github.com/puzige/vega/actions/runs/39240961416/job/118023026271) | 4 发现、4 执行、4 通过、0 失败、0 跳过，14 binaries，执行 11.390 秒；Fmt、全目标 Clippy、34 个 Python 测试和 required check 成功 |
| 独立删测与最终交付门禁 | GREEN 证据固定后再提交，最新结果以 PR 原始 run 链接为准 | 仅删本轮测试/独占支持并保留单行生产修复；最终空集完整门禁待 CI，不把 0 测试称作测试通过 |

有效 RED 与 GREEN 使用同一[冻结测试源码](https://github.com/puzige/vega/blob/c56ff793c1e4cf9ea3d56ede44f82743163943e3/crates/vega_ui/src/sidebar/pinned_rows_tests.rs)，Git blob `a99b9483e2b05708ff3369983e8db8fdb4b46c19`，SHA-256 `d1a349e202e9598e26723d7510601932f2162f3c3d0b096e9c74b8667086e5ff`。两提交间全部差异仅共享任务行一条 `.flex_shrink_0()`，主会话和独立审查确认测试内容、预期及支持未追改；原始测试从上述不可变提交找回，不依赖临时分支或会过期 artifact。日志未单独列 ignored，源码无 ignore，不猜测额外计数。

本地已执行的快速静态检查：`cargo fmt --all -- --check`、`git diff --check`、`cargo metadata --no-deps --offline --locked --format-version 1`，退出码均为 0；锁文件静态审查确认开发期仅增加 10 个测试传递包，原 914 个包的版本/来源/checksum 未变。运行/悬停/操作图标的状态容器及几何已测，默认测试资源源不提供真实 SVG 像素验收，不能据此声称图标外观已验证。

A01–A06 对应四组冻结测试均已在带测试 GREEN 执行；A07 原生系统/键盘/文字与图标像素、发布安装及重启验收仍待用户。规格无产品范围偏离；仅上述留证的测试 API 接线纠错。

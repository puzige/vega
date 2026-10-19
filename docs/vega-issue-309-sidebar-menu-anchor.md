# 侧栏菜单跟随实际触发位置

- Issue：[对话右键菜单出现位置不对 #309](https://github.com/puzige/vega/issues/309)。
- 方案版本：v2，2026-10-19，Codex 根据 Vega 的执行前提回报修订并重新交接；v1 仅作历史记录。
- 当前代码基线：`1f70e72d4074c96846119398e52697aaab894d54`，已 fetch/rebase；本次交接只修改本文，没有测试或生产实现改动。不得恢复该基线已删除的历史测试。
- 任务分支：`feat/309-sidebar-menu-anchor`；worktree：`~/Workspace/worktrees/vega-309-menu-anchor`。
- 执行约束：[AGENTS](../AGENTS.md)、[执行总纲](vega-exec-guide.md)、[角色工作流](vega-issue-workflow.md)、[设计守则](vega-design-guidelines.md)、[测试先行规则](vega-rust-test-reset-tdd.md)。

## 1. 问题与范围

原始附件标注的操作是点击项目行末尾的“…”；实际出现的是“项目操作 / 移除项目”菜单，菜单远离该行，落在侧栏上方。Issue 标题称为对话右键菜单，但截图可直接证明的是项目菜单。本文以截图和实际代码链冻结范围，不将消息正文的右键复制菜单并入本卡。

本卡修复共用 `render_organization_menu` 的项目、排序/归档及分组菜单定位。主页面的项目“…”和最近任务排序按钮是当前用户入口；旧投影中项目/分组的按钮及右键入口也调用同一个菜单打开分支，须同步提供正确锚点，不重新启用旧投影。

任务行自己的 `render_row_actions` 已在触发器局部定位；本卡为它保留一项回归，不重写该菜单。菜单项、项目移除/分组/排序的业务及持久化语义、权限和导航守卫、主题、尺寸、文案均不变。不升级 GPUI、不改 schema、runtime、发布或 CI 流程，不增加说明文案或成功提示。

v2 明确允许在已挂载组织视图的根元素补充必要的 Tab/Shift-Tab 焦点遍历，保证从该区域内的现有焦点停靠点能够到达项目“…”及排序按钮。这是键盘菜单入口的执行前提，不依赖默认 Kit Root。范围仅为组织区域内冒泡的按键，不新增全窗口 Tab 路由、不改变 Composer/Settings 快捷键或关闭菜单后的既有焦点策略。

## 2. 验收矩阵（先于实现计划）

下列自动回归均使用进程内 GPUI 测试平台、真实生产渲染器和事件分派，只在 Actions 执行。测试通过实际菜单边界及可观察终态判断结果，不查源码字符串，不只断言菜单状态字段。

| 用例 | 前置与操作 | 可观察结果与证据 | 层级 / 当前状态 |
|---|---|---|---|
| M1 项目按钮 | 挂载 Sidebar，至少两个项目且中间有足够任务使下方项目明显远离顶部；分别点击两个项目“…” | 空间足够时菜单右边与对应按钮右边对齐、顶边与按钮底边对齐；打开下方项目的菜单跟随下方按钮，目标操作属于该项目。菜单打开不折叠、选择或打开背后项目/任务 | GPUI 几何及行为；待写测试 / 待 CI |
| M2a 键盘入口可达 | 在真实组织渲染中，以另一个已绘制的非目标焦点停靠点作为前置焦点；通过 Tab 到项目“…”及排序按钮，以 Shift-Tab 验证反向遍历；指针留在窗口另一处 | 实际焦点随 Tab/Shift-Tab 按现有停靠顺序移动，能到达目标按钮并用 Enter/Space 打开对应菜单；遍历本身不打开、折叠或变更项目/任务，不写入草稿。禁止直接聚焦待验收按钮替代遍历 | GPUI 真实按键分派及焦点；待写测试 / 待 CI |
| M2b 键盘锚点与菜单内导航 | 将入口可达性与菜单几何分开测试：可直接建立目标按钮焦点作为本项前置，分别 Enter、Space 打开项目及排序菜单 | 与鼠标点击使用同一按钮锚点，绝不使用指针位置；菜单内箭头/Tab/Shift-Tab 导航及 Enter 激活保持有效，Tab 不把焦点跳出菜单；Escape 关闭且不传播给外层 CloseSettings 路由。编辑态及拖动期间新增根路由不得抢占既有按键 | GPUI 事件、焦点及几何；待写测试 / 待 CI |
| M3 其他共用入口 | 点击最近任务排序按钮；在测试容器挂载现有旧项目/分组投影，通过按钮及行右键打开菜单 | 按钮入口对齐实际按钮；右键入口以本次鼠标事件的窗口坐标为起点。内容与对象正确，左右键入口都不复用上次项目的锚点；不为测试把旧投影加入产品页面 | GPUI 生产投影与菜单集成；待写测试 / 待 CI |
| M4 滚动及重开 | 在多个项目/任务溢出侧栏的 fixture 中滚动，关闭后重新打开下方项目菜单；调整侧栏宽度后重开，再调整窗口大小 | 每次以当前已布局按钮的实际边界定位，不按项目序号估算 y，不复用首次坐标；窗口内菜单表面完整可见，溢出按既有规则避让；窗口缩小时已有菜单重新执行避让 | GPUI 布局及事件；待写测试 / 待 CI |
| M5 窗口边缘与长菜单 | 在 960×600 窗口、240/365 侧栏宽度以及浅/深主题下打开靠下按钮；另用 320 高的组件测试窗口挂载旧分组菜单，覆盖超过最大高度的滚动 | 表面边界在窗口内；保留现有宽度、最大高度和滚动，末尾项通过滚动或键盘可达且能正确执行；浮层不被侧栏滚动遮罩截断。320 高仅用于组件测试，不修改产品最小窗口 | GPUI 几何、命中及滚动；待写测试 / 待 CI，系统外观待用户手测 |
| M6 关闭、切换及执行 | 选中 A，先打开项目 A 菜单，再打开 B；Escape/外点关闭；重开 B 后点击“移除项目”；使用 owned 项目和未发送草稿 | 切换后只有一个组织菜单且锚点属于 B；关闭后消失；菜单内部点击不误触背后行，命令仅对 B 生效，A、任务历史和草稿保留；重开使用新锚点并保留现有高亮初始化行为 | GPUI 与现有文件库 fixture；待写测试 / 待 CI |
| M7 原任务菜单回归 | 通过任务行右键和键盘入口打开原有完整任务操作菜单，Escape 关闭；再通过项目按钮打开组织菜单 | 任务菜单仍在自己的触发器旁，任务命令和关闭行为不变；组织菜单打开仍执行现有 `close_actions`，不留两个重叠菜单；不改消息正文选择/复制路径 | GPUI 回归；待写测试 / 待 CI |

M1–M7（包含 M2a、M2b）可合并为少量有区分度的测试函数，但测试阶段必须写齐上述目标。所有新增用例名称统一以 `organization_menu_placement_` 起首并描述行为，非文档代码不含本卡编号。边界比较使用 GPUI 逻辑像素；允许锚点取整带来的不超过 1px 差异，不用固定截图坐标替代实际触发器边界。

第一份测试提交应分开取得两类有效行为 RED：M1/M2b 的固定锚点几何错误，以及 M2a 的 Tab 不移动焦点；不要因为入口先失败而让全部几何断言无法执行。不要求所有回归项均 RED。测试平台/编译/依赖故障不算此行为 RED。矩阵不是执行结果，目前未取得测试发现或执行计数。

## 3. 根因与代码证据

当前基线上的调用链（使用符号定位，行号会随清理变化）：

1. `organization/render.rs::render_pi_project` 在 `project-more-{id}` 按钮调用 `vector_control`，动作是 `Control::Menu(OrganizationMenu::Project(...))`。
2. `vector_control` 的鼠标和键盘处理器只转交动作，丢弃鼠标位置，也不传按钮的已布局边界。`organization_control` 以及 `projections.rs` 的右键入口有相同问题。
3. `control_action` 的 Menu 分支只保存菜单种类、重置索引并聚焦菜单，没有锚点信息。
4. `render_organization` 将组织菜单统一挂在组织容器下，而非各触发按钮下。
5. `organization/menu.rs::render_organization_menu` 使用 `Anchor::TopRight`、`AnchoredPositionMode::Local`，位置固定为 `(sidebar_width - 16, SIDEBAR_LINE_HEIGHT)`。项目在列表中下移或滚动后，此 y 完全不变。

因此本问题是打开事件未携带锚点、渲染又固定取容器顶部坐标，不是项目数据、字体缩放或 `deferred` 导致的位置偏移。截图符合此调用链；本地未运行应用复现，不将静态分析写成原生复现通过。

已核对锁定依赖：`Cargo.lock` 中 `gpui-kit 0.6.1` / `gpui-pre 0.3.4` 的 `elements/anchored.rs`。`Window` 模式将 `.position` 解释为窗口内坐标；`Local` 模式在此基础上加元素布局原点。`deferred` 只延后绘制、保留计算出的布局偏移。旧设计守则提到的依赖版本不作为本卡 API 事实。

另一个 API 陷阱：`Div::on_children_prepainted` 回传的是**子元素**边界列表，不能把图标的边界当成整个按钮。下文要求通过铺满按钮的 canvas 记录真实按钮边界。

### v2 确认的键盘前提缺口

Vega 的[阻碍回报](https://github.com/puzige/vega/issues/309#issuecomment-6253515851)成立：`ThreadsBlock::render` 在 organization 分支提前返回，后面的旧列表 `tab → window.focus_next/focus_prev` 不会生效；组织按钮只有 `focusable/tab_stop` 与 Enter/Space，组织根没有 Tab 路由。应用在 `crates/vega/src/main.rs` 直接挂载 VegaWindow，`window/render.rs` 没有补上通用 Tab 路由。菜单内部已有 Tab 处理，不能用它证明触发器可达。

锁定 GPUI 的 `div.rs::paint_mouse_listeners` 为可聚焦元素注册了真实 mouse-down 聚焦；因此手测可先点击项目行取得组织区域内的起始焦点，再用 Tab 遍历，不要求通过新增全窗口路由从 Composer 进入侧栏。测试可以显式建立另一个已绘制停靠点的初始焦点，但必须通过真实 Tab/Shift-Tab 到达待验收按钮。

## 4. 冻结的定位行为

- 所有坐标统一为窗口内逻辑像素，不使用屏幕坐标、Retina 物理像素或项目行号。
- 鼠标点击按钮与键盘激活按钮：锚点是该按钮最近一次 prepaint 的右下角，菜单使用 `Anchor::TopRight`。常规位置是按钮下方、右边对齐。
- 项目/分组行右键：使用该次 `MouseUpEvent.position`，菜单使用 `Anchor::TopLeft`，常规位置从指针处向右下展开。仅修复已有入口，不给主页面额外增加右键功能。
- 打开时替换本次菜单和锚点，保留现有高亮初始化、菜单聚焦和关闭任务菜单行为。关闭后旧锚点不得用于下一次打开，不扩展本卡到重写菜单滚动策略。
- 窗口边缘避让继续由已有 `anchored().snap_to_window_with_margin(px(8.))` 处理，保留 `Layout::TASK_MENU_WIDTH`、现有 `menu_height` 与滚动能力。这里的 8px 是溢出避让边距，不保证所有未溢出的菜单到窗口边缘至少 8px。
- 位置在本次打开时确定。侧栏滚动或宽度变化后的重开必须取新边界；本卡不新增打开期间连续跟随触发器的动画或全局观察器。窗口改变大小时每帧仍执行原有窗口避让。
- 保留菜单表面的 `occlude`、内部事件阻断、外点/Escape 关闭以及仅对菜单表面的 `deferred(...).with_priority(3)`。不能延后整个窗口锚点计算而重新引入滚动/布局偏移。
- 在 `render_organization` 的根元素增加 bubble 阶段 Tab 路由：仅处理没有其他修饰键的 Tab / Shift-Tab，分别调用现有 `window.focus_next` / `focus_prev`，处理后停止传播。不要使用 capture 阶段抢在子菜单或输入框之前执行。
- 组织菜单或任务菜单已打开、任务/分组正在编辑、重排拖动进行中时，不执行新增根遍历；保留各自已有键位和状态处理。当前焦点在组织区域之外时，该处理器不接管事件；不调用普通动作、导航、保存或取消，不自建第二份焦点顺序。

### 坐标算术核对

以下是逻辑像素的合成算例，菜单宽 240、**示例高度** 100；100 不是新增高度 token，实际测试从渲染结果取得菜单高度。矩形写作 `(left, top, width, height)`。

M5 的 320 高组件窗口沿用现有上限：`menu_height = max(320 - 16, 32) = 304`；足够多的既有分组操作超过此高度后，应在 304 高的表面内滚动，不新增测试专用菜单项。

| 场景 | 输入及锚点 | 未避让菜单边界 | 最终边界及验算 |
|---|---|---|---|
| 普通按钮，1200×760 窗口 | B=(280,300,24,24)，右下角=(304,324) | (304−240,324,240,100)=(64,324,240,100) | 右边=304，顶边=324，底边=424，小于760，无需避让 |
| 同一按钮下移160 | B=(280,460,24,24)，锚点=(304,484) | (64,484,240,100) | 菜单顶边也移动160；固定顶部算法不能满足 |
| 底部按钮，1200×760 窗口 | B=(280,710,24,24)，锚点=(304,734) | (64,734,240,100)，底边834 | y=734−(834−760+8)=652，底边752=760−8 |
| 窄侧栏导致左溢出 | 锚点=(224,324)，TopRight | (−16,324,240,100) | 左溢出后x=8，右边248；允许浮层越出侧栏，仍在窗口内 |
| 右键靠近窗口右下，960×600 | P=(950,590)，TopLeft | (950,590,240,100)，右1190/底690 | x=950−(1190−960+8)=712；y=590−(690−600+8)=492；右952/底592 |

## 5. 实现计划与允许范围

**先完成第 6 节的测试交接和有效 RED，再执行本节生产修改。**

1. `crates/vega_ui/src/sidebar/threads_block/organization.rs`：为打开的组织菜单保存窗口坐标与菜单角的成对锚点；仅属于 UI 私有状态，不写入 `SidebarOrganizationSnapshot`、数据库或跨 crate 类型。在初始化和菜单关闭/替换路径保持菜单与锚点一致，不能以旧值或零值兜底打开。
2. `organization/render.rs`：让菜单打开入口显式接收菜单目标、窗口点及锚定角，再保留 `close_actions`、高亮初始化和焦点逻辑。调整 `Control::Menu` 的分派/参数，使无锚点动作不能打开菜单；非菜单动作行为不变。
   同文件 `render_organization` 的 `sidebar-organization` 根元素按第 4 节补齐最小 Tab/Shift-Tab bubble 路由及菜单/编辑/拖动 guard。只在这个已挂载入口补接，不把旧列表 handler 移到整个 `ThreadsBlock` 或 VegaWindow，不改旧投影/应用外层的键盘架构。
3. 在 `vector_control` 和 `organization_control` 的菜单按钮路径使用按钮内部、绝对铺满按钮的非交互 canvas，在 prepaint 将完整 `Bounds<Pixels>` 写入本次元素与事件处理器共用的轻量 `Rc<Cell<Option<Bounds<Pixels>>>>`。按钮提供 relative containing block；canvas 不占布局、不注册遮挡鼠标的 hitbox。鼠标左键和 Enter/Space 读取该边界的右下角，再打开菜单。无需新增全局边界缓存，也不使用仅测试可用的 `debug_bounds` 定位生产菜单。
4. `organization/projections.rs`：为已有项目/分组右键入口传本次 `event.position` 和 `Anchor::TopLeft`；按钮路径依照上一步。不要为此启用当前未挂载的投影或调整视图选择逻辑。
5. `organization/menu.rs::render_organization_menu`：读取本次锚点，改用 `AnchoredPositionMode::Window` 和对应的角；移除固定 `(sidebar_width - 16, SIDEBAR_LINE_HEIGHT)` 定位。保留现有布局尺寸、可滚动表面、绘制层级、焦点、键盘及菜单动作路径。关闭处同时清理本次锚点；不得增加与定位无关的业务变化。
6. `organization/reorder.rs`：仅在现有开始拖动时关闭组织菜单的路径，同步清理锚点或调用统一关闭入口；不改拖放判断、顺序或边界缓存。
7. 只做快速格式/差异静态检查，交回同卡，交由 Codex 审查冻结测试未变及同一目标的 GREEN。

测试文件建议为 `organization/menu_placement_tests.rs`，仅在 `organization.rs` 增加相应 `#[cfg(test)]` 接线。当前基线已清理旧集合，应新建本卡需要的最小 owned fixture，不从旧 ref 复制整套历史测试或共享支持。`vega_ui` 当前 dev-dependencies 已含 GPUI test-support 和 tempfile；优先直接复用，禁止恢复被清理的 `vega_conversation/test-support` 等旧开关、升级版本、引入新依赖或修改 workflow。确有额外测试接线需求时先在同卡列清必要差异交 Codex 审查。

测试阶段允许在上述生产文件及 `sidebar/mod.rs` 加入本卡必需的 **`#[cfg(test)]` 观测接线**（例如仅测试编译的 `debug_selector`），以观察实际触发器、菜单表面、项目行、滚动容器和相关菜单项；普通 `.id()` 不会自动进入 `debug_bounds`。观测接线不得改变正常构建、布局、命中、焦点顺序或事件处理，不得提前加入 canvas 锚点或 Tab 路由等生产修复。观察的是生产输出，不用探针替代位置/焦点决策。

GPUI fixture 外层必须使用 `VegaWindow` key context 和与实际应用一致的 Escape → CloseSettings 绑定及外层处理器，以可观察外层状态断言菜单关闭不向外传播。此处只复现既有接线，不要求改 `crates/vega`、引入 Kit Root 或扩大生产 public API；M2a 也不能由 fixture 自己处理 Tab 来替生产过关。

生产行为改动限于上述五个文件；`threads_block.rs`、`sidebar/mod.rs`、主题 token 和服务/存储实现以现状做回归。`sidebar/mod.rs` 仅允许上文列明的测试观测接线，不加生产键盘处理或布局变化。测试自身缺陷先留证审查，验收含义变化必须停下重开测试先行周期。代码不新增注释，除非是仓库允许的 unsafe SAFETY 一行。

## 6. 测试先行交接与 Actions

### Vega 第一阶段：仅测试

领取前重读 Issue 最新交接并核对无人占用，置 In progress。复用本文任务分支/worktree；本地没有该 worktree 时在约定目录从远端同名分支创建。先 fetch 并按仓库规则更新基线，保护任何他人改动。若基线变化，记录实际新基线、复核本文调用链，不能沿用旧 SHA 伪装已验证。

生产未改时写齐 M1–M7（含 M2a/M2b），提交测试及必要的测试独占接线；本地不运行 Vega、编译或执行测试。回报实际基线、测试 commit/补丁、测试目标清单及静态检查结果后停止写入：保持 In progress，阶段“等待 Codex 审查测试 / RED”，当前执行者待领取，下一执行者 Codex。此时不能开始第 5 节的生产修改。

### Codex：建立同一个测试先行 draft PR

审查测试确实驱动生产渲染和可观察结果、生产逻辑尚未变更后，以 base `master` 建立/复用 draft PR，触发现有 `pr-check`。不另建测试基础设施，不用自动关闭 Issue 的关键字。

下列命令**只供 Actions/云端运行**；目标过滤仅说明需要辨认的本卡测试，不替代现有 workflow 的完整门禁：

```sh
cargo nextest run -p vega_ui -E 'test(organization_menu_placement_)'
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/release_tests.py
python3 scripts/prepare_computer_use_tests.py
python3 -m unittest discover -s scripts/tests -v
cargo nextest list --workspace --message-format json
cargo nextest run --workspace
```

现有 Actions 自动执行完整门禁，不修改 workflow 去添加上面的定向命令。原始 Nextest 日志须能辨认本卡测试名称、几何断言失败和真实数量。新增测试后不应将本卡当空集；发现数、执行数、忽略/跳过数分别记录，未取得写未取得。required check 继续为 `check (fmt, clippy, test)`。

Codex 取得固定锚点与缺失 Tab 路由这两类有效行为 RED 后，在同卡给出实际 run/job/测试提交及失败原文链接，冻结测试目标/数据/断言，再明确交 Vega 进入生产实现。云端暂不可用、RED 原因不相关或基线发生漂移时停在此阶段，由 Codex 处理；不本地绕过。

### Vega 第二阶段与 Codex 集成

只实现第 5 节，不能在实现阶段或实现后补测、变更期望、ignore 或自动重试换绿。完成后回报实现 commit、修改文件、静态检查退出码、测试未追改证据及待 GREEN 项，停止写入并交 Codex。Codex 在同一个 PR 确认同一测试 GREEN、待合并最新 head 的完整 required check 成功，才 squash merge；CI 每 5 分钟查询，通知或用户即时要求除外。

合并后为 In review、Issue 保持开放，用户手测前不删除任务分支/worktree。当前仅为方案交接：未写测试、未取得 RED/GREEN、未开本卡 PR、未修改生产代码。

## 7. 用户手测与回滚

合并并由云端发布后，用户在实际安装版本验收：

1. 找到侧栏下方项目，点击“…”；确认菜单紧邻该按钮。换上方项目再打开，确认位置和操作对象都切换。
2. 滚动项目列表、拖动侧栏宽度、调整窗口尺寸后重开；在底部项目确认菜单完整可见、菜单项可点且不触发背后项目。
3. 先点击项目行取得组织区域内的起始焦点，再把指针移到会话区域。用 Tab/Shift-Tab 到项目“…”及最近任务排序按钮（中间可能有既有任务停靠点），Enter/Space 打开、菜单内 Tab/箭头选择、Escape 关闭；确认位置跟随键盘按钮，路由和草稿保留。本步骤验收组织区域内遍历，不宣称 Composer 与侧栏之间新增全窗口 Tab 导航。
4. 浅/深主题及最小窗口分别检查菜单不被裁切，原任务行右键菜单仍可用。仅用可移除的测试项目验证移除命令，保留真实任务和用户资料。

若定位或点击不符合本文，记录触发对象、入口、窗口/侧栏尺寸及脱敏截图，退回 In progress，由 Codex 修订方案后重开测试先行周期。回滚为通过 PR revert 本卡最终 squash 提交，不改用户数据；坐标是瞬态 UI 状态，没有数据迁移或配置回滚。

## 8. 交接证据

- 领取记录见 Issue；实际交接 commit 与本文 permalink 由发布本方案后的最新交接评论列出，不能在本文预造 SHA。
- 本轮只静态读取用户截图、生产调用链、锁定 GPUI 源码及现有 CI 定义，并核对上表算术。
- 本地验证：仅文档差异、空白和引用检查；准确结果在交接评论记录。
- 测试补丁、Actions RED、冻结记录、生产实现、同一测试 GREEN、最新 head 门禁及用户手测：均待后续阶段，不将方案发布或看板 Ready 记作修复完成。

## 9. 版本与重新冻结记录

- [v1](https://github.com/puzige/vega/blob/d480a8e1a7c453b5da8254c817133fb5ef48f5ce/docs/vega-issue-309-sidebar-menu-anchor.md)：首次定位固定菜单坐标并给出 M1–M7；错误地把 Tab 到触发器当成已有入口前提。
- [Vega 交回](https://github.com/puzige/vega/issues/309#issuecomment-6253515851)：同步到 `1f70e72d` 后确认前提缺口，未提交测试或生产代码，已停止写入。
- v2：保留定位及键盘目标，明确补齐组织根 Tab/Shift-Tab；将 M2 分为入口可达和独立几何/菜单导航，补充边界 guard、测试观测和真实 Escape 作用域要求，并同步手测起点及允许范围。属于**生产/测试都尚未开始时的显式验收修订**；不追认 v1 测试或 RED，没有修改已冻结断言。
- 分支上 Vega 重放的 v1 文档补丁已与原远端提交经 range-diff 核对一致。本轮只更新该文档交接提交；推送使用旧远端 SHA 的精确 lease，若分支出现并发更新即停止覆盖并重新核对。历史 v1 链接及两次交接评论保留。

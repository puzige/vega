# #309 v3 — 按消息驱动顺序契约修复组织菜单锚点

版本：**v3，2026-10-19，Codex 已审查并冻结本版验收及允许范围**。用户明确要求更新并冻结 v3，然后直接触发 Vega 从新 master 重新先测后修、保留旧证据。

- Issue：[puzige/vega#309](https://github.com/puzige/vega/issues/309)。
- 本轮生产基线：`b32ad22e530d27925e375df00b6bd401bf9646a3`；创建分支前已 fetch，包含 #308 的 `84baff54` 及后续 Markdown、子任务生命周期更新，没有本卡旧生产修复。
- 新任务分支：`feat/309-menu-anchor-v3`；worktree：`~/Workspace/worktrees/vega-309-menu-anchor-v3`。本次冻结提交**只有本文，没有 Rust 测试或生产变更**。
- 旧轮：[Draft PR #489](https://github.com/puzige/vega/pull/489)，旧分支 `feat/309-sidebar-menu-anchor`、旧 worktree 和备份全部保留；旧最终 head 为 `d516604a0395775b742920623a58ffae0646abf8`。
- 执行总纲：[AGENTS](../../AGENTS.md)、[exec-guide §7.2](../vega-exec-guide.md#72-test-first-与有效测试当前规则)、[工作流](../vega-issue-workflow.md)、[设计守则](../vega-design-guidelines.md)、[Rust 开发/交付生命周期](../vega-rust-test-delivery-lifecycle.md)。本地不运行 Vega、不编译、构建、Clippy、打包或执行任何测试。

## 1. 本轮契约与非目标

沿用 [#308 消息顺序契约](../308/vega-issue-308-message-order.md) 及其[归档与交付修复](../308/vega-issue-308-review-repairs.md)：新正式消息置前，手动拖动可重新排列；打开会话、重命名、同一消息流式更新不能重新按时间排序。更新时间/创建时间/手动排序模式菜单已经退役，原位置为直接归档按钮。旧 sort enum 仅作兼容，不重新启用时间排序。

#309 原始截图证明的是点击侧栏项目“…”后，项目菜单出现在上方而非触发器旁。本版仍只修复**现存项目/分组组织菜单的定位与必要键盘入口**，并验证与 #308 的兼容性。旧投影仍有项目/分组按钮和行右键接线，须正确传锚点，但不重新把旧投影加回产品页面。

不恢复 Filter、ToggleArchive、ToggleSort 或排序菜单，不修改存储顺序、迁移、消息提交/失败语义、模型、权限、通知、复制、Markdown 或子任务行为。原任务行菜单只做回归；不重写它或正文复制菜单。菜单内容、尺寸、颜色、文案和业务命令沿用新 master，不加说明或成功提示。

## 2. 验收矩阵（先于实现）

以下是本轮完整冻结目标，使用进程内 GPUI + owned 文件库，只在 Actions 执行。通过实际布局/焦点/事件、真实归档构造元素和公开存储/组织服务结果观察，不查源码字符串，不用 fixture 替代生产的锚点、Tab、Click 或排序决策。

| ID | 前置与操作 | 冻结的可观察结果 | 本轮预期与状态 |
|---|---|---|---|
| V1 项目锚点 | 挂载实际 Sidebar，至少两个项目，中间有任务使按钮分处不同高度；分别点击项目“…” | 空间足够时菜单右边/顶边对齐对应按钮右边/底边，后一个项目跟随其自身按钮；目标正确，打开菜单不折叠、选择、打开背后对象，不改草稿/顺序 | 新基线应有几何 RED；待测试/CI |
| V2 键盘可达 | 从组织区域内另一个已绘制停靠点经真实 Tab/Shift-Tab 到项目“…”；直接归档按钮也能从相邻控件到达。指针留在窗口另一处 | 实际焦点按现有停靠顺序前后移动；遍历自身不执行按钮动作、不改任务/草稿/持久顺序。项目按钮 Enter/Space 打开正确菜单；不能直接 focus 目标冒充 Tab 可达 | 项目/根遍历应有独立焦点 RED；归档作为兼容回归；待测试/CI |
| V3 键盘几何和菜单导航 | 独立建立项目按钮焦点后 Enter/Space 打开，避免入口失败掩盖几何；用现存项目/分组菜单做箭头、Tab/Shift-Tab、Enter/Escape | 菜单用按钮而非指针锚点；内部导航环绕并保留菜单焦点，Escape 关闭且不触发 VegaWindow 的外层 CloseSettings。至少一次通过现存项目上移/下移命令验证项目顺序结果，任务顺序/历史/草稿及路由保留；不再写 Created | 独立几何 RED + 菜单回归；待测试/CI |
| V4 共用旧入口 | 在测试容器挂载现存旧项目/分组投影，通过按钮和行右键分别打开菜单 | 按钮对齐完整触发器，右键使用本次事件的窗口内坐标；切换对象/入口不复用旧锚点。不能为测试重新启用旧页面 | 新几何 RED/回归；待测试/CI |
| V5 重开与边缘 | 滚动侧栏、改变侧栏宽度后关闭重开；打开时调整窗口。覆盖960×600、侧栏240/365、浅/深主题的有区分度场景，320高组件窗口覆盖长分组菜单滚动 | 每次打开读当前按钮完整边界；窗口变化继续避让。表面完整可见、可命中，不被侧栏遮罩裁切；长菜单末项经滚动/键盘可达且可正确执行。320只用于组件测试，不改产品最小窗口 | 几何/滚动/命中；待测试/CI |
| V6 关闭与准确目标 | 选中A，先开A后开B；Escape/外点关闭；重开B并移除owned项目B | 仅一个组织菜单，锚点/命令属于B；关闭不穿透到背后行；移除保留A、任务历史、草稿和本地目录，沿用既有解绑语义。关闭、拖动开始、分组编辑等原关闭路径不留下可复用旧锚点 | 行为/持久化回归；待测试/CI |
| V7 按键边界 | 菜单已开、任务标题编辑、分组编辑或重排拖动期间按Tab；另测带其他修饰键的Tab及组织区域之外的焦点 | 新组织根处理器不抢占各状态的既有行为，不保存/取消编辑、不隐式变更顺序；外部焦点不由本卡接管；保留重排投影清理和Escape路由 | guard回归；待测试/CI |
| V8 原任务菜单 | 通过原任务行右键/键盘打开任务操作菜单、执行代表命令及关闭，再打开项目菜单 | 任务菜单原位置、业务目标、命令和关闭行为保留；打开组织菜单仍关闭旧任务菜单，不留重叠菜单，不改复制或正文选区生命周期 | 回归；待测试/CI |
| V9 直接归档兼容 | 真实归档构造元素在关闭/开启两状态；通过鼠标点击、实际Enter/Space完整按下/松开或辅助功能Click激活 | 沿用 `Role::Button + on_click`、真实名称“显示已归档/隐藏已归档”、Click/Focus支持和选中态；每次只切换一次，正确显示/隐藏归档项，焦点/草稿正常，不弹旧排序菜单。只走Tab不切换 | 新基线预计GREEN的保护断言；待测试/CI |
| V10 新顺序兼容 | owned同区A/B/C，声明初始顺序；通过实际消息insert使C置前，再经组织服务手动将C移回；在此期间开关菜单、Tab/直接归档、visit/rename及同一消息正文更新；再插入另一条新消息 | 完整ID顺序按场景直接声明：A/B/C → C/A/B → A/B/C，菜单/Tab/归档显隐/访问/重命名/同消息更新不改持久顺序，下一条新B消息才得到B/A/C。Manual及其他区域、绑定、消息/草稿保留；不引入Created时间排序或UI二次置前 | public store/service + 实际UI组合兼容回归；待测试/CI |

V3 项目菜单的代表场景可使用两项目A/B：打开A的菜单，导航至“下移”并激活，期望项目顺序B/A，任务的完整区域顺序及业务数据不变。导航环绕按**本次真实菜单项数**断言，不保留旧四项排序菜单的索引常量。

V9 的语义观察沿用 #308 已明确的真实元素边界：`Element::a11y_role` / `write_a11y_info` 写入的节点名称和 Click/Focus 支持，不能用手造假按钮或键盘可达性代替。按键测试包含 KeyUp，避免把“只派发 KeyDown 没有产生标准Click”误判成产品失败。不恢复旧整套辅助功能/应用测试集合。

V10 的同消息正文更新应选用该API实际支持的消息状态，并查回新正文确认更新已发生；不能以无效ID、零行更新或未提交变化充当“不重新排序”的证据。

本轮可将场景组合成少量有区分度的用例，不预先承诺测试数量，不用“旧19−4”代替实际清点。行为名以 `organization_menu_placement_` 开始，不把Issue编号放进非文档代码。V1与V2/V3须隔离到足以分别取得**菜单几何缺失**和**真实Tab不移动焦点**两类有效行为RED；V9/V10是保护既有新行为，不人为制造RED。无关编译/fixture/依赖故障不算有效RED。

## 3. v2 退休场景与旧证据

以下四项在旧基线正确，但与 #308 新契约互斥；作为**旧轮历史场景退休**，不是删除失败测试换绿，也不是本轮fixture修正：

| 旧冻结用例（省略共同前缀） | 本轮处理 |
|---|---|
| `shift_tab_and_tab_reach_sort_menu_from_neighbor` | 排序菜单不再存在；新V2单独验证项目菜单及直接归档的真实遍历 |
| `keyboard_sort_anchor_ignores_pointer` | 不要求归档按钮弹菜单；现存菜单的键盘几何由V3验证 |
| `menu_navigation_traps_focus_and_activates_visible_choice` | 退休四项排序菜单及Created持久化预期；V3基于现存菜单命令建立新场景 |
| `sort_uses_current_button_and_applies_sort` | 不恢复Filter/ToggleArchive/时间排序；新V9/V10验证直接归档和消息/手动顺序 |

其余15项可以作为测试设计/源码来源逐项评估，但**没有本轮通过证据**，也不保证无需适配。允许在生产未改的本轮测试阶段，从已归档源码选择兼容目标及最小fixture；须记录来源、必要适配及新预期，不整文件/整目录恢复旧ref，不将新场景伪装成旧源码未变。测试阶段写齐V1–V10后经本轮RED审查冻结，之后禁止追改。

旧证据的固定索引：

| 证据 | 不变来源 |
|---|---|
| v2原文 | [f850a94f的完整v2](https://github.com/puzige/vega/blob/f850a94fd1f841162df691b6b04c68cb6e7e893a/docs/vega-issue-309-sidebar-menu-anchor.md) |
| 原始测试/完整支持补丁 | [PR #489来源与补丁索引](https://github.com/puzige/vega/pull/489)；原提交 `39f13272e818d8b5ed951e6a82a1db8ac6ad59bb`，冻结测试head `9bd60bcc6d1856e06b17d1ae8909c4ae422a1821` |
| 冻结完整源码 | [core 11项](https://github.com/puzige/vega/pull/489#issuecomment-6259506641)，SHA256 `0154411aca7ee8f16cd48e9bf5ce75d300501c51f9ab8e4f617c68f6e76d9cc3`；[extended 8项](https://github.com/puzige/vega/pull/489#issuecomment-6259507112)，SHA256 `e3f2fb36b486eaf5129d9a7573a74970ac48f21bacccbb013cfded5b8aebc923` |
| 旧有效RED | [39199152656](https://github.com/puzige/vega/actions/runs/39199152656)：19执行、7通过、12失败、0跳过；[冻结记录](https://github.com/puzige/vega/pull/489#issuecomment-6259675579) |
| 旧同批GREEN | head `492cd411982c127d2796235a06291294a521c628` / [39203720922](https://github.com/puzige/vega/actions/runs/39203720922)：19发现/执行/通过、0失败/跳过；[持久原始证据](https://github.com/puzige/vega/pull/489#issuecomment-6260364844) |
| 旧独立删测/最终门禁 | head `d516604a0395775b742920623a58ffae0646abf8` / [删测审查](https://github.com/puzige/vega/pull/489#issuecomment-6260664219) / [39206827156](https://github.com/puzige/vega/actions/runs/39206827156)：0发现/0执行，no Rust tests；[原始证据](https://github.com/puzige/vega/pull/489#issuecomment-6260745763) |
| 冲突审查 | [Vega回报](https://github.com/puzige/vega/pull/489#issuecomment-6260844748)、[Codex核验与结论](https://github.com/puzige/vega/pull/489#issuecomment-6261299528) |

上述GREEN/空集门禁只证明旧生产基线 `f054c35874513bab4c65d7e179fdac6758503182` 的旧轮结果，不覆盖本轮；旧测试、源码、run、PR、分支/worktree/备份不删除、不改写、不伪装成新证据。

## 4. 新基线根因及冻结的生产行为

新 master 的调用链仍为 `render_pi_project → vector_control(Control::Menu) → control_action → render_organization_menu`。打开分支没有按钮锚点，菜单仍以 Local 模式固定 `(sidebar_width−16, SIDEBAR_LINE_HEIGHT)` 定位。组织渲染提前返回，旧非组织列表的Tab处理不会生效，当前组织根没有相应bubble遍历。V1/V2的缺失因此仍可在未移植本卡修复的基线上测试。

修复行为与边界：

- 坐标只用窗口内逻辑像素。按钮鼠标/键盘打开使用本次已绘制**完整按钮border box**右下角 + `Anchor::TopRight`；已有行右键用本次 `MouseUpEvent.position` + `Anchor::TopLeft`。
- 菜单用 `AnchoredPositionMode::Window`，保留现有240宽token、最大高度/滚动、`snap_to_window_with_margin(8)`、occlude、内部事件阻断、外点/Escape关闭和仅菜单表面的deferred priority3。8是既有溢出避让边距，不承诺所有未溢出位置都有8px边距。
- 菜单种类与锚点成对保存，打开替换并初始化高亮，关闭/拖动/编辑等原关闭路径清理锚点；不使用旧值、零点或当前指针兜底键盘打开。每次重开取新布局；打开期间只沿用窗口避让，不新增全局跟随动画/观察器。
- 组织根新增仅Tab/Shift-Tab的bubble处理，使用现有focus_next/focus_prev，不自建焦点顺序；其他修饰键、组织之外焦点、菜单、任务/分组编辑及重排拖动由原处理器负责。保留 #308 的capture阶段重排投影清理，不挪到应用或整个ThreadsBlock。
- **保留新master的真实构造链**：`vector_control -> Stateful<Div>`、`archive_control`、归档专属 `Role::Button + on_click`、选中态/名称及保存后刷新。归档分支不同时注册原MouseUp/KeyDown再重复Click，不被菜单适配器截走；其他按钮行为不变。
- 沿用 `Layout::SIDEBAR_ACTION_SIZE`，当前为**24**逻辑像素；早前审查评论中“28px token”为数值误述，本版纠正，不据此改大小。任务菜单独立的几何不作为该token依据。

### 数值验算

合成示例使用菜单W=240、**示例**H=100（实际高度由渲染观察，不新增高度token），按钮24×24，矩形记作(left,top,width,height)：

| 场景 | 计算与结果 |
|---|---|
| B=(280,300,24,24)，1200×760窗口 | 锚点(304,324)，菜单(304−240,324,240,100)=(64,324,240,100)，右边304/底边424 |
| 按钮下移160 | B=(280,460,24,24)，锚点(304,484)，菜单顶边484，比前案也下移160 |
| 底部按钮B=(280,710,24,24) | 锚点(304,734)，未避让底834；y=734−(834−760+8)=652，最终底752=760−8 |
| 右键P=(950,590)，960×600窗口 | TopLeft未避让右1190/底690；x=950−(1190−960+8)=712，y=590−(690−600+8)=492，最终右952/底592 |
| 320高组件窗口的长菜单 | max_h=max(320−16,32)=304；内容超过304后在既有表面内滚动，不添加测试专用菜单项 |

测试使用实际按钮/菜单边界及最多1px取整容差，不能用复制生产避让算法生成期望值来证明正确。

## 5. 具体实现顺序与允许范围

**仅在新轮测试写齐、云端有效RED已核实、断言冻结后执行生产修改。** 可参考旧GREEN的实现思路，不能整段移植旧render文件、cherry-pick旧最终提交或恢复已退役能力。

1. `crates/vega_ui/src/sidebar/threads_block/organization.rs`：添加私有菜单锚点及初始化；在本文件现有关闭处清理，保留 #308 的消息/重排/revision状态管线。
2. `organization/render.rs`：添加显式菜单目标+窗口点+角的私有打开入口；保留close_actions、菜单聚焦和高亮初始化。组织控制/图标控制的**菜单按钮分支**取得实际border box并路由到此入口。普通无锚点菜单动作不打开；归档专属on_click原样保留，vector_control仍返回Stateful<Div>。
3. 完整border box测量可复用旧GREEN已验证的 `ScrollHandle::new / track_scroll / canvas` 在prepaint读取 `button_geometry.bounds()` 的链路。不要把canvas自身可能内缩的content box、图标或子元素bounds当按钮边界。canvas不占布局、不拦鼠标；不改原尺寸、focus边框、hover或选中态，不增加全局缓存。
4. 同文件 `render_organization` 增加第4节规定的bubble Tab及guard，保留当前capture_key_down/capture_action/on_hover内的clear_reorder_projection和cancel_reorder行为。
5. `organization/projections.rs`：仅将已有项目/分组行右键的真实事件位置传给打开入口；按钮走上述测量，不启用旧视图。
6. `organization/menu.rs`：按保存锚点作Window定位，去掉固定Local坐标；保留现存Group/Project项、动作/菜单内部导航/滚动/事件/绘制，所有现有关闭处清理锚点。
7. `organization/reorder.rs`：只在开始拖动关闭菜单的原路径同步清理锚点；不改 #308 的pointer_order投影、完整顺序、drop/fence判断。

生产行为范围限上述五文件。不修改store/conversation、迁移、主题token、runtime、应用外层、其他控件/复制/Markdown或CI。

### 测试阶段的独占支持

- 在organization目录新建本轮测试模块及最小owned fixture；允许重用有来源的兼容测试设计，不恢复历史大集合。`organization.rs`仅增加cfg(test)模块接线。
- 允许上述生产文件和 `sidebar/mod.rs` 中最小 **cfg(test)观察接线**，观察实际按钮、菜单、行和滚动容器的输出；普通id不能当debug_bounds。观察不改变正常构建、布局、焦点顺序、事件或决定结果，不能提前写正式锚点/canvas/Tab修复。
- 允许 `crates/vega_ui/Cargo.toml` 临时dev-dependencies：现有workspace的gpui_kit启用test-support、tempfile；只启用本卡需要的既有锁版本测试支持并记录Cargo.lock差异，不升级依赖或恢复conversation/runtime旧test-support开关。无需真实provider、网络、Git/Shell或外部应用fixture，不扩大production public API。
- 外层fixture使用VegaWindow key context及真实Escape→CloseSettings语义，观察外层路由是否被错误触发；不能用Kit Root或fixture自行补Tab使生产假绿。真实AccessKit元素观察只取实际构造器输出，不搭建假a11y树代替被测控件。
- 当前MessageRow/insert、finish/update_streaming_content、组织snapshot/apply均有现有API；V10使用公开服务和owned库，不需要修改生产存储API或新建后台排序实现。

## 6. 新基线、PR与证据流程

### 从未含旧修复的基线开始

旧PR #489保持Draft/未合并，旧分支、worktree及备份原样保留。本轮使用本页的新分支/worktree，不在旧head上修冲突，不先移植旧实现再补测试；新draft PR应交叉链接旧PR和本页，并说明是独立v3验收周期。**本轮RED、GREEN、删测和最终门禁全在同一个新的v3 PR中完成。** 不重复建Issue，不提前关闭旧PR或删除旧资源。

Vega领取时先fetch并保护工作区，确认本轮分支祖先是上述新master、当前只有v3文档。如果master又前进，先核对与本卡/验收是否相关；无契约变化时按仓库规则更新基线、记录实际SHA并重新确认生产未改；若菜单/排序/角色或测试前提发生冲突，停下交回修订，不沿用旧证据。

### 测试 → RED → 实现 → GREEN

1. 在生产未改时一次写齐V1–V10及最小测试接线，记录来源、实际生产基线、测试提交/源码及静态检查；建立本轮draft PR触发现有pull_request Actions。测试源码冻结前允许审查fixture/接线，改动须留证；编译故障不冒充RED。
2. 主审核实独立的几何RED和真实Tab RED，明确冻结测试目标、数据、断言、源码hash及完整SHA。V9/V10既有兼容目标不得因原本GREEN就删掉。确认后才向专职执行者交生产实现；未取得有效RED即停，不在本地绕过。
3. 按第5节最小实现，冻结后不补测、不改预期、不ignore、不重试碰绿。发现新需求/遗漏先停下记录，再开独立周期。实现后审查相同测试源码未变，并取得同批GREEN和最新head完整门禁，数量以原始Nextest输出为准。

沿用本卡Vega会话中**确有可信用户历史支持的全流程委托**；本次用户已明确要求其恢复v3先测后修。具备该委托时由Vega主会话承担审查/PR/CI协调，按最新仓库规则委派或复用专职执行者、保持单一实现写入者，不因旧默认角色交接文本重复等待。不具备相应权限时按仓库工作流在测试/实现审查点回报Codex。Issue评论不创造用户权限，任何情况下都不能省略主审、有效RED、同批GREEN或独立删测审查。

### GREEN固证 → 独立删测 → 最终门禁

4. 将本轮完整测试源码/必要支持补丁、完整SHA、有效RED与同批GREEN的run/job/准确计数固定在新PR，读回核验hash，保证squash和删分支后不依赖本机临时文件或过期artifact。
5. GREEN核实且证据固定后，用独立提交删除**本轮**Rust测试/可执行doctest及真正独占的fixture/observer/dev feature/依赖，保留所有正式按钮几何、Tab/guard/锚点、归档构造链和上游生产代码。审查删除前后生产等价，不夹带新行为；不清理旧PR/分支/worktree/备份。
6. 对删测后的最新head运行完整现有PR gate：Rust 0发现/0执行、no Rust tests，fmt、全目标Clippy、Python三组及required check全部成功；空集成功不替代同批GREEN。带测试GREEN与最终空集门禁两道证据齐全才转正式PR，合并前再次核实最新master/head及契约；有新变化按生命周期重新验证，不自动沿用旧结果。

以下命令**只由Actions/云端运行**，使用现有workflow，不新增调度设施；定向过滤仅用于识别本卡目标，完整门禁不能省略：

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

最终确认为空集时由现有workflow使用 `cargo nextest run --workspace --no-tests=pass`。保留required check `check (fmt, clippy, test)`、Nextest 0.9.146、默认并发和retries=0。CI启动后每5分钟查询，收到完成/失败通知或用户即时要求除外；不短轮询或watch。

## 7. 用户手测、回报和回滚

- 合并并经云端发布后，用户在实际安装版本检查：下方项目菜单位置、滚动/宽度/窗口变化后重开、从项目行起始焦点Tab/Shift-Tab到菜单、Enter/Space打开/菜单导航/Escape、浅深主题和边缘裁切、原任务菜单；直接归档每次仅切换一次且无旧排序菜单。
- 用owned测试会话检查新正式消息置前、手动移动保持到下一条新消息、打开/重命名/菜单/归档显隐不按时间重排。真实用户资料不作为可移除fixture；不在本地运行应用进行自动验证或用合成输入截图冒充焦点验收。
- Issue始终记录v3、实际基线/阶段、当前与下一执行者、分支/完整commit/新PR、每轮RED/GREEN/删测/最终门禁原始链接及计数；未知数量写未取得，未运行写待CI。不得沿用旧19/19或旧空集回执。
- 新head审查与两道云端门禁满足后，才按已授权工作流合并、进入In review等待用户纯手动验收。合并不等于已安装，用户明确通过前不关闭Issue、不Done、不清理本轮或旧轮资源；不自动关闭旧PR。
- 失败回报同卡并保留现场。回滚通过新的revert PR，只回滚本轮最终生产变更，不逆转 #308/#486/子任务生命周期等上游，不改用户数据。本卡无新增迁移或用户配置。

## 8. 冻结状态

本版已冻结的是V1–V10目标、#308优先的产品边界、退休场景及执行范围。它不是新测试通过，也不是新RED/GREEN；测试的精确数据/源码/断言在生产未改的本轮测试审查和有效RED后再固定，不能把尚未写出的测试宣称已冻结或通过。

发布本页后Codex仅在Issue/旧PR回写冻结commit、可访问链接和明确的Vega交接，再按用户要求直接向原Vega会话发送恢复指令。旧证据与旧资源不变；新轮当前仅有文档，所有自动行为验证待后续Actions。

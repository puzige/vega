# #309 v3：先行测试阶段

2026-10-19。依据[冻结 v3](vega-issue-309-sidebar-menu-anchor.md) 的 V1–V10，在生产实现未改时编写本轮测试。用户在本轮会话明确授权 Vega 协调独立审查、PR、Actions 与门禁后合并，并明确不使用 CUA；专职子 agent 是唯一代码写入者，主会话只负责审查、文档及集成。

## 基线与阶段

- 实际生产基线：`39b706e2e646c53fcfb25d010b3eecd19b323eb3`。相对原冻结基线仅增加 #493 的链接预览重借用修复及文档，不影响本卡侧栏/存储/服务契约。
- 文档分支 HEAD：`91a205bbb973c865951018919fbff068e84eb564`，由原方案提交 `63d10bea9251c2766af208d8b6a8e9a4d7ccc0b5` 同步最新 master 后得到；方案原文及旧提交保留可访问。
- 本阶段只有测试、测试观察接线和临时 dev 支持；没有正式锚点、按钮测量 canvas、Tab 遍历或存储行为修改。
- 本轮静态清点为 **21 个 GPUI 测试定义（9 + 9 + 3）**，不是发现/执行/通过计数。
- 新 draft PR 的云端有效 RED、同批 GREEN、删测与最终门禁尚未取得；未取得前不得实施生产修复。
- 旧 PR #489、旧分支/worktree/备份保持原样。旧 RED/19 GREEN/空集不作为本轮证据。

## 源码内容标识

目录：`crates/vega_ui/src/sidebar/threads_block/organization/`。

| 文件 | 定义数 | SHA-256 |
|---|---:|---|
| `menu_placement_tests.rs` | 9 | `dc429efa7c629fe95d1840ca946ca2d152a5a12e3e36834f2390b80f708bb683` |
| `menu_placement_extended_tests.rs` | 9 | `e5d0aae02e2a30cc43b8b77164a4df4629e50132eab2a3b44d4a9b5f4942cb15` |
| `menu_placement_compatibility_tests.rs` | 3 | `3f607b550a46ca43a28ea94a33aaccb1090dfac61c4918e40cffb8076e82ae71` |

测试提交及原始补丁由新 PR 固定，不预填尚不存在的 SHA。测试阶段送云端不等于已经取得有效 RED；只有主审核实新几何与真实 Tab 行为失败后才能冻结并继续实施。测试自身接线错误须先留证审查，不改变验收含义。

## V1–V10 对照

下列测试名称均省略共同前缀 `organization_menu_placement_`。

| 验收 | 本轮测试后缀与边界 |
|---|---|
| V1 | `project_buttons_follow_separated_rows_without_navigation`：真实 Sidebar 的上下项目按钮，实际边界及路由/草稿/持久数据不变。 |
| V2 | `tab_and_shift_tab_reach_project_menu_from_project_row`、`direct_archive_tab_from_neighbor_does_not_activate`：真实 Tab/Shift-Tab，从另一个已绘制停靠点观察焦点，不由 fixture 补遍历。归档遍历共享根处理缺失，不能把它误当 V9 激活回归必须基线 GREEN。 |
| V3 | `keyboard_project_anchor_ignores_pointer` 独立建立焦点取得几何证据；`project_commands_wrap_trap_focus_and_move_project` 按当前菜单项数验证箭头/Tab 环绕、焦点保持、Enter 下移及 B/A；`escape_consumes_window_action_without_closing_outer_route` 使用 VegaWindow 的 Escape→CloseSettings。 |
| V4 | `project_projection_uses_current_button_and_pointer`、`group_projection_uses_current_button_and_pointer`：现存旧投影的按钮和真实右键事件，不重新启用产品旧页面。 |
| V5 | `reopen_uses_scrolled_resized_current_layout`、`minimum_window_surface_escapes_sidebar_clip`、`long_group_menu_reaches_and_executes_last_item`：滚动/宽度/窗口变化、浅深主题、960×600、240/365 侧栏、320 高组件长菜单。 |
| V6 | `switch_close_remove_preserves_other_project_and_draft`、`drag_and_group_edit_close_then_reopen_current_target`：目标替换/关闭/重开；真实移除解绑但保留目录、历史、草稿、preferences/memberships；拖动和编辑关闭路径。 |
| V7 | `modified_tab_and_outside_focus_do_not_enter_root_traversal`、`inline_rename_keeps_tab_focus_text_and_escape_action`、`pending_group_edit_blocks_root_traversal_without_saving`、`active_reorder_keeps_tab_focus_and_escape_cancels_without_route`、`tab_clears_pointer_projection_without_reordering`：修饰键/外部焦点/编辑/拖动 guard 及 #308 capture 清理保护。 |
| V8 | `task_menu_preserves_pointer_keyboard_commands_and_close`：原任务菜单的指针/键盘几何、代表命令、关闭、组织菜单替换。九项基础命令下界断言不宣称完整组织附加项总数。 |
| V9 | `direct_archive_real_button_click_and_key_release_toggle_once`：实际 archive_control 构造元素的 Button role、显示/隐藏名称、Click/Focus 支持及选中绘制；鼠标和 Enter/Space 完整按下/松开，两状态每次切换一次。 |
| V10 | `committed_messages_manual_order_and_ui_preserve_compatibility`：owned 文件库，完整七 ID 顺序直接声明，同区 A/B/C→C/A/B→A/B/C→B/A/C；新正式消息 insert、真实 ReorderThread、实际 UI 组合；open/visit/rename 和同消息更新不重新排序，更新影响一行且从第二连接查回已提交正文。 |

几何与真实 Tab 分属独立测试；键盘几何不等待 Tab 成功，不让入口失败遮住定位断言。V9 激活/语义和 V10 是既有契约保护，不人为制造 RED。

### V7 证据限制

当前生产 `edit_group` 创建并聚焦 editor 状态，但没有 renderer 挂载该输入。测试只从生产创建的待编辑状态验证根遍历 guard，保留未保存文本、实体身份和持久结果；没有经测试观察器补挂输入，也不声称验证了已挂载分组编辑器的输入/焦点行为。本卡不修复这一卡外界面缺口。

## 历史来源与适配

只读来源为 `9bd60bcc6d1856e06b17d1ae8909c4ae422a1821` 的两个菜单测试模块：逐项评估并选择 15 个兼容场景，不整目录恢复旧测试。四个与 #308 互斥的排序菜单场景按 v3 §3 退休，未恢复 Filter、ToggleArchive、ToggleSort 或 Created 时间排序预期。新增六项覆盖当前菜单导航、拖动/编辑重开、capture 清理和三项兼容场景。

必要适配：

- 锁定 GPUI 的 simulate_keystrokes 只派发 KeyDown，共享 helper 补完整 KeyUp；Tab 派发真实事件而非调用 focus_next/prev。
- 几何场景移除重复项目移除职责；独立 V6 保留并增加目录/preferences/memberships 断言。
- #308 下插入历史会置前，V6 种入历史后刷新真实 Sidebar，再建立 UI/持久基线。
- V3 使用现存项目命令与真实项数；V10 使用有效同区 append 恢复 C，不用跨区锚点或内部 JSON 写入制造顺序。

## 独立审查及测试准备纠正

正式生产修改前，独立只读 reviewer 对完整 V1–V10、观察接线和锁定 API 进行审查：

1. 确认 fixture 不使用 Kit Root（它自身会补 Tab），不手造归档按钮或辅助功能树，不由 observer 安装锚点/测量/遍历。
2. V3 项目菜单导航和 V6 目录/元数据保留曾在中间稿缺失；当前稿已在测试冻结前补齐既定验收。
3. 中间稿将 idle hover 误作 pointer_order 创建条件，并有一处 rustfmt 换行失败。该稿未提交云端、没有有效 RED。生产只在鼠标按下或 pending hover 创建投影；当前用例改为真实 task action 焦点+鼠标按下，先断言投影存在，再派发 Tab/Shift-Tab 观察 capture 清理，移除无效 idle-hover 假设，不变更生产行为或增加验收目标。
4. 最终窄范围复审确认上述夹具缺陷已解除。`Fixture::assert_highlight` 无调用但处于已有 allow(dead_code) 范围，非门禁阻碍；不为覆盖率添加无效调用。

以上仅静态审查，不是类型检查或行为通过证据。

## 最小测试支持

- `organization.rs` 的 cfg(test) 模块接线。
- `sidebar/mod.rs`、organization 的 render/menu/projections/reorder 中真实控件/菜单/行/滚动区 debug selector；正常构建行为不变，不加焦点停靠点或事件处理。
- UI dev dependencies：现有 workspace gpui_kit 的 test-support 和 tempfile。
- Cargo.lock 恢复已知测试支持包/语义边及版本消歧标签。静态比较：没有删除或升级任何既存包版本/source/checksum，新增十个测试支持包；不恢复 conversation/runtime test-support，不改变 workflow。
- 依赖补丁来源 `39f13272e818d8b5ed951e6a82a1db8ac6ad59bb`，仅作为版本来源，不作为本轮测试证据。

## 本地静态检查

最终测试交付后由主会话读回：

| 检查 | 结果 |
|---|---|
| 使用已安装 Rust 1.98.0 的 `cargo fmt --all -- --check` | exit 0 |
| `git diff --check` | exit 0 |
| 测试源码定义数/hash、无新增注释/ignore/退休 sort 场景 | 静态核对 21 定义，结果如上 |
| Cargo.lock 版本/source/checksum 与受保护生产范围 | 静态核对无升级，store/conversation/runtime/Markdown/CI 无差异 |

没有本地运行 Vega、编译、构建、Clippy、Rust/Python 测试、benchmark 或打包；全程不使用 CUA。

## 下一门禁

创建本轮新 draft PR 并交叉链接 #489，以现有 pull_request Actions 跑完整门禁，分别读取几何及真实 Tab 原始失败，准确记录发现/执行/通过/失败/跳过。无关编译/fixture/依赖/runner 故障不算 RED。有效 RED 未核实前生产实现停止。

同批 GREEN 后将完整测试/支持补丁、完整 SHA 和 RED/GREEN 原始链接固定在同一 PR，再用独立提交删本轮 Rust 测试及独占支持，最终空集 head 完整门禁通过后才转正式 PR。用户手测前不关闭 Issue、不 Done、不清理本轮或旧轮资源。

## 新轮云端 RED 与冻结

测试提交为 `f02679b9afbc478ec8b60353d21a6100b6c7370f`，新 [Draft PR #497](https://github.com/puzige/vega/pull/497)。[Actions 39242636898](https://github.com/puzige/vega/actions/runs/39242636898) 已完成：[Nextest 118026891799](https://github.com/puzige/vega/actions/runs/39242636898/job/118026891799) **21 发现/21 执行、10 通过、11 失败、0 跳过、0 ignore**，跨 14 binaries；Python 5+15+14 通过，[fmt/全目标 Clippy 118026891446](https://github.com/puzige/vega/actions/runs/39242636898/job/118026891446) 成功，required check 因目标行为缺失而失败。编译/发现均成功，不是接口缺失或环境 RED。

[原始 11 失败与 10 通过清单](https://github.com/puzige/vega/pull/497#issuecomment-6265957563)已固定并读回，Nextest 原始 job 日志 SHA-256 `7872d0a64f1228f87020cdb0c86e631d52b3608534b67f6788ea52bf526588e5`。主审与独立 reviewer 已核实：

- 八项失败在实际菜单/按钮几何断言。例如 V1 菜单右边 280，而实际按钮右边 260；键盘几何独立开菜单后也失败。旧分组按钮实测 63×32，为真实完整按钮，不强改成 vector token。
- 两项失败在真实 Tab 后焦点未移动：项目行 30 未到相邻 31；归档相邻 37 未到 36。不是直接 focus 或 fixture Root 补出的遍历。
- V5 底部场景在菜单未超出侧栏滚动区的定位结果失败，符合固定位置根因；这不是独立证明遮罩缺陷，后续实际外区点击尚待 GREEN 执行。
- 原任务菜单场景已走完原菜单代表断言，只在最后项目菜单几何失败；V6 后续移除断言尚未执行，不能称全场景通过。
- V9 真实归档激活/语义、V10 提交消息/手动顺序保护，以及菜单导航/guard/capture 清理均基线通过。

[完整 core](https://github.com/puzige/vega/pull/497#issuecomment-6265842868)、[extended](https://github.com/puzige/vega/pull/497#issuecomment-6265843653)、[compatibility](https://github.com/puzige/vega/pull/497#issuecomment-6265844664) 与[测试支持补丁](https://github.com/puzige/vega/pull/497#issuecomment-6265845488)已经逐条固定并精确读回。支持补丁 SHA-256 `923e9b297ba91e5140f41382e1e75c316daa722d61024a46c6b22cb90f4d68ff`。

[冻结与实施授权](https://github.com/puzige/vega/pull/497#issuecomment-6266003510)确认保留本报告三份源码 hash、目标/输入/断言不变，仅向原专职执行者交 v3 五文件最小实现；同批 GREEN、独立删测和最新 head 最终门禁仍待取得。

## 有效 RED 后的最小实现与静态审查

专职执行者交付 157 新增/27 删除，生产仅触及五文件：

- `organization.rs` 保存/初始化私有菜单锚点，分组编辑时与菜单一并清理。
- `render.rs` 显式菜单目标/点/角打开入口；没有按钮几何的普通 Menu 不打开。菜单按钮用 ScrollHandle parent bounds + canvas prepaint 取完整 border box，鼠标/键盘使用右下角 TopRight；非菜单按钮保持原动作。组织根新增仅普通 Tab/Shift-Tab 的 bubble 遍历及 guard，capture 重排清理原样保留。
- `projections.rs` 只将原项目/分组行右键的当前 MouseUpEvent.position 传给 TopLeft 打开入口。
- `menu.rs` 用保存的 Window 锚点取代固定 Local 坐标；原菜单项目/尺寸/滚动/occlude/内部阻断、margin 8 和只 deferred 表面 priority 3 不变。所有原关闭处清理锚点。
- `reorder.rs` 只在原拖动开始关闭菜单处同步清理锚点。

主审和独立 reviewer 静态核实 GPUI 在 child canvas prepaint 之前已更新父控件 ScrollHandle.bounds，测量的是完整按钮而不是内缩 canvas。归档 Click 分支、archive_control 构造器与原 capture/hover/mouse 重排处理经字节比较不变；Stateful<Div> 返回及 Role::Button/on_click 不变。三份冻结测试 hash、Cargo/观察支持、store/conversation/runtime/应用外层/CI 均未改，未追加测试。fmt/diff 静态检查 exit 0，无新增注释/unwrap/expect。独立审查无阻挡，批准提交云端同批 GREEN，不宣称类型/行为已通过。

本地仍未运行 Vega、编译、Clippy 或任何测试；提交此实现时云端结果待取得，GREEN 前未删测试。

## 同批完整 GREEN 与证据固定

带测试实现提交 `daf2e20d5258ff694d6cec164ca2c0631bb7b364` 的 [Actions 39245493845](https://github.com/puzige/vega/actions/runs/39245493845) 已成功。

| 云端 gate | 结果与原始来源 |
|---|---|
| Nextest | [118036287464](https://github.com/puzige/vega/actions/runs/39245493845/job/118036287464)：21 发现/21 执行/21 通过/0 失败/0 跳过/0 ignore，14 binaries，20.104s |
| fmt / 全目标 Clippy | [118036286924](https://github.com/puzige/vega/actions/runs/39245493845/job/118036286924)：成功 |
| Python 发布/组件准备/delta | 5+15+14 项成功，Nextest job 原始输出可核对 |
| required check | [118037812109](https://github.com/puzige/vega/actions/runs/39245493845/job/118037812109)：成功 |

[21 PASS 完整有界原始输出与固定索引](https://github.com/puzige/vega/pull/497#issuecomment-6266341150)已发布并精确读回，Nextest job 原始日志 SHA-256 `d87b0e8563aa106356fbaed6f5332d4f12d3ff68c69ac6c332339ccfd1856c75`。本报告三份测试 hash 与 RED 完全相同，测试/观察/Cargo 支持没有随实现改写；没有新增、删除、ignore、放宽或重试用例。

全部 V1–V10 现已执行到底，包括 RED 未到达的项目移除/目录历史保留与滚动区外菜单命中/命令。该证据属于进程内 GPUI/owned 文件库，不代替用户在发布版本的真实手测，也不表示应用已经安装。

只有在上述 GREEN 与原始测试/支持/RED/GREEN 固定后，才向专职执行者交接独立删测。删除范围仅本轮三模块 21 测试及 cfg(test) 观察/接线、UI 临时 dev dependency 和对应独占 lock 边；所有正式锚点、按钮测量、Window 定位、Tab/guard、archive Click/stateful/capture/顺序行为保留。最终空集完整门禁与独立删测审查仍待取得；PR 保持 Draft，Issue OPEN / In progress。

## 上游前进后的同批复验

清理执行者按交接条件先 fetch，发现 master 已到 `9cd84638912547914d23764b1c5de30e4997c117`，因此立即停止，未删除测试或支持。主审核查新上游为 #496 的任务行 `flex_shrink_0()`（组合几何相关）与 #495 的 Chat 兼容/思考设置；本卡验收、五文件、测试/支持/Cargo/CI 不变，无产品契约冲突。

使用普通 merge 同步并保留原 RED/GREEN 历史，复验 head `18624060ae7c6ac4cb847bd551d6be97471fe08b`。三份测试 hash 与 #309 生产实现和原 GREEN 完全相同，上游改动完整保留；fmt/diff 静态检查 exit 0。[PR 同步记录](https://github.com/puzige/vega/pull/497#issuecomment-6266400539)固定停止清理原因与后续时序。由于实际组合布局前进，要求同一冻结测试和完整云端 gate 重新 GREEN，不能沿用旧 GREEN 删测或合并。

复验 [Actions 39246684926](https://github.com/puzige/vega/actions/runs/39246684926) 已成功：[Nextest 118040208052](https://github.com/puzige/vega/actions/runs/39246684926/job/118040208052) 21 发现/21 执行/21 通过/0 失败/0 跳过/0 ignore、14 binaries、31.281s；[fmt/Clippy 118040208399](https://github.com/puzige/vega/actions/runs/39246684926/job/118040208399)、Python 5+15+14、[required check 118041893528](https://github.com/puzige/vega/actions/runs/39246684926/job/118041893528) 全部成功。

[复验 21 PASS 原始输出与恢复清理授权](https://github.com/puzige/vega/pull/497#issuecomment-6266507653)已固定并读回，整轮原始日志 SHA-256 `d00e2ebdfbe40aff6a66484fefe5b0cfe3ca94d2e47c59c172506c1d88720541`。该结果是最新组合基线的同批 GREEN，不是重复碰绿或改变断言；现在才恢复本轮独立删测。

## 独立删测与生产等价审查

专职执行者在 `18624060` 的同批 GREEN 固定后，独立删除三模块 21 测试/2,528 行及其独占 fixture/helper、三模块接线、九处观察接线、UI dev test-support/tempfile 和对应 Cargo.lock 支持。正式文件的控件 ID、无障碍、焦点、事件、锚点/测量 canvas/Window 定位/guard/归档 Click/重排 capture 均保留。

原支持反向补丁只有 render.rs 因正式实现插入而上下文不匹配，dry-run 未改文件；其余七路径精确反向移除，render.rs 仅删两处 cfg(test) observer，不整段覆盖。清理源补丁（不含主会话文档）SHA-256 `32f93c5d18bb403f3ea335a7df7d9390d60246692fdca7aa274f60fc71ab261d`。

主审与独立 reviewer 均确认删测前后生产等价：剥离原测试接线/临时表达式包装后的六个生产源码逐字节相同；相对当前 master 的非文档差异仅 v3 允许五文件。Cargo.lock、UI Cargo.toml、Sidebar 外层及含 #496 行高修复的 threads_block.rs 与最新 master 字节一致。保留所有 #495/#496 上游、Python/fixture、CI/Nextest、store/runtime/应用外层，未夹带新生产行为。

依赖清理为 924→914 包，既存包 version/source/checksum 不变，无新增依赖边；只移除十个测试独占包及 retained 包的 gpui→backtrace/proptest、UI→tempfile 边。最后本地 fmt/diff、静态残留扫描通过，workspace Rust test attributes 静态匹配为 0；真实 0发现/0执行仍须最终云端 gate 确认，不称本地测试通过。

清理后五文件 SHA-256：

| 文件 | SHA-256 |
|---|---|
| organization.rs | `b9ed2e15854203b2b84510525a9d06e56465238d156cd93ed7dbfae29cdc3d90` |
| render.rs | `1e0d2da26ae1ea421caa01d9747d6c615714f8ea2ba343514c4d4ac45a1da6c6` |
| projections.rs | `28a97429f4d4578627834de096a4c0ef93b702b766e43168923680811a98d668` |
| menu.rs | `a97d95bcde5312386728a371a7a63063aa23c62cf0e513a25aac0edf2b234ad7` |
| reorder.rs | `000384d2906d2ca322c7281d3ecf7f42b50d11a03d959c2e9a49fd3f28d56c8d` |

下一步对独立清理后的最新 head 跑完整现有 PR gate。两轮同批 GREEN 不替代最终空集门禁；旧 #489/分支/worktree/备份继续保留。用户手测前不关闭 Issue、不 Done、不清理任务资源。

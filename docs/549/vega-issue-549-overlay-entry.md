# #549 浮层入场续接规格

## 0. 当前授权、版本与已有产物

- 2026-10-25 用户确认原执行者停止，并授权本会话接续实现和验证。认领已发布并读回：[vega-549-takeover-20261025T021146Z](https://github.com/puzige/vega/issues/549#issuecomment-6392320961)。
- 初始只读基线：`420a0eb9a06594173b658f7e3a4437e3d2b5432d`；写入前再次 fetch 并快进至实施基线 `572a3c6f1b50ea34c2214245dcd18ee3a70bb737`。新增 #559 压缩行对齐与 #604 重命名外点取消已逐项读 diff，保留且不扩入本卡。分支 `feat/549-overlay-entry`；工作树 `vega-549-overlay-entry`。旧分支、未推送提交和工作树原样保留，不把旧树整文件恢复到当前基线。
- [原规格](https://github.com/puzige/vega/blob/da3b2ac1910a2d54cbf45c29019b66b92941e702/docs/549/vega-issue-549-overlay-motion.md) 的 O01–O11、E01–E13 产品验收保留。本页收口候选 API、owner 边沿和验证方式，替代旧的文档阶段权限、强制 TDD/RED/测试冻结措辞；不删减产品范围。
- PR #579 已合入 `4db4992df27e9b54e62927209a7fcde420b601f6`，当前已有 `Motion::DISCLOSURE_TURN_MS`、`TAB_SELECTION_MS` 以及 `interaction::{decorative_transition, decorative_value}`。本卡只增加浮层消费者需要的 token/薄接线，不重复实现 #550、不恢复旧测试。
- #535 PR #603 已合入本基线。#544 及 #552 PR #566 不在接管范围；不同工作树不会互写，当前仅在本分支集成各入口窄接线，保留主线业务变化，后续新基线须重新核对差异。
- 代码必须由专职实现子 Agent 单一写入；主 Agent 管规格、Git/PR、验证与集成；完成后由非写入者审查精确版本。遵守根 AGENTS、执行指南、设计守则与当前快速交付政策。本地不运行 Vega/编译/构建/打包/测试/profile/CUA，不安装或重启应用；不新增代码注释。

## 1. 场景、验收与风险

下表是产品要求，不表示已运行或通过。自动证据只由现有 Actions 产生；真实系统体验保留用户手测。

| ID | 操作/前提 | 可观察结果 |
|---|---|---|
| O01 | 每个纳入入口打开，在入场期间输入、导航、选择 | 整体短淡入；首帧即能操作，无额外等待，无逐项错峰 |
| O02 | 入场中/完成后 Esc、外点、再次触发 | 原关闭语义立即生效，无 down 关闭/up 再开、点击穿透或动画后才关闭 |
| O03 | 关闭请求中的层后切会话/页面，旧结果到达 | 原取消、代际围栏、prior-focus 所有权保留；旧结果不复活浮层、不抢新页面焦点 |
| O04 | 中间帧关闭并快速重开、同帧 close/open、重复刷新、交替对象 | 从该 owner/object 最后真实绘制值接续，不排队、不借用其他对象进度；无关刷新不重播 |
| O05 | 关闭后 Tab/Enter/点击旧位置 | 无交互 child、ghost focus/hit/a11y 节点或残留遮罩；下一独立手势正常 |
| O06 | Reduce Motion、隐藏、失活、卸载、恢复 | 直达终态，停止装饰调度；恢复不补播旧动画；业务 stream/terminal 不暂停 |
| O07 | 浅深主题、窄宽窗、边界位置、流式重绘 | 原几何、锚点、命中、焦点环、deferred 优先级不变；背景/边框/文字/图标一起淡入 |
| O08 | 索引失败、错误/警告/权限/取消提示出现 | 必要内容立即全不透明且可操作；安全层不走装饰 wrapper |
| O09 | 链接 hover、leave/re-enter、查询中开关 | 原 400ms arm、150ms leave grace 与 worker/poll 时序保留，不叠加动画等待 |
| O10 | 长会话、长菜单、真实生产流式路径 | 保留输入/关闭/取消，不新增正文逐帧重测或跟随；无测量不声称性能改善 |
| O11 | 宽窗 Environment、Dock、安全提示、未纳入帮助层 | 原行为不变，无尺寸动画、无业务数据/权限策略变化 |

数值核对：统一入场 120ms 位于 Issue 建议区间 100–140ms。快重开 `elapsed <= 120ms`（包括恰好 120ms），超过此值重新从 0 入场；只复用同一 token，不引入关闭计时任务。测试中 60ms 位于 0–120ms 之间，曲线沿用 Kit 默认 ease_out_cubic，不把半时长断言成 alpha=0.5。

## 2. 完整入口与最小接线

每项只处理现有真实表面，不添加第二个遮罩/焦点 owner，不移动原 deferred 的空间坐标。表面在 deferred 内使用透明度，而非仅给 deferred 外祖先设置透明度。

| 入口 | 当前文件/符号 | owner / 接线及保留边界 |
|---|---|---|
| E01 | `vega_ui/src/conversation_stream/render.rs::render_file_dropdown`；`file_reference.rs` | ConversationStream 的文件候选层；loading/results 不当新打开，索引失败旁路；原 priority 2 |
| E02 | 同 `render_model_picker_layers/render_picker_slider_layer/render_picker_list_layer`；`core.rs` 开关 | Slider/List 各有真实对象身份；切层不串进度；原 priority 2、模型 guard、Thinking 控件保持 |
| E03 | `conversation_stream/utility_bar.rs::render_backend_menu/render_utility_projects_menu` | Backend、项目菜单各自 owner 状态；保留 draft guard、项目选择、capture 防重开、priority 2 |
| E04 | `branch_selector.rs::Render::render` 和真实 open/close 路径 | BranchSelector；保留搜索焦点、100ms Git 轮询、失败立即可见、priority 2 |
| E05 | `sidebar/threads_block.rs::render_row_actions/render_action_menu` 和 actions 开关 | thread/object 区分；保留 anchored/local/snap、priority 2、归档/删除/重命名语义 |
| E06 | `sidebar/threads_block/organization/menu.rs::render_organization_menu` 及 organization 状态 | menu owner/分组对象区分；原 priority 3、原存储/动作/键盘规则；危险确认不淡入 |
| E07 | `conversation_stream/pending_messages.rs::render_pending_menu/close_pending_menu` | pending message ID 区分；保留原 menu_focus、priority 3、队列及插话业务 |
| E08 | `conversation_stream/selection_menu.rs::SelectionMenu::render` | MenuState 持元信息，PopupMenu 仍由原 handler 生成/移除；不二次包遮罩；原 POPUP_PRIORITY/选区/有条件回焦 |
| E09 | `settings/provider_management.rs::render_provider_management` 及 provider/model 菜单开关 | ProviderManagement 对象区分；priority 2；保存冲突、测试失败、权限/删除确认不淡入 |
| E10 | `image_preview.rs::Render::render/set_applications/dismiss_menu` | ImagePreview 的打开方式菜单；只菜单淡入，原 priority 2；真实错误/保存/外部打开回执不改 |
| E11 | `command_palette.rs::Render::render`；`vega/src/app_palette.rs`；必要 `window/render.rs` owner 接线 | 持久 AppPalette 保留元信息，短命 CommandPalette 只拿表面 handle；仅内卡淡入，外 backdrop/取消/回焦即时；search generation 不充当动效身份 |
| E12 | `vega/src/window/workspace.rs::render_environment/render_workspace` 及 Environment 开关 | VegaWindow/Workspace 持状态；只窄窗卡片，保留 outside layer/stop_propagation/锚点；宽窗 rail/Dock/PTY 不改 |
| E13 | `link_preview.rs::Coordinator/dismiss/arm/MarkdownLinkPreviewLayer::prepaint/build_card` | Coordinator 按现有 link key 保留对象元信息；保留 Positioner、bounds 测量、priority 100、400/150ms；error/rejected/warning 旁路 |

明确排除：权限 picker/卡片/确认、错误与保存冲突、Settings navigation guard、provider_test_help_tooltip/settings_help_open、普通 tooltip、系统文件对话框、全页/整窗/宽窗 Environment/Dock 尺寸、message anchors/#344、正文/复制/Final/模型 runtime。不得用本卡顺带合入其他未合并 PR。

## 3. 实际 API 与实现契约

### 已静态核实

- 锁定 `gpui-kit=0.6.1` / `gpui-pre=0.3.4`，不改版本/registry/vendor/通用 CI。Kit `transition_with_status` 首次采用目标；显式零时长 seed 后正常 retarget 可通过顺序同 key 采样实现，源码支持 next-frame state 优先取回；云端尚待实证。
- `Styled::opacity` 及真实 Div paint 覆盖 style/子内容。`Window::with_element_opacity` 是私有，不调用。自定义表面 adapter 应像既有 `settings/motion.rs::MotionElement` 一样代理真实 Stateful Div 的 layout/prepaint/paint/a11y，不增加尺寸盒子或另一个事件 owner。
- keyed state 只保留连续访问帧，不能把“相同字符串 key”误当已卸载子树的永久状态。使用 owner 持有、可克隆到真实表面的窄 `OverlayEntry` 元信息；不是全局 registry/Presence/动画引擎。
- deferred 不保存祖先 opacity，故 alpha 必须作用于 deferred 里面原来的实际表面。

### owner 边沿与绘制

1. 新增共享 `Motion::OVERLAY_ENTER_MS=120`。沿用 Kit 曲线/时钟/插值/帧调度，不复制引擎；不预建其他未使用 token。
2. `OverlayEntry` 仅保存必要生命周期元信息：显式 open/close、owner/object 身份或 revision、最后真实绘制 alpha、close 时间及待 seed。事件 handler 可记录边沿/读 executor clock，但不在事件阶段调用 keyed sampler。
3. open/close 要在原 owner 接受事件时记录，包含**无中间 draw 的同帧 close/open**；反复 close 不刷新原 close 时间。可在 render 做幂等同步作为保险，不能只依赖最终 open bool 重建边沿。
4. 立即关闭仍由原业务 handler 负责，直接移除 content、取消/围栏请求、按原规则回焦。元信息不持 child、FocusHandle、a11y/命中节点，也不排任何关闭任务。关闭不画退出动画、不再请求装饰帧；已经排定的一次回调可自然排空。
5. 同 owner/object 的快重开从最后**实际 paint** alpha seed 后向 1 retarget；过期、无真实 paint 或 owner/object 失效则 seed 0。新对象不得借用别人的值。持续打开/异步结果/无关刷新不 seed。
6. 用持久 owner 绘制路径维护采样命名空间，或在重新挂载时由 owner 的保留值显式重建；不得假定卸载后 Kit state 尚存。最后 alpha 仅在实际表面 paint 委托时提交，而非仅 render/prepaint 时写入。
7. 共享 helper 增加窄 seed/retarget 接线时，**一次操作只消费一次 activation interruption**。旧 decorative_value 的接口/既有消费者不退化；不能 seed 调用先吞掉中断，下一次 target 又继续动画。失活、不可见、Reduce Motion 或 safety bypass 必须优先于 seed，保留打开层在恢复时为 1，不把恢复当新 open。
8. owner/线程/页面/对象失效时丢弃其连续性；page hidden 与 offscreen 按实际 owner/viewport 门控，原订阅随 owner 释放，禁止每帧新增 observer。业务 stream/terminal/timer 不受暂停。
9. E11 元信息放 AppPalette，不放每次重建的 CommandPalette；原 `open_palette` 对已开层 refocus，不改成 toggle。E13 元信息放 Coordinator，按现有 key 隔离，随 reading scope 消失清理；ActivePreview 的 Drop 取消保留。
10. 原安全/错误分支在首次及入场中变更时都 snap 整层到 1。透明度不控制业务可用性；焦点、a11y、命中、输入与选择从首帧开始按真实 open 状态生效。

## 4. 当前验证计划与真实限制

按当前风险规则实施，不强制 RED 或巨型测试矩阵。需要开发 Rust 测试时只使用现有 Kit test-support 临时 dev 接线（`vega_ui`，必要时 `vega`），不引入新依赖版本或修改 workflow。既有 pr-check 无 features 参数，故 test-support 必须由临时 dev manifest 接线启用。父统一固定提交、draft PR 和 Actions；实现者不私自触发 CI。

高风险验证采用真实 host/adapter，至少覆盖：

- seed→中间值→120ms 终态、整层 alpha 传入真实 child paint、幂等刷新；实用生产入口接线与边界/错误旁路，而非 toy sampler 代替全部入口。
- 最后真实 paint 快重开、恰好/超过120ms、同帧 close/open、两对象隔离及 owner 重建；E11 owner 与短命 view 的边界、E13 原延时/reading scope。
- 即时关闭和原焦点/输入/取消围栏，trigger down/up 不重开，关闭后下一独立操作可用；Reduce Motion/hidden/inactive 与停帧，deferred 几何/层级不变。

可行公开观察路线：TestAppContext 创建真实窗口、executor advance_clock、完整 Window frame draw/clear；读取 `painted_quads`、debug bounds/scale factor。必要临时 child paint probe 位于真实委托边界，读实际继承 opacity/quad，不只记 sampler 值。quads 不直接暴露 SVG/glyph raster；静态调用链与真实 paint 委托不冒充每个像素实测。窗口默认 active=false，阳性对照须显式 active 且 Reduce Motion=false。`VisualTestContext::draw` 不等于完成整个窗口 frame。

无失活期间 draw 的恢复要求保留；测试应记录实际发生的 frame，而非凭“未显式 draw”推断。#550 用户接受的 D05 验证缺口不自动适用本卡，若仍无法得到合法路线，须保留原断言/证据并报告，不通过删/ignore/改期待换绿。

完整 AX 树/VoiceOver、OS Reduce Motion、字形/图标像素、真实长会话/流式观感交合并后用户手测，不能把 None AX 树或无 renderer 截图当通过。没有性能采样，不宣称性能收益。

开发测试如使用：固定远端可恢复完整源码/patch、SHA、实际执行结果，保留失败证据；达到必要验证后统一清理测试及独占支持，由非写入者核对最终完整 head。最终 required CI 必须明确 `no Rust tests`（0发现/0执行），不是行为测试通过。保留 Python/fixtures/benchmark/通用 CI。最新 head required 成功且独立审查无阻断才合并，Issue 保持 OPEN/In review 等用户手测，不自动关闭/Done。

## 5. 当前周期集成与修复记录

- 第一批生产/开发测试提交 `9115ff4c034aee99e08fbc903c8b9d709b71f2ba`，Draft PR [#612](https://github.com/puzige/vega/pull/612)。仅共享接线、E01/E11；其余入口仍未完成。
- 原始 [Actions 40160001377](https://github.com/puzige/vega/actions/runs/40160001377) 为 failure；Clippy `121069369407` 与 Nextest `121069369177` 均在测试编译阶段报告 `recursion limit reached while expanding #[test]`，位置 `interaction/overlay/tests.rs:120`。Kit facade glob 导入把 test 属性宏带入作用域，GPUI 宏生成的裸 `#[test]` 被再次解析为自身；这是测试接线问题，不是有效行为结果，禁止加 recursion_limit 掩盖。两 job 原始日志 SHA-256 分别为 `cb200a56aadeea0d83e8df8148cf2666ceb0ef7348b8ec36733934671a4b7b8d` / `4d708332184b0f53085e6a72f30a4d5e8f3e08db1be9d2c31dabb7adda95a287`。Rust 发现/执行计数未取得，不把九个定义当执行。
- 独立审查在同一提交发现两处 P2 产品接线遗漏：E01 已加载菜单所在 retained stream 被 Settings/路由隐藏时未 suspend，恢复可能重新入场；E11 异步任务/文件预览成功直接赋 close_requested，未在接受时记录 close 时间，延迟 render 后可错误延长120ms连续性。原 O03/O04/O06 不变：修复实际 owner 隐藏边沿、统一所有接受的 close intent；新增 loaded menu 卸载恢复及延迟 close intent 回归，保留现有断言。允许在 `vega/src/window/render.rs`、必要 navigation 接线和 `ConversationStream` 中添加窄公开装饰生命周期方法，不改变业务关闭/请求/焦点规则。
- 写入前再次 fetch 并将最新 master `429c6a819fc5a30f8280afd2f6ed015d4682bf41` 正常 merge 到 `a8e1ef3c7e17027bbfc64ffe8a4d072e3effd1ea`。#601 产物组与 #518 Native fork 已保留。Thread 新增 provenance 两字段要求开发 fixture 补 None；不是本卡新增持久化范围。
- E12 实际符号为 `render_workspace`，修正旧候选名，不改变产品范围。以上是本轮真实证据与下一修复路线，不宣称已修复或通过。

## 6. 交付与回滚

- 单一 writer 先交本页对应源码及必要开发验证，READY 包含精确 diff/文件身份、静态检查、未决项、停写声明；父固定内容后独立审查与 Actions 同版本进行。后续 delta 需重审受影响不变量。
- 本页初始阶段仅规格及静态调研，无新增生产变更、Rust 测试、PR/CI或原生手测。所有待验证项目保持未验证。
- 回滚仅撤销本卡表面 adapter/owner 元信息/token 与明确消费者；不撤既有 #579 共享基础、#535/#344/#544/业务/权限变化，不动用户数据或正在运行的应用。

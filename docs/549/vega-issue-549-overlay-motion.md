# #549 浮层轻量入场与统一动效生命周期

状态：SPEC/API/先行测试设计，v2 候选；主 Agent 续接审查已明确共享策略及消费者责任，浮层观测与 O04 仍未取得运行证据。本稿不是生产实现许可或测试通过声明。

- 原审计生产基线：`871f91f246e90753743a256cee1eec315e74effc`；续接树已 fetch 并快进到 `9b901a34ee00611f73fbd58275c7087282af5e2c`。新增 #554 文档和 #547 图片预算修复已保留；本卡关键浮层/动效源码未变，旧基线不是当前 HEAD。
- 分支：`feat/549-overlay-motion`；worktree：`vega-549-overlay-motion`。
- 范围仅本文件；本阶段不改生产、Cargo、workflows、测试或 `docs/README.md`，不 commit/push/PR/Issue 写入、不嵌套委派。
- 父 Agent 已 fetch/创建/领取。只读 `ls-remote origin refs/heads/master` 与本地 `origin/master` 均为上述 SHA，写前未发现新远端影响；不操作有用户改动的主工作区。
- 已读本 worktree 根 AGENTS、执行指南、Issue 工作流、设计守则、kanban skill、Rust 测试交付生命周期；三个 worktree 根规则一致，沿目标路径没有嵌套 AGENTS。
- Issue 正文与最新有效评论：[#549](https://github.com/puzige/vega/issues/549)、[2026-10-22 16:26:41Z 领取记录](https://github.com/puzige/vega/issues/549#issuecomment-6345788998)。原 Issue 的旧审计 SHA/行号不是本轮源码基线。

## 1. 场景与验收矩阵（先于方案）

下列是应冻结的目标，均未执行。A 为 Actions 生产路径验证；M 为用户真实系统/体验手测。入口编号见 §3，不能只测一个动画 demo 后声称全部入口通过。

| ID | 前置/操作 | 必须观察的结果 | 证据路线 |
|---|---|---|---|
| O01 | 对每个纳入入口首次打开；随后在入场结束前输入、方向键/Tab、选择 | 整体短淡入，不逐项错峰；首帧业务可用，无新增等待；选择执行原动作 | A：真实入口事件、焦点、结果；透明度绘制观测 |
| O02 | 入场中及终态分别 Esc、外点、再次点击触发器 | 同一事件周期关闭并移除交互子树，不等动画；触发器 down→up 不先关后开；关闭手势不穿透触发底层动作 | A：完整 down/up、底层哨兵动作、随后独立点击阳性对照 |
| O03 | 关闭带请求/订阅/旧 prior-focus 的入口；立即切线程/页面并让旧结果返回 | 原取消/代际栅栏立即生效；回焦仍按原所有权，不能定时回焦或抢新页面焦点；旧结果不复活浮层 | A：生产 controller + 外部服务替身；真实焦点 |
| O04 | 打开→中间帧关闭→迅速再开；另做同帧开关、重复刷新和不同入口交替 | 稳定身份、从当前可用视觉采样衔接，不排队、闪回或把其他浮层的进度借来；无关重绘不重播 | A：真实 owner、时钟、绘制采样；见 §4 的快重开候选与冻结门禁 |
| O05 | 关闭后 Tab/Enter、点击旧位置；检查隐藏子树 | 无 ghost Tab、命中、a11y 节点；非模态入口关闭后下一个独立手势正常可用，不留遮罩 | A：focus/hit；a11y 激活限制见 §6；M：VoiceOver |
| O06 | 开前/运行中开启 Reduce Motion；窗口失活、页面隐藏、入口移出可视区、卸载，再恢复 | Reduce Motion 直达终态；隐藏/失活/完成无持续装饰帧；恢复不补播过期序列；卸载不续订或 resurrect | A：门控、TestWindow 帧调度与生产绘制；M：OS Reduce Motion |
| O07 | 浅/深主题、窄/宽窗口、靠边触发、流式生成中重绘 | 锚点、尺寸、命中框、焦点环和 deferred 层级不动；背景边框不能盖过浮层 | A：debug_bounds、painted_quads 顺序/颜色；M：真实像素/观感 |
| O08 | 文件索引失败/警告、权限卡、错误和取消提示出现 | 真实安全/失败内容立即可见、可操作；不得因统一 wrapper 被淡入/延迟 | A：正常与失败分支对照 |
| O09 | 链接 hover 开启、离开/再进入；命令查询、分支查询尚未完成 | 保留链接 400ms 开启和 150ms 离开宽限；不追加等待；既有 worker/poll 不是动画时长 | A：测试时钟、原请求触发时机；不跑真实网络 |
| O10 | 生产长对话、长菜单、流式生成同时打开/关闭菜单 | 仍能输入、取消与关闭；不增加正文逐帧重测或底部跟随；不得用静态检查宣称性能改善 | A：合成数据驱动生产流；M：长对话体验，性能结论待证 |
| O11 | 宽窗 Environment、Workspace Dock、权限/错误卡及未纳入帮助层 | 原行为不变；本卡不做尺寸动画、不改权限策略/业务数据 | A：必要负向回归；静态范围审查 |

## 2. 已核实 API 与设计来源

- `Cargo.toml:91` 锁 `gpui-kit = 0.6.1`；`Cargo.lock` 的 `gpui-base=0.6.1`、`gpui-pre=0.3.4`。Kit 是 facade，motion 实际在只读 registry `gpui-base-0.6.1/src/motion.rs`；不再沿用设计守则中的旧 0.6.0/git GPUI 描述，不升级/新增依赖。`vendor/mdstream` 是 #529 基线，不能从旧 ref 恢复 render 文件。
- `vega_theme/src/lib.rs:314–319` 仅有 `Motion::INTERACTION_DURATION_MS=120`、`THINKING_SHIMMER_MS=1800`，下文新 token 尚不存在。
- `interaction.rs:161–216` 在 Element paint 里使用 `gpui_kit::base::motion::{transition, Transition}`；`interaction/platform.rs` 已有 Reduce Motion 平台桥，读不到系统值时保守为 true。沿用桥，不新做系统设置检测。
- `motion.rs:243–343`：`transition`/`transition_with_status` 使用稳定 key、第一采样直接采用 target、`background_executor().now()`、当前采样 retarget、缩短直接 reversal；duration zero/Reduce Motion 直达 target，只有 Delayed/Running 请求帧。首次条件挂载直接 target=1 **不会淡入**，必须测试初始化接线。
- `Presence::sample` 首次 present 从 0 开始，但只管理视觉是否保留，不能管理 focus/hit/a11y/业务取消。首轮不保留退出视觉子树，不把 `should_render()` 当交互许可。
- GPUI `Animation` 在 request_layout 阶段工作；Kit `MotionReveal::prepaint` 会以 MinContent 重测自然高度并可请求下一帧。两者都不能被称为免费/compositor-only。本卡不使用高度 reveal、不承诺零 layout。
- 依据：[Apple HIG Motion](https://developer.apple.com/design/human-interface-guidelines/motion)，已读官方数据正文，采用 purposeful、brief/precise、optional、可取消且不阻塞交互的原则；[固定 Kit Motion 文档](https://github.com/longbridge/gpui-kit/blob/96103905ea0c9c199db206ada7a9b2e3114e6339/website/base/motion.md)，并交叉核实以上 registry API。下文 120/80/1000ms 都是 **Vega 产品候选，不是 Apple 官方数字**。查阅日 2026-10-22。

## 3. 本轮确定入口清单（不以“所有菜单”替代枚举）

表中所有“纳入”都是完整卡范围；若因并发/接线暂不能实施，保持未决，不悄悄减为演示子集。仅整体表面入场，内部异步数据更新不再重播。现有焦点、取消、选择和关闭 handler 仍由原 owner 决定。

| 入口 | 生产文件/符号（`crates/` 下） | 包装位置/层级 | 既有延时与特殊边界 |
|---|---|---|---|
| E01 纳入 | `vega_ui/src/conversation_stream/render.rs::render_file_dropdown`，650+ | 文件选择表面；原 `deferred(...).with_priority(2)` 之内，关闭仍无交互 child | 索引/请求完成不是动画等待；loading 正常表面可淡入，`file_index_failure` 立即全不透明 |
| E02 纳入 | 同文件 `render_model_picker_layers` / `render_picker_slider_layer` / `render_picker_list_layer`，1214+ | 模型列表与 Thinking 档位层各自稳定 channel；原 priority 2、锚点/边距不动 | 无新增 timer；层级切换保持当前业务状态，不改模型/提交 guard；与 #535 串行集成 |
| E03 纳入 | `vega_ui/src/conversation_stream/utility_bar.rs::render_backend_menu` / `render_utility_projects_menu`，116/215 | Backend 菜单、项目选择各自表面，priority 2 | 立即打开；新建任务前提与选择动作不改 |
| E04 纳入 | `vega_ui/src/branch_selector.rs::Render::render`，773+ | 分支下拉整体，保留原 deferred/priority 2 与搜索焦点 | 现有 100ms `poll_current_head` 是业务轮询，不修改/叠加开场等待；错误分支旁路淡入 |
| E05 纳入 | `vega_ui/src/sidebar/threads_block.rs::render_row_actions` / `render_action_menu`，1265/1570 | 每任务操作菜单；保留 anchored TopRight/local/snap、仅菜单表面 deferred priority 2 | trigger 与菜单为不同命中层；置顶/重命名/归档/删除等语义原样，确认/危险提示不淡入 |
| E06 纳入 | `vega_ui/src/sidebar/threads_block/organization/menu.rs::render_organization_menu`，106+ | 分组/组织菜单变体，原 priority 3；以 menu owner/object 身份区分 | 不改组织存储状态机；无新增打开延时 |
| E07 纳入 | `vega_ui/src/conversation_stream/pending_messages.rs::render_pending_menu` / `close_pending_menu`，956/906 | 排队消息菜单，原 anchored/priority 3、原 menu_focus | 不改 composer/发送/过程逻辑；可能与 #535 写入重叠，父先确认单一写入者后接入，不能删验收 |
| E08 纳入 | `vega_ui/src/conversation_stream/selection_menu.rs::SelectionMenu::render`，49+ | message/Thinking 选择上下文菜单的现有 `PopupMenu` entity 外层表面；保留 `POPUP_PRIORITY` | `window.defer` 是事件阶段安排而非动画延时；不改选区；不得给 Kit 菜单再套第二个完整事件遮罩或修改依赖 |
| E09 纳入 | `vega_ui/src/settings/provider_management.rs::render_provider_management`，1493+（provider_menu 1749、model menu 1892） | provider/model 更多菜单各自表面，原 priority 2 | 保存冲突/测试失败/权限结果本身立即出现，不淡入错误帮助 |
| E10 纳入 | `vega_ui/src/image_preview.rs::Render::render`，234+，menu 422 | 图片“打开方式”菜单，原 priority 2、既有应用选择 | 只菜单表面；外部应用提交/保存结果不改，不宣称已打开 |
| E11 纳入 | `vega_ui/src/command_palette.rs::Render::render`，201+；`vega/src/app_palette.rs::toggle_palette/close_palette/sync_palette` | 命令面板实际卡片；背景拦截器不随透明度撤销，不移动原 root host | `close_palette:174+` 立即 cancel/generation++/view=None/prior-focus；worker 25ms poll 是结果收集，不是入场延时 |
| E12 纳入 | `vega/src/window/workspace.rs::render_workspace_layout` 所属布局路径 1425–1579 / `render_environment(true)` | **仅窄窗** Environment 覆盖卡片；原 outside layer、卡内 stop_propagation、右侧锚定不动 | toggle/close/Esc/响应式切宽/Workspace 覆盖立即失效；不改宽窗 rail 或 Dock 尺寸；父串行集成 workspace.rs |
| E13 纳入 | `vega_ui/src/link_preview.rs::MarkdownLinkPreviewLayer::prepaint` / `build_card`，386+/1101+；`arm/pointer`，813/913 | 链接预览卡，保留 Positioner、测量/命中/可视范围校验与已有绘制层级 | **400ms arm + 150ms leave grace 原样**；只在既有出现时刻开始视觉淡入，不另等 120ms；错误/拒绝/警告分支立即显示 |

明确排除但保留回归：`render_permission_picker` 与权限卡/确认、错误/警告/保存冲突、`render_settings_navigation_guard`；Settings `provider_test_help_tooltip` 与 `settings_help_open` 的帮助气泡、普通 tooltip、系统原生文件对话框；全页/整窗/宽窗 Environment/Workspace Dock 尺寸；message anchor 导航与 #344。这些不是“普通操作菜单统一入场”的入口，不向安全提示或系统组件扩散。若父审认为 Issue 的菜单范围还含其他具体入口，须在测试冻结前明确补入，不能实施后补验收。

## 4. 冻结候选方案与最小共享 API

### 4.1 时长、身份和门控

共享文件的未来唯一写入者是 #549；#550/#551/第三层只消费本节，不各自增 token/engine。

| `vega_theme::Motion` 候选常量 | 值 | 消费者/含义 |
|---|---|---|
| `OVERLAY_ENTER_MS` | 120 | E01–E13 整体 opacity 0→1；在 Issue 100–140ms 区间 |
| `DISCLOSURE_TURN_MS` | 120 | #550 固定槽箭头，不是正文高度 |
| `COPY_SWAP_MS` | 80 | #551 执行后图标入场，非剪贴板等待 |
| `COPY_RECEIPT_MS` | 1000 | #551 最新一次写入路径执行后的回执截止时间 |
| `TAB_SELECTION_MS` | 120 | #552 现有标签选中填色；不移动几何或正文 |
| `SETTINGS_NAVIGATION_MS` | 120 | #553 已通过守卫的局部标题/选中标记；不淡入表单或必要按钮 |

候选公共接线放 `vega_ui::interaction`（沿用既有平台桥，不新增 crate 或跨业务层类型）：

```text
decorative_transition(duration: Duration, visible: bool,
                      window: &Window, cx: &App) -> Transition
```

只返回 `Transition::new(duration)` 或 `Transition::new(Duration::ZERO)`；`visible && window.is_window_active() && !cx.reduce_motion()` 才允许运动。使用 Kit 默认 ease_out_cubic、无 delay；具体对象的真实可见性/路由/结束条件由消费者传入。此函数不保存状态、不安排 timer、不决定业务 open、focus 或 a11y。#550/#552 的上述时长和曲线已由主 Agent 冻结用于先行测试；#551/#553 仍是已记录的产品候选。共享文件必须只有本层一个 writer，不能由并行消费者提前添加实现。

策略函数本身不等于完整生命周期。`gpui-pre::Context::observe_window_activation`（context.rs:444）能收到失活/恢复；现有 `observe_system_motion` 仅在恢复时刷新系统设置，不负责让所有运动同步终态。消费者或最小共享挂载接线必须在失活、owner 隐藏/卸载时记录中断并使采样直达当前真实目标，或确保停用边沿产生一次同步；不能只等下一次活跃 render 才检查 gate。否则一段未达终态的 transition 可能在窗口恢复时继续旧行程，违反不补播要求。不得为此重复安装每帧 observer、增加全局 MotionManager 或暂停真实业务任务。先行测试必须包含“不在停用期间额外 draw、直接恢复”的顺序，证明旧动画不会补播。

浮层窄用途候选 `overlay_enter(id: ElementId, open: bool, visible: bool, content: Option<AnyElement>) -> AnyElement` 可同放 interaction：返回一个稳定、始终被 owner 渲染的轻量 carrier，内部调用 Kit transition。`open=false` 必须无交互/视觉 content；它不拥有业务取消/回焦/遮罩，不是通用 Presence 框架。公有 surface 只为跨 `vega_ui`/`vega` 的 E11/E12 使用。父审若可用已有元素组合实现同契约，可省略这一构造器，不引入 MotionManager、全局 registry、队列或每帧 layout scheduler。

### 4.2 入场与快重开候选（需前置阳性对照）

1. carrier 身份由现有 owner entity/消息或任务 ID + 固定用途组成；同一逻辑浮层复用，不能按行索引、时间戳、每次 render 随机值生成。body 的开关不能移走 carrier 的全局 element namespace。不同菜单/线程不共享状态。
2. 正常首次闭态 carrier 先在同一个 key 上以 zero-duration 采样 0；首次直接 mounted-open 的特殊路径先 seed 0、再以 120ms target=1。显式初始化解决 transition 首样采用目标的问题；两次同帧采样须在首轮 Actions 阳性对照确认，不把尚未运行说成可用。
3. 打开期间每帧只调用 Kit sampler，保存最后一次实际绘制的 alpha。关闭立即卸载 content/原焦点链并执行原取消；carrier 仅将该 channel zero-duration 固定在最后已绘制 alpha，记录 `closed_at`，不绘制、不保留命中/Tab/a11y child、不继续请求装饰帧。
4. **快重开定义候选**：同 owner 关闭后 `elapsed <= OVERLAY_ENTER_MS`，从保留的最后可见 alpha retarget 到 1；超过 120ms 的新打开先 zero-duration seed 0 再入场。该 120ms 复用本 token，不增加无限生命周期/计时任务；闭态只在下一次 owner 事件读取时间。关闭本身始终即时，没有退场 alpha 曲线。
5. 同帧 close/open 无闭态绘制时，owner 必须记录逻辑关闭边沿，不能只读最终 open 推断。已卸载 owner/切页/线程丢弃 channel；旧请求只能经原代际栅栏更新。隐藏/窗口失活/Reduce Motion 使可见打开层直接 alpha=1，恢复保持终态，新的显式打开才按规则入场。
6. 仅保留 `was_open / last_painted_alpha / closed_at` 等生命周期元信息；曲线/插值/时钟推进都委托 Kit，不实现第二引擎。若同帧 seed、全局 ID 留存或快重开 continuity 在真实生产 host 不可行，先向父报告 **O04 方案/观测阻塞**，不能退回“重新 mount 每次从零”或删除 O04。
7. 正常帧的透明度必须作用于**整个真实表面**（背景、边框、文字/图标），但布局/命中仍按原完整尺寸。自定义 Element 需完整转发 layout/prepaint/paint、a11y 与 child 生命周期；GPUI `with_element_opacity` 是 crate-private，不可虚构为可调用 API。可通过真实 Stateful Div 的 opacity + 子绘制组合，先在 Actions 证明所有子项共同淡入，尤其 deferred 仍位于原层级。

### 4.3 即时退出与安全边界

- 不使用 Presence 保留交互退出层。本轮没有 `Closing` 业务等待态，没有 on-animation-end 才关闭/cancel/focus 的代码。
- 原 trigger capture、防内部冒泡、outside handler 和 mouse-up 语义保留。关闭手势 down/up 必须作为同一手势消费，不能透明卸载后让 up 落到下层；若原入口缺此保证，是 O02 必须在实现前覆盖的行为目标，不把点击穿透称动画限制。
- prior-focus 仍由原 owner 按原规则恢复；新 wrapper 不保存/恢复另一个 FocusHandle。特别 `SelectionMenu` 原本只在菜单仍持焦或焦点为空时回焦，不能统一成无条件。
- 普通菜单里出现新的错误/警告时，立即 snap 整个表面至 alpha=1，维持恢复动作。权限/确认/错误卡从不经过普通装饰淡入。

## 5. 逐文件/符号动作与并发边界

以下都是未来最小实现计划，本阶段未执行。

| 文件/符号 | 动作 | 不动部分 |
|---|---|---|
| `crates/vega_theme/src/lib.rs::Motion` | #549 单一 writer 按消费者阶段提供 §4 共享 token | 已有 hover、Thinking token 不改值；无消费者的尺寸抽象不新增 |
| `crates/vega_ui/src/interaction.rs` | 窄 `decorative_transition` 与必要 overlay carrier；复用 `platform`；先验证 opacity/seed 接线 | 不重构既有 Interaction hover/pressed，不另做 Reduce Motion bridge |
| §3 E01–E10 的 renderer | 只在原整体表面接 carrier，closed 分支仍无 body；沿原 global identity 放 carrier | 原事件/菜单项目/业务/权限/选择/anchor；不动 vendor 或业务存储 |
| `command_palette.rs`、`app_palette.rs` | 前者接卡片，后者仅必要 owner open 生命周期接线 | cancel/generation/view/prior_focus 时序，worker/搜索逻辑不改 |
| `window/workspace.rs` E12 | 仅窄窗卡片 alpha 接线；与父/第三层串行集成 | #554 仅评估；宽窗/Workspace/Dock/PTY 身份原样 |
| `link_preview.rs` E13 | 既有真实 card 绘制层接 alpha；失败 bypass；不二次包装交互 | 400/150ms、请求 cancel、读区/滚动失效、Positioner 原样 |
| 未来临时测试 modules / dev feature | 仅在下一阶段获授权时添加本轮用例与观测接线 | 不恢复旧测试全集；不改 CI topology |

#535 的 composer/render/过程与 E01/E02/E07 有潜在交叉，不能从本 worktree 整文件覆盖；#344 的 `message_anchors` 不写。父须在下一阶段分配明确符号与合并顺序，必要时先同步其最新生产基线并重新验证同一冻结用例。当前第三层只在其 #552 worktree 写 Workspace 先行测试/只读接线；本层 E12 的 workspace.rs 生产接线尚未开始，必须等待该版本停写后串行集成。共享 Motion/interaction 生产始终只由本层 writer 提供。

## 6. 测试可观察点、具体 blocker 与最小下一测试包

### 6.1 已核实可用的观测层

- 已有 `.github/workflows/pr-check.yml`：macOS Clippy + Nextest，`pull_request` base master；仅父创建/更新 draft PR，测试阶段临时启用现有 Kit `test-support`（向 gpui/gpui_platform/gpui-base/component 转发），不新增/升级依赖或 workflow。
- `TestAppContext`/`VisualTestContext` 可创建真实 view、draw/输入事件、focus、deactivate；`executor().advance_clock` 正是 Kit 时钟，不需真实 sleep。用真实窗口渲染完成的 frame 读取 `Window::painted_quads()`；`VisualTestContext::draw` 自身不是 renderer 截图证据。
- alpha/层级：选真实浮层 bg/边框 quads 的颜色 alpha、内容 mask、绘制顺序，配相同 bounds 的底层阳性对照。quads 是 scaled pixels，debug_bounds 是 logical pixels，必须按 scale factor 换算。quads 不含 SVG/文字，不能据其 alone 声称整层子元素淡入。
- 帧：`Window::simulate_next_frame(cx)` 是公开 test-support 路线，可观察待调度数；不要误调用 crate-private `TestAppContext::test_window`。TestWindow 自身有 `frame_scheduled/frame_wake_count`，但未证明应用外部可直接取得其实例。先完成排空已有通知/首帧的阳性对照；实际流式业务/shimmer 也请求帧，不能把窗口全部唤醒归因浮层。
- 交互：同一次事件后检查生产 open/view/cancel 和 focus；通过真实目标点击确认不可交互而不是仅检查 bool。生产选择结果/请求栅栏不能由 mock 重写，替身只隔离外部 IO。

### 6.2 不可假定的观测与下一验证

| 限制/证据 | 分类 | 下一最小验证，不降低验收 |
|---|---|---|
| GPUI `Window::debug_a11y_tree_json()` 存在，但 `platform/test/window.rs` 无 `a11y_init/tree_update` 实现，trait 默认为空；active flag 默认 false | 测试平台观测缺口，不是已证产品缺陷 | 先行测试用真实生产 Element 的 `write_a11y_info` 检查节点属性与 child 挂载，同时 parent 决定是否有获准的 test-only platform hook；全树消失/VoiceOver 仍交用户手测。不得 `None` 当“空树通过”，不修改 registry |
| quads 不覆盖 SVG/字形整体 alpha，`with_element_opacity` 非公有 | 绘制边界接线待证 | 在原生产绘制入口加可删的 `cfg(test)` 只读 recorder，记录已交给实际 child 绘制的 alpha/身份/geometry，配真实 surface quads；不能只观察 sampler 数值。若不能连接到实际 child paint，O01 阻塞，不以 toy 替代 |
| 首次 mount-open seed、快重开冻结与跨帧 global key 保留未执行 | 方案待 Actions 阳性对照 | 在包含真实 E01 host 的先行补丁中冻结 O01/O04 时序/断言，API 尚不存在时仅接受明确最小接口缺失 RED；接口落地后同断言必须真实执行 |
| 业务通知与装饰帧混合，离屏 group hover gate 不等于 owner 可见 | 观测归因/门控待证 | 先在真实入口静止 fixture 排空所有业务任务；增加流式对照只比较生产 invalidation/绘制结果，不硬说窗口必须 0 帧；门控必须来自实际 owner/viewport |
| E01/E02/E07 与 #535、E12 与第三层同文件 | 集成/写入范围前提 | 父先明确单一写入者与顺序，再测试→独审→Actions；不抢占生产写入 |

### 6.3 最小先行测试包（整体先写齐，再 RED）

1. **入口表驱动生产集成包**：E01–E13 每入口 open/close/选择与 E01/E04/E09/E13 失败 bypass；保留布局/原延时。可按共用真实 host 合并 fixture，但不能省去入口。
2. **主失败目标** `overlay_enters_without_delaying_interaction`：打开 E01（真实 ConversationStream）与 E11（真实 palette owner），t=0 业务可用且表面尚未达全 opacity，t=60ms 中间值，t>=120ms 终态；基线目前直接全 opacity，应为有效行为 RED。不能拿“不存在测试 ID”或依赖错误算 RED。
3. **必要中断/安全包** `closing_consumes_gesture_and_invalidates_owner`、`reopening_continues_last_visible_sample`：入场中 close/reopen、触发器 down/up、outside、Esc、切线程后旧结果、焦点/Tab/下一独立点击、闭态零装饰调度。即使旧基线部分回归已绿，也与新失败目标一并冻结。
4. **门控与层级包**：Reduce Motion、inactive/hidden/unmount；两入口同时存在；浅深、边界位置、长流式真实 renderer，deferred bg/边框顺序；安全/错误即时对照。a11y/SVG recorder 接线必须先独立审查。
5. 所有生产行为未改时写齐上述测试和最小 test-only hook；独审后父触发现有 `cargo nextest list --workspace --message-format json`、非空 `cargo nextest run --workspace`，并保持 fmt/全目标 Clippy/Python 门禁。**本地不运行这些命令**。接口缺失 RED 必须明确行为断言尚未执行；网络/编译接线/runner 错误不算 RED。

用户手测保留：真实系统 Reduce Motion 切换/窗口失活、VoiceOver 全树/焦点、整层文字/图标与像素层级、生产长会话/流式观感。未授权 CUA、本地运行或 profile，暂无性能结论。

## 7. 交付门禁、回滚与当前证据

- 此版本仅可供 spec review；共享 API/token、120ms 快重开定义、a11y/SVG 观测接线及交叉文件顺序须父审冻结。矩阵不是测试，不允许据此直接实现。
- 父后续：先行测试→非写入者独审→现有 Actions 有效 RED→最小实现→同一测试 GREEN→PR 固定测试源码/完整 SHA/原始失败与计数→独立删 Rust 测试及独占支持→最终 head 完整空集门禁。无关失败不能弱化断言或移除入口；验收变化重新开轮。
- 回滚候选：只移除表面 motion 接线/新 token 的已确认消费者，恢复原即时打开/关闭；不得回滚 #529/#535/#344 或业务取消、焦点、安全修复。由父经 PR 操作，本阶段不执行。
- 当前本地只做只读源码/registry/GitHub/API 调研及文档静态检查；未运行 Vega、cargo/测试/bench/profile、未 CUA、未发起 Actions，RED/GREEN 均未取得。
- 未提交文件 SHA-256、可重建 diff 标识、精确 HEAD、静态退出码与停写声明在本轮 READY 报告给父；本文件不嵌入自身 hash，避免自引用变更。三张卡各自只有一份新规格，`docs/README.md` 不动。

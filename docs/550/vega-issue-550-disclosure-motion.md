# #550 Thinking、工具与过程折叠箭头可中断过渡

状态：本轮验收冻结、先行测试准备；未独审、未取得有效 RED，不允许生产实现。旧 v1 仅为设计稿，不等于 READY。

- 本轮生产基线/本 worktree HEAD：`2ba904f7074115de5ee89cc18c376c083f803357`。父已 fetch 并 ff；相对旧 `871f91f246e90753743a256cee1eec315e74effc` 仅 docs/#554，生产不变。本 writer 不重复更新 Git/index。
- 分支：`feat/550-disclosure-motion`；worktree：`vega-550-disclosure-motion`。旧 writer 当前 run 不可用且已释放写入权，本轮唯一 writer 接续保留的规格，不重建树或恢复旧 Rust 测试。
- 本轮允许：本目录规格/证据；`vega_ui` 新增 disclosure/production-path 测试、mod/lib 最小 cfg(test) 接线；`disclosure.rs`/`icons.rs` 仅 cfg(test) 只读 observer；dev-only 既有 `gpui_kit` test-support。锁文件仅必要 feature 解析并由父协调。禁止生产函数/行为变化、源码注释、新依赖/升级、interaction/theme/workspace 及 #535/#344/#546 共享生产修改。
- 本轮不 commit/push/改 PR/Issue/Project，不嵌套委派；父掌 Git、独审、draft PR 和 Actions。主工作区用户脏改动不碰。已重读本树 AGENTS、执行指南、Issue 工作流、设计守则、kanban skill、Rust 测试生命周期；目标路径无 nested AGENTS。本地不运行 Vega/编译/build/check/Clippy/test/bench/profile/CUA，只快速格式/静态检查。
- 父冻结：两个 helper 使用单 `ChevronRight`，收起 0、展开 π/2，120ms、Kit 默认 ease_out_cubic、stable key 连续反转；首挂直接终态；仅图标动画，正文/aria/点击/focus 即时；Reduce Motion/失活/隐藏/完成停止装饰帧；不做高度动画。#549 后续同 writer 定义 `DISCLOSURE_TURN_MS=120` 与 `visible && active && !reduced` 最小策略，本阶段不新增这些生产 API。#549 保留 13 浮层入口，#551 保留 hover/focus、不 toast/sticky，本阶段均不写生产。D01–D11 与 #416 全部边界保持不变。
- 已读 [Issue #550](https://github.com/puzige/vega/issues/550) 正文及 [2026-10-22 16:26:53Z 最新评论](https://github.com/puzige/vega/issues/550#issuecomment-6345793007)。旧 Issue 行号仅作索引，具体事实以本轮 SHA 为准。

## 1. 场景与验收矩阵（先于方案）

全部待验证。A=Actions 真实生产路径；M=用户真实系统/观感手测。每条共享箭头入口都必须纳入，不能只证明 transition 数值函数有效。

| ID | 前置/操作 | 冻结目标 | 观测 |
|---|---|---|---|
| D01 | Thinking、工具、工具组、过程总头、过程阶段各展开/收起 | 固定槽内箭头约 120ms 改向；内容与 aria-expanded/点击结果同事件周期改变，不等动画 | A：真实 header 事件、可见正文、focus、生产绘制；a11y 限制见后文 |
| D02 | 展开→未完收起→再展开，多次重复 | 当前角度连续改向；最终方向与真值一致，不排队、不闪回起点 | A：同 owner、测试时钟、绘制参数采样 |
| D03 | 首次挂载、历史重入、虚拟列表重新挂载、无关流式重绘 | 直接采用当前真实方向；不播伪造初始折叠、无关刷新不重启 | A：生产 renderer 与身份 |
| D04 | 默认/hover/键盘 focus、禁用、无内容 | 沿用现有 expanded/hover/keyboard-focus 显隐及可发现性；禁用不触发、热区和焦点顺序不改 | A：mouse、Tab、Enter/Space、focus 真实路径 |
| D05 | Reduce Motion、隐藏、离屏、失活、终态，再恢复 | 直接终态或停止装饰帧；恢复不补播；Thinking shimmer 原门控不回归 | A：帧调度/生产门控；M：OS Reduce Motion |
| D06 | 长正文/长工具输出；箭头每个中间帧 | 内容只因真实折叠发生一次可见性/几何改变；不逐帧高度插值、不因装饰反复 invalidate/re-measure 消息 | A：debug_bounds + 原生产失效/测量边界观测；不能仅断言“无 layout” |
| D07 | 首次可信 Final；之前已上滚、选字、过程持焦或手动展开 | 保留 #416：本轮过程立即收起一次，前述阅读状态不阻挡；清理隐藏选区、必要回焦到真实可见 header，Enter 能再展开 | A：真实 Final 投影、选区与 focus，不伪造隐藏焦点 |
| D08 | 首次收起后手动展开，重复 Final/完成/流式重绘 | 手动优先、无二次自动收起；Final 原文不重复、不截断 | A：生产状态转换/正文与复制来源 |
| D09 | Composer、Final、其他轮、同阶段产物持焦/有选区；本轮收起 | 不抢无关焦点，不清可见选区；原锚点与脱离底部跟随保持，不操作 #344 的 message_anchors | A：跨轮/产物与滚动对照 |
| D10 | 历史多轮、坏/缺 Final、失败/取消、安全入口 | 不用动画推定 Final；未知正文不隐藏，错误/权限可见；行为沿用既有契约 | A：生产历史 hydration/异常事件 |
| D11 | 流式长思考、长输出、窄宽窗口、浅深主题 | 标题起点、展开热区、槽位尺寸、正文基线稳定；不掩盖 #545/#395 性能问题 | A：生产几何；M：长会话体验/性能待证 |

## 2. 源码/API 事实与依据

- 本基线是 `gpui-kit 0.6.1 / gpui-base 0.6.1 / gpui-pre 0.3.4`，不是旧设计文档的 0.6.0/git 依赖；不加/升级依赖。#529 的 vendored mdstream 和当前 render 分块必须保留。
- `crates/vega_ui/src/disclosure.rs:12–93` 的 `chevron` 与 `inline_chevron` 都直接替换 ChevronRight/ChevronDown。inline 槽为 `Layout::CONVERSATION_CHEVRON_SIZE=12`；Thinking trailing 槽宽 `DISCLOSURE_TRAILING_SLOT=24`、图标 `ICON_SIZE=16`。显隐条件是 enabled 且 expanded 或键盘 focus，另加既有 group_hover；本卡不把收起箭头改常驻。
- `gpui_kit::base::motion::transition` 是 facade 到 `gpui-base-0.6.1/src/motion.rs:243–343`：第一采样立即采用 target，稳定 key retarget 保持当前值，直接反转缩短返回时长；Reduce Motion/zero duration 直达终态。时钟是 `cx.background_executor().now()`，测试可 `executor().advance_clock`。
- GPUI Kit component `Icon::rotate(radians)` 存在（registry `gpui-component-0.6.1/src/icon.rs:164`），交给 SVG transformation，不改变父布局槽。Vega `icons::sized_icon` 返回 `AnyElement`，**不能直接对其假调 `.rotate()`**；需在变为 AnyElement 之前窄接 Kit 图标，或在 disclosure 中构造相同来源的图标。
- `interaction.rs:161+` 已有 Element paint 使用 transition 的生产例子；`interaction/platform.rs` 是现有 Reduce Motion 桥；`conversation_stream/shimmer.rs` 已按实际可见/active/reduce 门控，不重做 shimmer。
- `run_activity.rs::settle_final:457+` 清隐藏 Thinking 选区、`expanded=false`、必要时 focus 到同一个 header handle；`phase.rs::settle_final_presentation:654+` 清对应过程 copy 选区并恢复原滚动策略；`core.rs::invalidate_item:832+` 参与 item 失效/列表重测。箭头不接管这些业务决策。
- 官方依据：[Apple HIG Motion](https://developer.apple.com/design/human-interface-guidelines/motion)（已读官方正文：短、准确、可取消、可选、不阻塞）；[固定 Kit Motion 文档](https://github.com/longbridge/gpui-kit/blob/96103905ea0c9c199db206ada7a9b2e3114e6339/website/base/motion.md)。查阅日 2026-10-22；120ms 是本轮父冻结产品值而非官方数字。

## 3. 确切入口、候选行为与共享接口

### 3.1 本轮入口与逐文件动作

以下是未来实现动作，本阶段都未执行。行号用于本 SHA 定位，优先按符号查找。

| 文件（`crates/vega_ui/src/` 下）/符号 | 入口/动作 | 保留不动 |
|---|---|---|
| `disclosure.rs::chevron`、`inline_chevron` | 唯一共用方向过渡入口；用同一 Right glyph 的 0→π/2 rotation，封装本地窄 Element；保持两个槽位和原色/大小 | 原 FocusHandle、显隐语义、无额外 Tab/hit/a11y 节点 |
| `icons.rs::kit_icon` 附近（仅必要时） | 增加 crate-private 的窄旋转 chevron 绘制接点，在 AnyElement 前调用真实 Kit rotate；复用 icon mapping | 不改 Copy、Fork、主题、通用 icon_button，不重做图标体系；与 #551 修改错开 |
| `conversation_stream/thinking.rs::Render::render`，533+ | Thinking 源正文 disclosure，调用 `chevron`；传真实 owner 身份/可见门控 | `set_expanded`、选区、滚动条、正文与 shimmer 语义 |
| `tool_card.rs` 的工具 header，754+ | 独立工具卡 `inline_chevron` | 真可展开条件、命令/状态文字、选区、错误与内滚动 |
| `tool_activity_group.rs` 的组 header，263+ | 工具组 `inline_chevron` | 展开 children 的同步条件与固定布局 |
| `conversation_stream/run_activity.rs::render_with_details`，1278+ 与阶段 header，1712+ | 过程总头/阶段两种 `inline_chevron`；稳定实体/阶段身份，不能按可变 row index | 原 toggle、`aria_expanded`、真实 header focus、手动优先与阶段/工具业务 |
| `conversation_stream/run_activity.rs::settle_final`、`phase.rs::settle_final_presentation`、`core.rs::invalidate_item` | **生产只读回归边界**；测试观察选择清理、focus、重测 | 不为箭头改 Final/锚点/底部跟随逻辑；#535/#344 的实现范围不碰 |
| `vega_theme/src/lib.rs::Motion`、`interaction.rs` 的共享生命周期 | 只消费 #549 接口，不在 #550 分支并行写 | #549 为唯一共享写入者 |

侧栏树组织展开/持久化状态机、消息锚点、正文/数字/token 动画、全页/尺寸动画不在范围。不要将改一个 inline helper 等同覆盖 Thinking 的 trailing helper。

### 3.2 本轮冻结行为（生产尚未实现）

- 从 #549 提案消费 `Motion::DISCLOSURE_TURN_MS = 120` 与 `interaction::decorative_transition(Duration, visible, &Window, &App) -> Transition`。后者只是 Kit Transition 的时长/门控选择，不保存业务状态或制造引擎。#549 独占定义；第三层只消费 spec 候选。当前分支没有这些 API，不得将提案说成现有实现。
- 使用 `transition_with_status((真实 owner + disclosure 用途, angle channel), target_radians, policy, window, cx)`；target 为收起 0、展开 `FRAC_PI_2`。沿用 Kit 默认 ease_out_cubic，无延时、无 spring、无循环、无 timer；持续 retarget 交给 Kit，不实现自己的角度队列。
- 初次/历史/虚拟列表 remount 直接 target；本卡**不 seed 从零**，利用 Kit 首样 adopts target。只对同一已挂载 glyph 的真实 expanded 变化做动画，无关 title/stream/主题变化不更换 key。
- 内容 visible、aria-expanded、selection cleanup、focus 和业务 toggle 在原事件处理/渲染周期完成，不等 120ms；箭头纯装饰，不能用动画结束 callback 修改 expanded。
- 固定槽内旋转；按现有 12×12 或 16×16 图标的中心旋转，父 12×12/24×16 槽不变。90°=`π/2`；中心分别 `(6,6)` / `(8,8)`，只改变图形方向，不能将整行宽高乘动画进度。不改标题 gap、hitbox、文本起点、正文高度。
- 不用 `MotionReveal` 或逐帧高度插值。Kit MotionReveal prepaint 会测自然高度，GPUI Animation 可触发 layout；本方案也不宣称 compositor-only。验收是**无由装饰引起的 item invalidate/正文重测**，不是无任何 GPUI layout 调用。
- 可见性为 glyph 实际会显示且在可视范围内、owner 页面可见；active/reduce 由 #549 gate。失活/隐藏/禁用同步 target 并停止调度，恢复显示当前方向，不补播。若折叠后既无 hover 又无键盘 focus，原规则立即隐藏，不为动画尾帧延长显隐。
- group_hover 的最终样式由 GPUI 私有 `GroupHitboxes` 决定，不能假设公开 `is_group_hovered()`；在真实 header 的现有/窄 `.on_hover` 与 FocusHandle 上提供实际门控，或者使用能观察最终可见样式的真实 Element 接线。父审后在先行测试阶段确定最小做法，保持 group_hover/keyboard-focus 真值，不创建平行“是否展开”状态。若无法接实际可见性，D05 是 blocker，不以“最多再跑 120ms”删约束。
- focus/Tab 目标仍只有既有 header；装饰 SVG 不增可聚焦/语义子树。disabled/can_expand=false 维持原行为。

### 3.3 #416 必须原样保留

参考 [#416 规格](../416/vega-issue-416-final-collapse.md)。首次可信 Final 收起一次；之前上滚、选字、持焦和手动展开都不能阻挡第一次收起。之后用户再展开优先，不被重复 Final/完成消息强制收回。隐藏部分选择清理，但 Composer/Final/其他轮/可见产物不被清理或抢焦点；回焦用当前真实 header handle，必须以实际 Enter 再展开证明，不在流顶层按字符串另造 keyed focus。箭头中间角度不改变上述时序。原滚动锚点/脱离底部跟随与 #344 并行实现隔离。

## 4. 测试可观察点与有效 RED 设计

### 4.1 观测能力、限制与下一动作

| 要证明的行为 | 可行生产观测 | 未决/限制与最小下一验证 |
|---|---|---|
| 方向中间帧、连续反转 | 真实 header 事件→实际 glyph paint 参数（稳定 ID、asset、rotation、bounds）→Kit SVG；测试时钟驱动 | `painted_quads()` **不含 SVG transform**。需要先行补丁的可删 `cfg(test)` recorder 接在真正生产图标绘制边界，不是只记录 transition 返回值。阳性对照同时证实 glyph 在真实界面树/槽内；若不能观测实际绘制，D01/D02 阻塞，不能用纯数值 toy 宣称 UI 成功 |
| 几何与正文即时变化 | `debug_bounds`、真实展开 children、生产 focus/selection；箭头/标题/正文 bounds | quads 单位是 scaled，debug_bounds 是 logical；各 scale 统一换算；不能将 bounds 不变当旋转有效 |
| 不反复重测 | `core::invalidate_item`、真实列表测量/高度缓存边界的只读 test recorder，比较相同输入有无装饰帧 | 不把 GPUI request_layout 调用计数当消息重测；不改 #545/#344 实现或另建性能 harness |
| 帧结束/门控 | 公共 test-support `Window::simulate_next_frame(cx)`、真实 hidden/deactivate/Reduce Motion，排空通知后检查不再续帧 | TestWindow 有 frame counters 但 `TestAppContext::test_window` 为 crate-private，不能假调；长流式业务仍可请求帧，需要相同业务对照/本 channel 调度记录，不能硬判整窗必须零帧 |
| aria-expanded 即时/隐藏节点 | 真实生产 Element `write_a11y_info` 属性 + 内容挂载/focus 结果 | GPUI TestWindow 未实现 `a11y_init`；`debug_a11y_tree_json()` 可能 None，不等于空树通过。无获准测试平台 hook 前，完整树/VoiceOver 为用户手测；不得改 registry |
| OS Reduce Motion、观感/长会话性能 | 用户在实际构建上的纯手动操作 | 本地/自动真实 E2E/CUA/profile 均无授权，不声称已验证 |

基线已经清零 Rust 测试，不能复制历史全集或拿过去 #416 GREEN 充本轮。测试特性临时启用现有 `gpui_kit` test-support，依赖版本不变；所有必要 recorder/helper/feature 在同一冻结测试 GREEN 后独立删除。

### 4.2 下一最小先行测试包（须全部先于生产）

1. **五类入口生产路径参数化测试** `disclosure_turns_while_content_changes_immediately`：Thinking、独立工具、工具组、过程总头、阶段。先 mount 当前态，真实 click/Enter/Space 改展开，立即断正文/aria 真值，t=60ms glyph 在 `(0, π/2)` 中间，t>=120ms 为 target。基线目前直接 Right/Down 替换，**中间方向断言预期为有效行为 RED**。先证 recorder 能区分两个端点/真实绘制，不能拿 harness 空数据当 RED。
2. **反转/身份/几何包** `disclosure_retargets_without_replaying_mount`：展开 40ms→收起 20ms→展开，比较同一切换瞬间前后角度连续（容差只用于浮点，不放宽端点/时长）；终态正确、无队列；首次/历史 remount 直接 target；逐帧标题槽/热区/body bounds 固定，无动画触发的 item invalidate。共享函数存在与否本身不算 UI 证据。
3. **可发现性/门控包**：hover/focus/无内容/disabled，两个独立 owner 不串；Reduce Motion、离屏/隐藏、inactive/恢复，完成后无装饰帧；原 Thinking shimmer 启停不回归。
4. **完整 #416 生产回归包**：D07–D10。前置必须真实展开、实际 drag/select/focus 到可见入口；首次 Final 清隐藏选区并回焦，按 Enter 可展开；之后手动保持；Composer/Final/其他轮/产物焦点选区、滚动锚点/非尾随与历史坏/缺 Final/失败取消各保留。不能删“难造真实前置”的焦点断言求绿。
5. **长内容/流式包**：合成实际 ConversationStream 数据，箭头阶段变化和多轮 Final 期间输入仍响应；只断可观察业务/几何/无多余失效，性能与观感交手测。

测试源码和接线独审后，父使用已有 macOS PR Check：`cargo nextest list --workspace --message-format json`、非空 `cargo nextest run --workspace`，并保留 fmt/全目标 Clippy/Python。以上仅是 **Actions 执行计划，本地未运行**。若新 glyph 绘制接点尚不存在，可记录最小接口缺失 RED，但必须标行为断言尚未执行；依赖/错误 API/renderer 接线失败不算有效 RED。不能实现后再补所需回归。

## 5. 前置、集成、回滚与当前证据

- 父需先冻结 #549 shared API/token 与本卡 glyph recorder/实际可见门控方案；共享文件只有 #549 写。`icons.rs` 若 #550/#551 均需改，按窄符号顺序集成；#535 过程/renderer 与本卡 run_activity caller 只能父协调，不覆盖；#344 message_anchors 不改。
- 规格与上述未决可审，不代表测试已就绪或实现可开始。下步只授权先行测试时，再写齐→独审→Actions 有效 RED→最小实现→同一测试 GREEN→固定源码/SHA/run/job/计数→独立删 Rust 测试与独占支持→最终完整空集门禁。验收变化先停本轮并重新 TEST-FIRST。
- 回滚候选仅恢复两个 disclosure helper 的即时 glyph 与本卡调用接线，移除无人使用的窄 icon 接点；不回滚 Final/锚点/源码正文或其他任务修复。由父走 PR，本阶段不执行。
- 当前未运行 Vega、编译/check/Clippy/tests/bench/profile、未 CUA、未启动 Actions，RED/GREEN/用户手测均待证。只进行了只读 API/源码调研及文档静态检查。
- 原规格阶段只新增本文件；当前测试阶段产物与原始失败/静态证据另见 [writer 检查点](vega-issue-550-test-writer-checkpoint.md)，不能把它当完整测试 READY。交接须附确切 HEAD、文件 SHA-256、可重建 diff 标识、静态退出码与停写声明；不在文件内写自身 hash；未改 `docs/README.md`。

### 5.1 父 Agent 停写后诊断与最小补齐契约

原 writer 已停写，父 Agent 核对测试 SHA-256 `4e29d257bad073cfd64d70989b1c00e618a8053485fc5ec7a28cdf8b465dacc4`、recorder `c1bd496ccc12c8ad6fc5c49d21a13cad80b4b2cef593206009c59097ed9193c4`、检查点 `e06321ab8b5db368fd0828b3a2aba631d464c1d33fbe957fdd4d543ac116575d`。本树随后 fetch 并从 `2ba904f7074115de5ee89cc18c376c083f803357` 快进至 `6615748f91273bfb8ee424011b3b5a09b3777d2b`；tracked 测试/接线 diff SHA-256 `d83e4e5cf2fff6021d19d363e2123a30137b52ede1ea875ccae97238d1070043` 前后一致。#547/#546 保留，普通生产改动仍未开始。下列只消解原 D01–D11 的测试前提，不减少验收：

1. **绘制参数证据而非私有 Scene 读回**：原 §4.1 已允许真实图标绘制边界的 asset/rotation/bounds 参数记录。后续最小生产图标构造应在同一个窄函数内把同一个 `radians` 交给真实 `KitIcon::rotate(radians)`，并将构造出的真实 element 与配置参数交给测试期透明 paint recorder；不能录独立计算值、transition-only 状态或常量。当前未旋转基线准确记录 0；所有中间/端点断言原样保留。先行测试阶段可补一个只在测试中的真实 KitIcon 旋转配置/委托 paint 阳性对照和 recorder 接口，生产箭头不变。未来独审须核对记录参数与实际 rotate 为同一值、单 glyph/真实 asset/固定 geometry；这不是 GPU scene/pixel transform 的独立回读，不冒充用户观感证据。不得改框架/registry、新增 image/renderer 依赖或自造旋转引擎。
2. **精确失效与测量对照**：允许本卡树在 `core.rs::invalidate_item`、既有 `remeasure/remeasure_items` 调用边界加入仅 cfg(test) 只读计数，以及在生产 `render.rs` 的 list item 元素增加仅测试的透明 request_layout/bounds 观察。保留所有真实调用顺序和 #344 算法。锁定 List 源码每次正常布局本就会测量可见项，不能要求整个 GPUI 不做 layout，也不能用 bounds 未变代替失效证据；同一输入/时钟/窗口下对比装饰 redraw 与已终态 redraw 的真实消息测量和显式失效，保留真实折叠导致的必要一次失效。观察器不得改变 List 或持久化逻辑。
3. **产物测试前置**：允许默认关闭、仅由 vega_ui dev dependency 启用的 `vega_conversation/test-support`，在 conversation 拥有者内提供纯内存、测试期路由投影 fixture。仅用于真实 ArtifactCard 挂载/焦点与可见性，不当真实 Git/文件权限能力验收；不得对普通构建暴露构造器、transmute 或修改 sealed ID 字段可见性。若使用 service 身份，沿既有 nonce/seal/projection 规则生成，不调用 Git/open/网络。ArtifactCard 正文当前非 selectable，不能伪造其隐藏选区；验证其真实可聚焦控件在首次 Final 后仍存在、保焦，且其他真实可选 Final/Thinking 的选区不被清理，原 D09 不削弱。
4. **补齐组合而非交给实现后**：D04 的真实 keyboard Tab/disabled、D05 的无失活 draw 快速恢复与 offscreen/shimmer、D07 的真实上滚/选择/回焦、D10 的必要权限入口、D11 的长 Thinking 流式交错/输入，均先写齐断言并停写。headless a11y tree 不可用时须记录系统层缺口，同时尽可能读取真实 Element 的 aria-expanded 属性，不将 None 当空树通过。

父 Agent 复用同一 writer，仅写原测试和上述临时支持。完整停写包经非写入者独审后才建 draft PR 运行现有 Actions。当前没有有效 RED/GREEN，不放行生产；若指定观察仍技术不可行，给出精确 API/证据与最小下一步，不用删测、常量 recorder 或无关编译错误换绿。

## 2026-10-24 合并阶段的可观察契约解释

用户已授权本卡继续至 master；本段解释先前失败测试与原 D01–D11/#416 的映射，不按现有输出降低目标。

- D07/#416 的首次自动关闭对象是同一轮 `RunActivityGroup` 过程总入口。子 Thinking 的局部展开偏好不是“过程已关闭”的判据；本文件早先提及的 `expanded=false` 在该语义下指过程总入口。应通过 Final 前真实源正文已绘制的阳性前置、Final 后真实正文/阶段控件不再绘制、隐藏选择清理/焦点迁移、真实 Enter 重开/重关、原文完整及重复 Final 不二次收起来验收。无需改变 Final 算法来强清子卡偏好。
- D06 的“一次真实折叠”指真实被切换的 run 入口条目一次显式失效；过程汇总、阶段和 owned commentary 是不同列表项，各自必要失效不能混为同一条目重复。观测保留原始全量事件，按真实 owner/动态 run entry index 严格验证该条目恰好一次，且后续装饰帧对全部相关条目均不得额外 invalidate/显式 remeasure；相同输入的 settled redraw 测量对照保留。不得把全量预期改为 3、去重或删记录。
- D07 的真实 source 拖选须起于实际已准备并 paint 的可选文本 run/hitbox，按同一 source copy 身份与现有 layout/bounds/content mask 定位。不能以 chevron 的猜测坐标、直接改缓存或常量 selection 代替真实鼠标事件；原非空、清理、焦点/Enter 与原文字节断言保留。
- D04 的无内容/disabled Thinking header 应跳过键盘 Tab，已有可展开 Thinking 保持可达；显式 `FocusHandle` 的 tab-stop 必须随真实 `can_expand` 同步，不能只设置外层 div 标志。稳定 handle identity、enabled 顺序与原热区/显隐保持。
- D05 的目标继续是失活后恢复不补播，包括没有失活 glyph 采样的情况。当前 Headless 自动 draw 与原严格前置的关系仍在只读核查；没有固定合法观察路线前，不删除或放宽该回归来合并。

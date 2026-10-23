# Issue 307 · Computer Use 首次启用准备引导（冻结规格）

基线：`origin/master` @ `e4f18c65`。分支 `feat/307-cua-onboarding`。

本规格冻结 #307 的第一轮范围。用户 2026-10-22 裁决：采用悬浮卡片形态（方案 B）；浏览器扩展流程不在本轮；**卡片跟随系统设置窗口移动**（2026-10-22 二次裁决，取代“先不跟随”）。

第二阶段恢复检查点：同步基线 `92f281e5a5a56230fa4e4a8a8cf1ab6f20e9670b`；测试计划见 [phase2 TEST-FIRST](vega-issue-307-phase2-test-plan.md)。本次修订纠正 R12 阴影符号，限定 F7/F12/R19 的源码证据边界，并在 R3a 记录新的先测后修澄清；不把阶段一空集门禁视为完整功能交付。

## 2026-10-23 视觉交互修正（当前优先）

用户在 #537 合并后确认需要录屏中的紧凑拖拽辅助层，而非已交付的 480×420 权限诊断大面板。新的[紧凑浮层修正规格](vega-issue-307-compact-guide.md)定义本轮验收与实现边界：系统设置是主界面，依附其底部的浮层只保留简短提示、Vega 正式应用图标拖拽与关闭；独立权限事实、重新检测和错误恢复放回 Vega 设置页。该文覆盖旧 R13/R15/R17 的内容分配和上一轮面板尺寸，跟随/冻结、真实运行 bundle、false/false、实际抓取未验证、无自动完成/撤卡/恢复任务等边界不变。

本轮基线 `d649b490ebdf831c13fb4d8a770eb2b77f1f4239` 已包含仓库最新快速交付裁决，不强制 TDD/RED/GREEN/删测链。下文历史轮次的测试先行要求与旧交付证据仅保留为历史，不约束本轮。原生与视觉效果仍待用户在修正构建手测，不以先前 CI 或静态审查替代。

## 最新范围裁决（优先于下文旧轮次条款）

用户在恢复阶段选择“本轮只做引导与权限状态”。依据锁定驱动的 [集成阻碍证据](vega-issue-307-phase2-driver-blocker.md)，本轮范围调整如下，旧条款仅在不冲突时继续适用：

- 保留跟随式悬浮卡片、真实 `.app` 拖拽、组件与两项 TCC 状态、分别打开系统设置、手动重新检测，以及运行结束后持续可见的准备入口；浏览器仍不在范围内。
- 所有准备检查固定 `prompt:false, probe_direct_capture:false`，复用同一嵌入式会话。点击权限动作只打开对应系统设置，不调用 prompt true，不新增原生权限或实际抓取桥接。取代旧 R7 与阶段一 UserInitiated 的集成用法；模型预检不变。
- 实际抓取显示“未验证”，不得把 Granted 静态开关等同于 desktop ready。R3a 的未检测/失败区分继续有效。真实直采就绪及更新后实际抓取验证延后，不纳入本轮 A2/A3 完成声明。
- 本轮不承诺全部就绪后自动完成或自动撤卡，取代旧 R18/R19 中自动撤除部分。权限状态更新后用户可关闭卡片；关闭不改变权限或能力开关。跟随与探测均在关闭时停止，拒绝过期回调。
- “返回任务”不等同于“已验证可操控”：回到原会话由用户重新发起任务，仍走既有目标授权、新观察和失败处理；不得自动恢复旧动作、复用旧观察。取代旧 R21 的 Ready 门槛与 A3 的完整准备承诺。
- 运行时互斥与会话回收、缺失/未知/忙碌分别呈现、错误可重试、R10–R17 的跟随/冻结/拖拽/键盘要求及 R20 持续入口继续有效。管理员验证框不得遮挡仍为手测要求，不因取消自动完成而删除。
- 本次是明确验收变化：重开阶段二 TEST-FIRST，先冻结缩小范围的断言并取得云端有效 RED，再实现。旧计划中的非提示直采、自动 Ready/撤除测试不再属于本轮，不以删测换绿；这些测试尚未编写或执行。

## 1. 问题与已确认根因

正式构建在 TCC 开关显示 on 的情况下仍可能在首次 `ComputerObserve` 预检失败，运行约 8 秒后中断，主界面只剩“已中断 / 含已拒绝操作”，错误文本埋在可折叠工具结果里，没有持续可见的下一步入口。

已确认的四条实现原因（来自卡片与代码核对）：

1. 设置页开启按钮只保存开关，检测只调 `ManagedComponents::detect`（`crates/vega_ui/src/settings/agents.rs:210`）；系统设置与扩展按钮是被动入口。
2. 首次连接先做权限预检，失败即终止（`crates/vega_runtime/src/computer_use/backend.rs:362-383`）。
3. 连接错误统一记为 `Rejected`；运行结束 `AppAgentController::finish` 移除 active run（`crates/vega/src/app_agent.rs:588-601`），`artifact.rs:367` 仅在 active 存在时喂 control，于是 `set_computer_use_control(None)` 清空 `computer_use_diagnostic`（`crates/vega_ui/src/conversation_stream/computer_use.rs:6-15`），提示整块消失。
4. 原规格 `docs/75/vega-issue-75-computer-browser-use.md` R2/R16/CU14 已要求真实状态与下一步，但实现与测试未覆盖“用户能完成准备并回到任务”。

本规格不改变 R16 的 `prompt=false` 连接预检语义，也不放开模型工具路径上的权限提示。

## 2. 范围

**本轮包含**：桌面侧准备状态模型、准备探测会话、悬浮引导卡片（含跟随系统设置窗口）、会话侧失败态保留与回到准备的入口。

**本轮不包含**：浏览器扩展的安装/连接引导（卡片内保留显式未评估状态，不得显示为就绪）；非 macOS 平台；自动授予任何 TCC 权限。

## 3. 事实依据（实现必须遵守）

F1. `cua-driver 0.34.0` 的 `check_permissions` 返回 `accessibility`、`screen_recording`（TCC 预检布尔）、`screen_recording_capturable`、`direct_capture_status`（`ready`/`unavailable`/`timed_out`/`probe_failed`/`blocked_by_screen_recording`/`not_checked`）、`source.attribution`（`driver-daemon`/`caller`/`host`）。

F2. `prompt:true` 只能由受信任宿主在模型工具路径之外发起；`prompt:false` 只读检查。Vega 现有连接预检使用 `{"prompt":false,"probe_direct_capture":false}`，本规格不改该调用。

F3. 责任进程身份由 daemon 启动参数决定：`serve --embedded --host-bundle-id ai.vega`（`backend.rs:735-750`）。因此准备探测必须复用与 `Desktop::spawn` 完全相同的启动参数，否则探测测到的不是运行时的身份。

F4. `cua-driver permissions status` 通过运行中的 daemon 回答，身份是 `com.trycua.driver`；独立调用报告的是调用方终端的授权。两者都不能替代 F3 的嵌入式身份探测。

F5. GPUI 0.3.4 已内建原生外部文件拖拽：`on_drag` + `external_drag_payload` → `ExternalDragPayload::Files(FileDragPaths)`；macOS 后端构造 `NSDraggingItem(pasteboardWriter: NSURL(fileURL…))` 并调用 `beginDraggingSessionWithItems:event:source:`，与 Finder 拖 .app 同一路径。提升为系统拖拽的触发条件是指针按住左键移出窗口 viewport。

F6. 拖拽结束回调只记录 `operation` 到 debug 日志，不向应用暴露“是否被接受”。因此卡片不得以“落点被接受”作为成功判据。

F7. `WindowKind::Floating` → `NSPanel` + `NSFloatingWindowLevel(3)`（`gpui-pre-macos/src/window.rs:949,1129`）。该层级本身不能证明“不遮挡管理员验证”，实际叠放必须手测；`WindowKind::PopUp` 硬编码 level 101（`:1159`），高于验证框，不得用于本卡片。GPUI 无窗口移动 API（`PlatformWindow` 只有 `bounds/resize/window_bounds/start_window_move`）、无隐藏红绿灯 API、无设置 `NSWindowCollectionBehavior` 的 API；`titlebar: None` 仍是 `NSTitledWindowMask` + full-size content view。

F11. 拖拽一旦开始，GPUI 会在指针移出 viewport 时把拖拽交给平台（`hand_active_drag_to_platform` 取走 `active_drag`，`gpui-pre/src/window.rs:5381-5405`、`app.rs:2550`），此后 `App::has_active_drag()` 为 false，且 `FileDropEvent::Ended` 不暴露给元素监听。因此“拖拽进行中”只能由 `on_drag` 的构造回调在发起瞬间置位，无法从 GPUI 状态可靠推断结束。

F12. 卡片自有的 `NSPanel` 无公开访问器：`Window::window_handle()` 只给 NSView 指针（需 `raw-window-handle` 直接依赖），`[NSApp windows]` 可按标题枚举。GPUI 创建时按 `TitlebarOptions.title` 设置标题，之后仍提供 `set_window_title`；本卡调用方必须维持唯一标记不被覆写；`setTitleVisibility:` 隐藏绘制但不影响 `title` 值。

F13. 移动与层级所需的 objc 调用（`setFrameOrigin:`、`setStyleMask:`、`setLevel:`、`setCollectionBehavior:`）在本仓已有零依赖先例：`crates/vega/src/window/image_preview/native/macos.rs` 用裸 `objc_msgSend` + `#[link(name = "objc")]` + `#[link(name = "AppKit", kind = "framework")]`，不引入任何 crate 依赖。

F8. GPUI 窗口坐标为顶左原点，`window_bounds.origin.y` 相对目标屏幕 frame 顶部；AppKit 为底左原点。互转必须用该屏幕 `frame.height` 与 `frame.origin.y`。

F8a. 本卡所有定位与跟随计算统一在顶左原点坐标系统内进行：`y` 向下增大，屏幕可见区 `visible.y` 为顶部、`visible.max_y()` 为底部。微信流那套 `y = settings.minY + inset - padding` 是 AppKit 底左原点写法，直接搬到 GPUI 会把卡片落到窗口右上角，禁止照抄。

F9. 系统设置进程 bundle id 为 `com.apple.systempreferences`；其窗口 bounds 与 owner PID 经 `CGWindowListCopyWindowInfo` 读取，不需要任何 TCC 权限。授权验证框是同进程的另一个 layer 0 窗口，因此必须按面积取最大窗口，不能按顺序取最前。

F10. 当前构建 `Identifier=ai.vega`、`Signature=adhoc`、`TeamIdentifier=not set`。adhoc 重新签名后旧授权记录可能不再匹配当前构建，这是常态。

## 4. 冻结需求

### 4.1 准备状态模型

R1. 准备状态由四个互相独立的事实组成，禁止合并为单一布尔或单一“就绪”：

- 组件：`Available` / `Missing` / `Incompatible` / `UnsupportedPlatform`
- 辅助功能：`Granted` / `Denied` / `Unknown`
- 屏幕录制：`Granted` / `Denied` / `Unknown`，并附带直采状态 `Ready` / `NotReady(reason)` / `NotChecked`
- 浏览器扩展：本轮固定 `NotEvaluated`（未评估），不得映射为就绪

R2. `Unknown` 只用于探测本身失败（传输、协议、超时）。探测失败不得写成 `Denied`，`Denied` 也不得写成 `Unknown`。

R3. 屏幕录制为 `Granted` 但直采状态不是 `Ready` 时，必须表达为独立状态“开关已开启但当前构建实际抓取未就绪”，并与“未授权”区分。这是本卡的核心判据，不得折叠。

R3a. 第二阶段验收澄清（实现前冻结）：`NotChecked` 表示尚未验证，不能声称实际抓取失败或直接要求重新授权；下一步为“重新检测”。`NotReady(reason)` 才是已探测但未就绪的事实，保留具体原因和恢复动作。二者均不得显示就绪。此前阶段一未覆盖该区分，本次重新先测后修，不以阶段一 GREEN 追认。

R4. 目标授权（本次窗口/标签）不进入准备状态，继续由既有 `ComputerUseControl` 生命周期表达。

### 4.2 准备探测会话

R5. 新增宿主侧准备探测能力，独立于任何 `ComputerUseRun`，只读且有界：创建时按 F3 参数拉起 daemon 与 MCP 客户端，只调用 `check_permissions`，结束时回收进程与私有目录。

R6. 探测会话在卡片打开期间存活，重复检测复用同一会话，不得每次检测重启 daemon。

R7. 探测的 `prompt` 由调用方显式决定：用户点击“打开系统设置/请求权限”时可为 `true`，其余一律 `false`。模型工具路径不得获得 `prompt:true` 的能力：不新增 MCP 工具、不通过工具参数传递该开关。

R8. 已有运行持有电脑操控时，探测会话必须拒绝创建并报告“任务运行中”，不得并行拉起第二个 daemon。

R9. 探测失败必须产生 R2 的 `Unknown` 并保留可重试入口，不得静默降级为“就绪”。

### 4.3 悬浮引导卡片

R10. 卡片为独立悬浮窗口（`WindowKind::Floating`，level 3），不可调整大小、不可最小化、不可由用户拖动窗口（`is_movable: false`），红绿灯通过 `traffic_light_position` 移出可视区，标题栏透明。

R10a. 卡片创建后立即应用三项面板修正，全部集中在同一个 macOS-only FFI 模块（F13）：置 `NSWindowStyleMaskNonactivatingPanel` 位，使点击卡片不夺走系统设置的 key window；确认 `level == 3`；置 `canJoinAllSpaces | fullScreenAuxiliary`，使系统设置位于其他 Space 时卡片仍可见。不得新增白名单外 crate 依赖。

R10b. 卡片窗口按 F12 的唯一标题标记识别。识别失败时不得 panic、不得移动任意窗口：跟随降级为“只在打开时定位一次”，卡片其余功能不受影响，并在诊断中记录一次降级原因。

R11. 卡片位置：读取系统设置窗口 frame（F8/F9），落在其右下角内侧，留 18pt 内缩，并 clamp 到该屏 `visibleFrame`。系统设置尚未出现时按固定节奏重试，超过上限后落到主屏右下角。

R12. 定位计算必须是可独立验证的纯函数：输入为设置窗口 frame（可缺失）、卡片尺寸、屏幕可见 frame，输出卡片原点。坐标约定按 F8a（顶左原点）：

- 设置窗口存在：`x = settings.max_x() - CARD_INSET + CARD_SHADOW_PADDING - card.width`，`y = settings.max_y() - CARD_INSET + CARD_SHADOW_PADDING - card.height`
- 设置窗口缺失：落在可见区右下角，`x = visible.max_x() + CARD_SHADOW_PADDING - card.width - 24`，`y = visible.max_y() + CARD_SHADOW_PADDING - card.height - 24`
- 两种情形均 clamp 到 `[visible.x - CARD_SHADOW_PADDING, visible.max_x() + CARD_SHADOW_PADDING - card.width]` 与 `[visible.y - CARD_SHADOW_PADDING, visible.max_y() + CARD_SHADOW_PADDING - card.height]`
- `CARD_SHADOW_PADDING = 24`（卡片阴影预留边距）、`CARD_INSET = 18`（距窗口右下角内缩）

不变量：卡片可见边界（原点加尺寸再减阴影边距）应落在 `settings.max_x() - CARD_INSET` 与 `settings.max_y() - CARD_INSET` 上，未被 clamp 时必须精确成立。

R12a. 跟随：卡片可见期间按约 150ms 的节奏重读系统设置窗口 frame，并在位置变化超过 1pt 时把卡片移到新位置，使用无动画的 `setFrameOrigin:`，避免逐帧动画叠加成漂移。不得以固定屏幕角落代替读取失败时的上一次有效位置；读到 frame 后即恢复跟随。

R12b. 拖拽冻结：从 `on_drag` 构造回调置位的那一刻起，本次卡片生命周期内停止跟随（F11）。理由是在用户手势中途移动窗口会让拖拽源从光标下移走。冻结只停止移动，不影响检测与状态迁移。

R12c. 跟随的决策必须是可独立验证的纯函数：输入为设置窗口 frame（可缺失）、卡片尺寸、屏幕可见 frame、卡片当前原点、冻结标志，输出 `Option` 新原点（`None` 表示不移动）。阈值、缺失回退与冻结都在该函数内判定，驱动循环不得另带位置逻辑。目标位置必须与 R12 的 `perch` 完全一致（单一位置约定），不得在跟随路径上另算相对偏移；仅当新目标与当前原点任一轴差超过 `FOLLOW_EPSILON = 1` 时才返回新原点。

R12d. 跟随不得阻止撤除：检测到就绪后的自动撤除优先于任何进行中的跟随校正。

R13. 卡片内提供一行可拖拽的 Vega 应用条目，载荷为应用 bundle 路径，`is_directory = true`，即系统认可的“把 .app 拖进权限列表”路径。

R14. bundle 路径无法解析（开发构建、非标准安装）时，该行不得伪装为可拖拽：隐藏拖拽源，改为静态说明与“在 Finder 中显示”入口。解析判据：可执行文件必须位于 `<bundle>.app/Contents/MacOS/vega` 三层结构内，`<bundle>.app` 目录名以 `.app` 结尾，且可执行文件名必须为 `vega`；任一条件不满足即视为无法解析。

R15. 卡片按 R1 的四项事实分别呈现状态与下一步，不得只显示一句“缺少权限”。必需动作必须可键盘到达并具有可访问名称。

R16. 卡片不得以拖拽落点被接受作为成功判据（F6）；只以探测结果变化作为状态迁移依据。

R17. 卡片提供“重新检测”；辅助功能与屏幕录制分别提供“打开系统设置”。屏幕录制项必须说明其生效条件与辅助功能不同。

R18. 全部必需项就绪后卡片显示完成态并自动撤除；用户可随时关闭。关闭卡片不改变任何已授予权限，也不改变能力开关。

R19. 卡片不得盖住系统随后弹出的授权验证框：卡片位置取右下角内侧、层级固定为 3（F7），且在探测到就绪后立即撤除。跟随取同进程 layer 0 候选中“面积最大”的窗口（F9），不得按窗口顺序取最前；这只是目标识别策略，不证明验证框的层级或可见性。R19 的实际叠放仍需用户手测，未验证前不得宣称已满足。

### 4.4 会话侧回到准备

R20. 因权限未就绪而中断的运行，其准备入口必须在运行结束后持续可见，直到用户处理或显式关闭；不得只留“已中断”与可折叠日志。

R21. 该入口提供“重新检测”“打开系统设置”，并在检测到就绪后允许回到原任务重新连接；重新连接后必须重新观察目标才能执行动作，不得复用旧观察。

R22. 传输/协议类失败与真实权限拒绝必须区分呈现，不得把所有异常写成缺少权限或用户拒绝。

## 5. 验收条件

A1. 全新用户在 Vega 内可完成桌面准备，不需要外部解释设置位置：卡片给出可拖拽条目与两条权限的具体下一步。

A2. 缺少辅助功能、缺少屏幕录制、两者都缺、开关已开但直采未就绪、探测失败、组件缺失，六种情形各自给出对应原因与可操作下一步，且互不混淆。

A3. 完成准备后回到原任务，经重新检测、目标授权与新观察开始执行；失败与等待期间无未授权动作。

A4. 拖拽条目在正式安装构建中可用，在开发构建中不伪装可用。

A5. 卡片在系统设置已打开与未打开两种时机下都能正确落位，不越出屏幕可见区域。

A7. 拖动系统设置窗口时卡片随之移动，位置与窗口右下角内侧保持固定相对关系；停止拖动后不出现持续漂移或抖动。

A8. 开始拖拽 Vega 条目后卡片不再移动，手势期间拖拽源不离开光标；拖放结束与授权后卡片按 R18 撤除。

A9. 窗口识别失败或系统设置被关闭时，卡片停在最后一次有效位置并保持可用，不越出屏幕、不移动其他窗口。

A6. 运行结束后的准备入口持续可见且可操作。

## 6. 自动回归边界

按仓库当前规则，Rust 测试在开发期先行、云端验证 RED/GREEN，最终交付树不保留测试。

可自动验证（进程内，无真实 TCC、无真实窗口）：

- 准备状态分类矩阵：`check_permissions` 结构化返回 → 四项事实，覆盖 A2 的六种情形与 `Unknown`/`Denied` 不可互换。
- 直采状态映射：`screen_recording=true` 且 `direct_capture_status != ready` 时不产生“就绪”。
- 定位纯函数：设置窗口存在/缺失、多屏、贴近屏幕边缘、clamp 行为。
- 跟随决策纯函数（R12c）：位移阈值内不动、超过阈值移动、设置窗口缺失时保持当前原点、冻结时不移动、设置窗口重新出现后恢复跟随、clamp 仍然生效。
- bundle 路径判定：标准安装、开发构建、路径缺失三类结果。
- 卡片状态迁移：检测结果 → 呈现的下一步集合；就绪后进入完成态。
- 会话侧入口保留：权限失败终止后入口仍可投影出来。

不可自动验证（交用户手测）：真实 TCC 授权、拖拽落进系统设置列表、卡片与系统设置的实际叠放、管理员验证框不被遮挡、更新后重新授权、实际跟随的平滑度与拖拽中冻结的手感、窗口识别在真实 `NSApp.windows` 上的命中。

## 7. 关键风险与已定处置

K1. `WindowKind::Floating` 不是 non-activating panel，点击卡片可能激活 Vega，使系统设置失去 key window，进而影响拖放落点与随后弹出的验证框。处置：由 R10a 在同一个 FFI 模块内设置 non-activating 位；该位是运行时 `setStyleMask:`，与微信流“创建时即带该位”不完全等价，若手测发现仍会夺焦，改为创建后重建 styleMask 并复测。

K2. 卡片可能遮挡权限列表底部。处置：右下角内侧、18pt 内缩、就绪即撤；不追求零遮挡。

K5. 跟随需要每约 150ms 读一次窗口列表并可能移动窗口，属高频副作用。处置：只在卡片可见且未冻结时运行；位移 1pt 阈值内不调用移动；卡片撤除时先停止循环。若手测发现明显 CPU 或抖动，允许把节奏放宽到 250ms，不得改成事件订阅以外的推测式平滑。

K6. `setStyleMask:` 在 macOS 上可能重置 first responder 或动画行为。处置：只在卡片创建后调用一次，且不改变 `contentView`；失败时保持 level 3 并记录降级，不回退到 level 101。

K3. 屏幕录制授权通常需要重启应用才生效，辅助功能不需要。处置：由 R17 在卡片内明示差异，不承诺立即生效。

K4. adhoc 签名导致更新后旧授权失效。处置：由 R3 与 A2 的“开关已开但直采未就绪”覆盖，文案说明可能需要针对当前构建重新授权，不把静态开关当有效证据。

## 8. 交付顺序

1. 本规格冻结（本次提交）。
2. 先行测试：R1–R3、R12、R14、R16、R20 的进程内断言，云端取得有效 RED。
3. 最小实现：准备状态模型与探测会话 → 定位与跟随决策纯函数、bundle 判定 → 卡片视图 → 面板 FFI（层级/非激活/移动/识别）与跟随驱动 → 会话侧入口。
4. 同一测试 GREEN，固定证据后删除 Rust 测试与独占支持。
5. 最新 head 空集门禁通过，转正式 PR。
6. 合并后交用户手测：真实拖放、卡片跟随与拖拽冻结、卡片叠放与验证框遮挡、更新后重新授权。

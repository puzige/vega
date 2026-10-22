# #552 Workspace 标签选中态轻量过渡先行测试规格

- 内容版本：`v1.2-test-first-recovery-ready`；父 Agent 已审并冻结 §2 视觉方案。单一恢复 writer 保留原 16 个测试，新增 6 个测试与 §5.6 授权的临时观察支持，W1–W12 不变。本轮仅先行测试与观测接线，尚未取得有效 RED，不授权生产实现。
- 原始验收：[Issue #552](https://github.com/puzige/vega/issues/552)，本轮读取正文的 `updatedAt = 2026-10-22T16:39:27Z`。
- 分支：`feat/552-workspace-motion`；当前源码 HEAD：`6615748f91273bfb8ee424011b3b5a09b3777d2b`。父 Agent 在恢复前已 fetch＋ff，HEAD/branch 和未提交清单均已核对；保留 #547/#546 基线，不从旧 ref 恢复源码。
- 写入前保留的本稿 SHA-256：`330f7f9690421530c4659d24f3fee58b62ab986b358926b7ef5e30493d95dd01`；#553 只读规格 SHA-256：`36327716822ab0c5921112e402f30a8526d3d0c4c9b6804652bb31245c2a4bf6`，均已核对。旧执行已停止，本轮只有本 writer；不修改 Git/index 或外部状态。
- 本轮范围在 §5.6 和恢复开工包内：`docs/552/`、Workspace 的 `cfg(test)` 接线/测试、vega dev features，以及默认关闭的 vega_ui/vega_conversation test-support。仅机械抽取 TerminalView 私有 constructor，复用原 30ms 轮询；无生产 token/helper/导航/背景/aria-selected 实现。本地不运行 Vega、构建、check、Clippy、测试、bench、profile 或 CUA，不改其他树及 #344/#535/#550。

## 1. 验收矩阵

矩阵保留 Issue 的全部边界。以下是未来测试/手测的预期，不是本轮运行结果；“同一事件内”指生产导航提交，不要求显示器零时延。

| ID | 前置与动作 | 必须可观察的结果 | 证据路线/状态 |
|---|---|---|---|
| W1 | 已挂载标签，鼠标切到另一 Terminal/Diff/文件 | 真实 selected、内容投影、点击命中、焦点及标签可见区请求立即更新；只有选中填色采样变化 | 生产 Workspace 事件＋焦点＋draw；待 Actions |
| W2 | Tab/Shift-Tab 定位标签，Enter/Space 激活；程序入口切换 | 与鼠标相同的真实终态与视觉策略；保留各入口既有焦点意图，不为动画强制统一到错误焦点 | 同一生产组件的键盘/action 路径；待 Actions |
| W3 | 快速 A→B→C、A→B→A，未满时长再次切换 | 从当前可见填色转向最新目标；不排队、不重播旧目标、不延迟导航 | 可控时钟下中间帧与最终帧；待 Actions |
| W4 | 初次挂载已有 selected；隐藏/失活后恢复 | 第一帧采用当前真实状态；恢复不补播旧切换、不出现全标签先未选中再动画 | 初始/恢复 draw；待 Actions |
| W5 | 选中/非选中标签关闭、增删、重排、右/底部移动 | 采用既有 fallback/停靠行为；动画身份不串到邻项；被删除对象不能被过期任务恢复 | 生产 close/move/open；实体与终态断言；待 Actions |
| W6 | 内容未就绪、加载失败、空 body | 选中语义、错误与恢复入口即时；不等动画完成才展示，不伪装内容已成功 | 真实 controller，隔离外部失败；待 Actions |
| W7 | 连切 Terminal、Diff、文件、其他现有标签类型 | 不复制/重建 session/editor；非选中 Terminal/Diff 不获输入；各内容既有滚动位置与生命周期保留 | 生产实体身份、路由/焦点和滚动；真实 PTY 交用户 |
| W8 | hover、pressed、focus-visible、浅/深主题 | 保留不同状态和必要对比度；文字/关闭按钮/焦点环/命中框不插值；不能仅靠运动表示选中 | quads＋事件＋用户外观；待验证 |
| W9 | 标签在可见区外，切换/关闭后需要定位 | `scroll_to_item` 在下一次正常 render 立即消费 reveal 请求，不等过渡结束；不新增滚动动画 | 生产 tab scroll 终态与时间顺序；待 Actions |
| W10 | Reduce Motion 已开启或过渡中开启；结束/隐藏/失活 | 直接终态，停止本装饰的帧请求；后台真实终端/流式更新不算装饰循环 | #549 生命周期＋本接线；待 Actions |
| W11 | 真实 Workspace 同时有终端输出和对话流式更新 | 输入/焦点、导航、阅读位置不退化；不以孤立 demo 或上游微基准宣称性能提升 | 进程内生产组合回归＋用户真实手测；未测性能 |
| W12 | 可访问名称与选中状态 | 暴露的选中语义、标签名称、动作与当前业务状态一致，不从视觉进度推导；键盘路径无幽灵目标 | 生产状态/语义投影；系统 AX 覆盖缺口显式记录 |

## 2. 范围、视觉决策与非目标

### 2.1 已冻结：只过渡已有选中填色

不同时尝试移动 marker 或新增第二套指示器。`render_workspace_pane` 现有选中 pill 填色作为唯一插值对象；标签文字颜色、图标、关闭按钮显隐、焦点环和真实 selected 继续即时变化。即使旧标签底色正在淡出，它也不再拥有选中语义或输入。

| 项 | 父 Agent 已选定契约 | 数值/依据性质 |
|---|---|---|
| 视觉值 | 每个现有标签的选中填色强度，未选中 `0`、选中 `1`；映射到当前主题现有端点 | 归一化视觉投影，不是业务状态或新增颜色 |
| 时长/曲线 | 120ms、Kit `Transition::new` 默认 `ease_out_cubic`、无 delay、无弹簧/过冲 | 父 Agent 已冻结的产品值；不是 Apple/Kit 官方导航值或实测收益 |
| 范围 | 背景填色，不对整个标签或 body 设置 opacity/transform | 保持文字、控件、焦点和几何稳定 |
| 初次挂载 | 直接采用当前 target | 不能先构造 0 再等下一帧“进入” |
| 重定向 | 同一 stable key，从当前采样值转向最新 target | 不排队；反向可遵循 Kit 缩短回程的语义，不要求每次恰满 120ms |
| 减少动态效果 | 即时终态；取消未完装饰调度 | 依赖 #549 统一生命周期接线 |

#549 唯一 writer 将提供 `Motion::TAB_SELECTION_MS = 120` 和 `decorative_transition(duration, visible, window, cx) -> Transition`；只有 `visible && active && !reduce_motion` 时 duration 非零。此 HEAD 尚无这两个生产接口，本轮不添加、不伪造可用性。测试按用户已冻结的 120ms 行为设定采样时间，不提供生产 fallback。不得擅自扩大 `INTERACTION_DURATION_MS` 的 hover 语义；`THINKING_SHIMMER_MS = 1800` 不用于导航。

主题端点采用现有 `colors.bg_active_alpha` 与透明背景，不硬编码 RGB，不降低文本对比度来“变柔和”。hover/pressed/focus-visible 的现有优先级保留；动效采样只提供基础选中背景，不能覆盖即时交互状态。若 `InteractionExt` 或现有 hover 覆盖影响选中辨识，先在首轮测试/主题样本中暴露并由父 Agent 冻结取舍，不偷偷改整体状态语言。

### 2.2 明确禁止

- 不对 Terminal、Diff、文件正文、对话、整页或整张 pane crossfade/滑动/缩放。
- 不复制或重建终端会话、编辑器、DiffView、Artifact 等实体；引用现有 entity 不等于创建替身树。
- 不动画化 tab strip 或面板宽高，不延迟滚动定位、键盘响应、焦点和内容切换。
- 不以视觉 progress 反写 selected，不通过 completion callback 提交导航。
- 不新增动画引擎、依赖升级、常驻说明/成功提示，不调整业务权限或持久化。
- 不改 #549 的共享 Motion/lifecycle 或窄 Environment 浮层，不接管 #344/#535。

## 3. 状态、身份及生命周期

### 3.1 提交顺序

```text
真实输入/程序入口
  → 现有 workspace.open / close / move 等业务动作
  → 现有 workspace_activate_tab / workspace_focus 与 reveal 请求
  → 下一次正常 render 按真实 selected 选择 body、语义和命中
  → 仅已有标签背景向该 selected 的目标采样
```

没有“动画中暂时 selected”或 pending tab 队列。重复激活同一目标不重启动效；后台数据 notify 不生成新导航事件。内容失败不会取消真实选择或恢复上一份 body。

### 3.2 稳定身份

使用现有 `TabKey` 身份，加所在 pane 的稳定身份与属性命名空间作为动效 key；不使用循环下标、标题文本、每 render 的随机值/计数或 selected 值本身作为整条动画的 key。右/底部迁移使所在视觉容器变化时按新容器当前状态呈现，不跨容器飞行，不同时挂两份 body。原有 `workspace-tab-{key:?}` 和 `tab_focuses` 不能因视觉采样重建。

关闭/删除后不持有旧 view、session 或 callback 等待动画。重排中同一个 TabKey 的状态仍属于该对象；新对象首帧终态。装饰状态只保存必要的数值/键，不能延长业务实体生命周期。

### 3.3 共享生命周期的消费约束

#549 必须明确可见性、窗口 active、Reduce Motion 及恢复时状态同步的接口，本卡不复制实现。结束、隐藏或失活后不继续请求装饰帧；恢复按此时 selected 同步，无积压重播。关闭不使用 `Presence` 延迟 body 卸载，也不等待退出动画回焦。

独立的终端输出、流式消息、hover 等可能仍请求绘制；“无装饰循环”须针对本动画来源验证，不能断言整个应用绝对无帧。若现有 harness 尚不能区分请求来源，记录观测缺口并在先行测试阶段解决，不削弱终止断言。

## 4. 当前源码事实与具体符号计划

路径相对仓库根；这些是未来实施包，不是本轮写入清单。

| 文件/符号（当前行号） | 已核事实 | 未来最小动作/保留项 |
|---|---|---|
| `crates/vega/src/window/workspace.rs::render_workspace_pane`（1119 起，1193–1235） | 稳定元素 ID、`tab_focuses`、aria label；Enter/Space 与鼠标都执行 open→activate→notify；选中背景直接切换 | 仅背景采样接入 #549 token/lifecycle；事件、focus、尺寸与语义不依赖 progress |
| 同函数（1136–1143） | `reveal_tabs` 按真实 selected 找位置并 `tab_scroll.scroll_to_item` | 保留原位置/时序；不挂到动画结束回调 |
| 同函数 body match（1324 起） | 按 selected 引用现有 view；Terminal/Diff 等走各自 controller | 不包双树、不跨淡入；加载/失败/无 body 都即时 |
| `workspace_focus / workspace_activate_tab`（844–901） | 按 TabKey 选 focus，缺失时回 composer；Diff 有显式 focus intent | 只作回归观察，通常不需修改；不得借动画重写焦点规则 |
| `workspace_close_tab / workspace_move_selected / workspace_hide`（645/684/707 起） | 现有关闭、停靠、隐藏入口 | 验证清理/迁移不继承过期背景，不改其业务决策 |
| `vega_theme::Motion`、共享 lifecycle | 当前仅 120ms interaction / 1800ms shimmer；新共享契约由 #549 写 | 本卡只消费，不并行改共享文件 |

实施顺序应为：父 Agent 冻结本草案和 #549 接口 → 先写齐 §1/§5 测试及必要接线 → 独立审查/Actions 有效 RED → 背景局部接线 → 同一冻结测试 GREEN。若暴露原有焦点/选中语义缺陷，单独记录验收/范围变化，不能在动画实现后补测追认，也不能为通过放弃该回归。

**语义证据限制**：当前读到 `aria_label`、selected 数据和视觉分支，不能据此宣称平台 AX 已提供完整 selected 属性。未来须核对实际锁定 GPUI 可暴露的选中语义及生产投影；缺少接口时先报告/冻结最小补齐范围，不能把进度值当可访问状态，更不能用 macOS AX 查询失败推断导航正确。

## 5. 先行测试与阳性对照

### 5.1 生产组件测试，不用 toy 决策或源码 regex

在 `TestAppContext / VisualTestContext` 中挂真实 Workspace 根/生产 controller，保留实际 `workspace.open`、事件 handler、body match 和 focus 路径。通过框架事件/动作输入，不只直接改字段再测一个自制颜色函数。网络、文件失败或 PTY 只在外部依赖边界隔离；不能 mock 掉导航/焦点/绘制决策，不启动真实 shell/PTY 的自动 E2E。

- 初始 draw 确认 bounds/语义/真实内容；派发鼠标或键盘动作，在未推进动画时钟时断言新 selected、body、focus 和 scroll 已生效。
- `executor.advance_clock` 后由 Kit `TestWindowExt::render_frame` 完成真实根的一帧，再采样至少一个中间时刻和完成时刻；快速重定向还要验证旧目标不回写。不按目标帧率断言恰有固定帧数。`draw/render_frame` 不是 OS present 或显示器实测。
- `Window::painted_quads()` 观察真实 tab 背景 bounds/颜色；quads 使用缩放像素，需用实际 `scale_factor()` 与元素逻辑 bounds 对齐，纯色用 `Background::as_solid()`。不是所有 quads 都带测试 selector，按生产元素真实 bounds 关联，并用阳性对照排除误读背景/hover quad。
- 焦点用真实 `FocusHandle::contains_focused`、`window.focus_next` 及输入路由结果；不能靠截图、CGEvent、AppleScript 或 AX 只返回 Window 的结果作断言。
- Terminal/Diff 断言实体/会话身份未变化、非选中对象不收输入及恢复滚动位置。外部 session recorder 只是边界证据，不能冒充内核 PTY 或真实输出性能。

### 5.2 实现前必须有效的阳性对照

| 观察对象 | 阳性对照与负对照 | 能排除的观测故障 |
|---|---|---|
| 输入/焦点 | 真实事件激活已存在标签，选中内容的 handle 确实聚焦；非选中对象不收输入 | 事件未接到根、测试只改影子状态、focus fixture 不工作 |
| 绘制 | 无动画基线的两种 selected 状态对应不同真实填色；同一主题下颜色/bounds 可定位 | quads 选错、未 draw、逻辑/物理像素混用 |
| clock/重定向 | 未来接入后的真实背景在中间采样确有不同值，最终准确；Reduce Motion 为即时终态 | `advance_clock` 未连接锁定 motion 时钟、始终只看到终态 |
| 生命周期 | 可见导航发生有限帧请求，结束/隐藏/失活后本装饰来源停止；后台生产事件仍可生效 | “永远为零”的假停帧、错误禁掉真实内容更新 |
| 身份/重排 | 真实关闭/移动/重排后目标实体与内容仍对应正确 TabKey | 下标复用导致视觉/输入串对象 |

当前已有 API 路线不等于此 HEAD 有可直接运行的测试；历史 Rust 测试已清零，不整目录恢复。clock 与 motion clock 的接线、frame-request 可观测性、生产 root 初始化和外部终端隔离是先行测试阶段的明确检查点。若对照失败，按 harness/接线故障修正并留证，不算产品 RED；不能以只搜 `.transition` 字符串或复制采样算法替代。

### 5.3 云端与真实环境分工

- 未来由父 Agent 在现有 draft PR 触发 `.github/workflows/pr-check.yml`：`macos-latest` Clippy job 执行 fmt 与 `cargo clippy --workspace --all-targets -- -D warnings`；Nextest job 保留 Python 检查并用 `cargo nextest list/run --workspace`，空集才使用 `--no-tests=pass`。required check 为 `check (fmt, clippy, test)`。
- 有效 RED 必须来自本卡缺失的可观察过渡行为或明确最小接口缺失；原本即时语义等保全回归可先为绿，不能强造失败。网络/runner/无关编译故障不算 RED。
- 生产未改时写齐测试 → 独立审查 → Actions RED → 冻结 → 最小实现 → 同一测试 GREEN → PR 固定完整 SHA/测试源码/patch/run/job → 独立删 Rust 测试及独占接线 → 最新 head 空集完整门禁。`no Rust tests` 不是带测试 GREEN；不删除失败/未执行回归求通过。
- 本轮新增开发期测试，但本地不执行；**有效 RED/GREEN 均未取得，待父 Agent 独审后使用现有 draft PR/Actions**。阳性对照与完整性缺口未解决前，不得把无关编译/接线/环境失败认作 RED；不另建 CI/测量设施。
- 真实 Workspace 的终端输出＋流式对话并发场景由用户用核实的云端构建手测；记录构建、OS、屏幕/窗口、主题、Reduce Motion、数据/操作序列及结果。实际 present/PTY 性能需许可环境，不能把 headless 时钟、quads 或上游 sampler benchmark 写成收益证据。

### 5.4 首次部分包的测试入口与映射（历史，当前见 §5.7）

新文件为 `crates/vega/src/window/workspace/motion_tests.rs`，共 16 个 GPUI 测试属性，全部新写，未恢复历史 Rust 测试。`WorkspaceRoot` 只委托真实 `VegaWindow::render_workspace`；后者继续调用生产 pane、body match、事件和 focus。测试构造真实 `VegaWindow` 的惰性初始状态而不调用 `VegaWindow::new`，因为后者会启动 updater；没有复制标签选中、过渡或导航算法。`BodyObserver` 是无 ID/无额外布局的转发 Element，只记录实际 paint 的 EntityId，不选择 body。tab 的 Kit observer 读取真实原生属性，不注入 selected。

以下映射不是“全部覆盖/已运行”的结论；完整包受 §5.5 阻塞。所有测试均待独审和 CI，执行/通过/失败计数未取得。

| 验收 | 已写行为测试（函数名片段） | 未闭合部分，不能删掉 |
|---|---|---|
| W1 | `real_mouse_handler_body_focus_and_painted_background_are_observable`；`selection_commits_before_only_the_background_eases_for_one_hundred_twenty_millis` | 阳性对照待 CI；Terminal 路径缺边界隔离；正文/文字无整体 opacity 的独立画面观察尚不足 |
| W2 | `keyboard_tab_shift_tab_enter_space_and_programmatic_open_keep_real_focus_intent` | 真实文件 Enter/Space、程序入口已写；composer/Terminal/Diff 各自焦点意图及输入仍需组合回归 |
| W3 | `rapid_reversal_starts_at_the_current_painted_sample_and_never_replays_old_selection`；`rapid_third_target_cancels_the_old_target_without_queuing_navigation` | 当前采样/终态待 CI，不用自造 transition 算法 |
| W4 | `initial_mount_and_repeated_activation_are_already_at_the_real_target`；`hiding_and_restoring_adopts_current_selection_without_replaying_the_old_transition`；`visible_motion_requests_frames_but_completion_and_window_inactivity_stop_them` | 隐藏/失活恢复已写；后台真实数据持续更新的恢复仍属 W11 缺口 |
| W5 | `closing_selected_or_inactive_tabs_uses_real_fallback_and_cannot_resurrect_deleted_bodies`；`reordering_preserves_tab_key_samples_and_new_tabs_mount_at_their_current_target`；`docking_mounts_one_existing_body_in_the_new_pane_without_inheriting_another_panes_sample` | 关闭走真实 close 控件；停靠走真实 controller；当前没有生产重排入口，测试交换 tabs 仅模拟顺序输入，不冒充 handler 验证 |
| W6 | `diff_loading_failure_retry_and_missing_body_never_wait_for_the_decorative_clock` | 真实 Diff controller/view、refresh 恢复 handler 和空 body 已写；loading/error 文字本身没有只读观察入口，不能据 EntityId 断言文案已显示 |
| W7 | body 实体 observer、`docking_...` 与文件焦点负对照 | **未完成**：Terminal session/input/output 隔离、Terminal/Diff 非选中输入投递、文件/Diff/Terminal 内容滚动恢复、其余现有标签类型 |
| W8 | `hover_and_pressed_feedback_remain_immediate_and_do_not_change_selection_or_geometry`；浅深两套 quads 阳性对照 | 保持 tab 现有按下前不提交、hover 优先；focus-visible 边框、关闭按钮显隐及字形不插值的独立观察尚未齐；用户仍须做对比度外观手测 |
| W9 | `offscreen_tab_reveal_is_consumed_on_the_first_render_not_animation_completion` | 真实 scroll offset/bounds、关闭 fallback 和 reveal 消费已写；待 CI |
| W10 | `reduced_motion_at_start_or_mid_transition_adopts_the_latest_target_and_stops_frames`；`visible_motion_requests_frames_but_completion_and_window_inactivity_stop_them`；隐藏用例 | 隔离静态内容时的有限 frame callback 断言已写；并发真实更新下的装饰来源归因未完成 |
| W11 | 无合格可执行组合测试 | **未完成，不以文档或孤立文件 demo 替代**；终端输出＋流式消息＋输入/阅读位置的真实生产组合需要 §5.5 的边界支持 |
| W12 | `native_tab_names_and_selected_semantics_follow_business_selection_not_painted_progress` | 原生 label/selected 断言已写；当前 tab 只有 aria_label，尚未设置 aria_selected；须父 Agent 判定原有语义缺口和最小范围，不能把该失败认作 motion RED |

60ms 数值来自已冻结曲线的独立验算：`t = 60/120 = 0.5`，`ease_out_cubic(t) = 1 - (1 - 0.5)^3 = 0.875`；新标签强度为 `0.875`，旧标签为 `1 - 0.875 = 0.125`。容差为归一化强度 `0.005`，不是像素或 present 误差。快速反向不重复计算 Kit 算法，只要求同一时刻连续、方向正确且最终准确。

### 5.5 首次部分包的阻塞诊断（历史，当前见 §5.7）

**状态：部分先行测试补丁可审，不是完整 TEST-FIRST READY；无有效 RED，不得进入生产实现。**

1. **真实 app 构造未执行**：静态确认 `window/mod.rs::new` 会 `start_updater`，后者启动 OS worker；当前测试只构造真实 owner 的惰性初态，并走完整生产 Workspace 根，不挂完整应用壳。第一个阳性对照必须在同补丁现有 Actions 验证事件、FocusHandle、body EntityId 与真实背景；若失败是接线/观察故障，不是 RED。本地未执行，不能声称已证明可观测。
2. **终端边界超出本轮写入权限**：`vega_ui/src/terminal.rs::for_target` 无可注入 session 构造，直接调用 `TerminalSession::start_target`；`vega_conversation/src/terminal.rs::start_target` 启动 worker，`run` 打开 PTY/启动 shell。session/snapshot/send 和 scroll 状态私有。缺失目录只会制造失败终端，不能验证输出或输入隔离；不采用它替代 W7/W11。最小下一步是父 Agent 分配串行的 **dev/test-only 外部 session 边界**，允许真实 `TerminalView` 消费可控快照和输入 recorder，配合生产 `ConversationStream` 接收端；不改变业务选择/渲染决策、不运行外部进程/网络。本 writer 不写这些 crate。
3. **只读内容/滚动观测缺口**：FilePreview 的 ScrollHandle、Diff 的 UniformListScrollHandle/错误显示、Terminal 的快照与输入结果都没有供 vega crate 读取的边界。需要在其拥有者分配最窄的测试/临时观察接线；不能用本地 toy renderer 或保存 EntityId 代替恢复滚动、输入投递和可见错误。W6/W7/W8 保留原判据。
4. **帧归因缺口**：`Window::simulate_next_frame` 返回全窗口 callback 数。静态文件场景可以写有帧/终止的阳性/负对照；终端、流式消息、hover 等并发时无法仅凭总数辨认本装饰来源。需要 #549 唯一 writer 与父 Agent 冻结不修改业务的观测接线；不把全窗口绝对零帧作为 W11 断言。
5. **语义范围判定**：锁定 GPUI 支持 `aria_selected`，Kit snapshot 支持 `selected()`；此 HEAD 的生产 tab 未设置该属性。保留 W12 测试，由父 Agent 对原有语义缺口单独归因/批准最小补齐范围；本轮不写 production aria-selected，也不伪造 observer 的 selected。
6. **独审/云端状态**：当前无 commit/push/PR/Issue 写入或 CI。父 Agent 应先审本补丁和缺口，补齐授权内真实边界并冻结完整测试，再触发现有 draft PR/Actions。首次阳性对照不通过时先修 harness；只能把目标过渡行为缺失或明确目标接口缺失作为有效 RED。未来 #549 token/helper 尚不存在，但当前测试不导入缺失符号来遮住行为断言。

### 5.6 主 Agent 缺口诊断与测试范围续接（尚未生产实现）

父审确认本轮部分包的 tracked diff SHA-256 为 `bfd93636964e7a46e62e8c5e1d0101fb6837266882214fa30c11d460affdca8d`、测试文件为 `825c1af0b3f6853a09b56ae662411995d64bfb2e91526648f4c04f8091172f1c`。原 writer 已停写后，父 Agent 将本树从 `2ba904f7074115de5ee89cc18c376c083f803357` 快进至 `6615748f91273bfb8ee424011b3b5a09b3777d2b`；tracked diff 哈希未变。保留 #547 的图片/初始输入和 #546 Composer 胶囊样式，不从旧 ref 恢复整文件。#553 的外部 hash 变化来自父 Agent 在其独立树的文档修订，不是本卡缺陷。

原始 Issue 已要求可访问选中语义、非选中输入隔离、滚动及终端/流式组合；没有新增产品需求。以下是完整先行测试冻结前的测试范围澄清，原 W1–W12 不变：

1. **临时跨 crate 支持**：允许在本卡 worktree 增加一个默认关闭的开发期 `test-support` feature，从 vega 的 dev dependency 启用 vega_ui，必要时传递至 vega_conversation；不升级或新增第三方依赖，不扩展普通构建 public API，GREEN 留证后随测试独立删除。生产导航、session、输入、布局、渲染和保存决策保持原样。
2. **只替换外部会话边界**：`TerminalSession` 的 `Input/Resize/Scroll` 已是有界命令队列，`State` 用 vt100 parser 生成真实快照。测试支持可提供无 worker/PTY/shell 的 session＋driver，消费真实命令并记录输入、注入合成输出、委托真实 parser/快照；不能伪造选中或焦点决策。真实 `TerminalView` 通过测试期 session 构造继续使用原 send/key/paste/scroll/resize/render 和定时快照消费路径；不能复制终端 UI 或把读取快照字段冒充实际绘制。真实 PTY/OS 集成仍仅用户手测。
3. **只读内容与绘制观察**：允许在 FilePreview、DiffView、TerminalView 和 ConversationStream 的拥有者添加 feature-gated 只读 scroll/content/paint 观测，或以真实绘制边界透明 wrapper 观察必要的 bounds/alpha/文本/身份。不得从旁路 helper 计算预期结果，也不得在 observer 中决定正文/错误/selected。W6 必须结合实际错误呈现和真实恢复操作；W7 必须通过生产输入入口核对非选中对象无输入，并检查原 scroll 恢复；W11 用生产 ConversationStream 事件与真实终端控件组合，不启动网络或外部进程。
4. **调度证据分层**：静态内容中的全窗口 callback 阳性与停止对照继续保留。组合场景不能要求全应用零帧；同时验证真实输出/消息仍被消费、输入与阅读位置正确，并在本卡实际视觉 sampler 返回的 `MotionStatus` 处留下只读测试期按 pane/TabKey 归属记录。该状态必须来自真正用于填色的采样，不新增钟表/调度器、不由 observer 驱动帧；与锁定 Kit `transition_with_status` 仅在 Delayed/Running 请求下一帧的源证据一起审查。首次生产实现前需完整写出目标断言；不能等待实现后再补。
5. **已有语义缺口单独归因**：`aria_selected` 缺失属于原 W12 的可访问性缺口，不是 motion 性能结论；保留 Some(true/false) 原断言，后续最小实现允许从同一即时业务 `active` 设置真实属性，不从 progress 推导。观察器不可伪造。该缺口可独立成为 W12 的目标 RED，但不能替代其余行为/阳性对照。
6. **父审先行问题**：现有 BodyObserver 只证明真实 entity 进入 paint，不证明 glyph/正文/错误全 opacity；需补真实绘制观察。重排当前直接 swap 仅是生产投影更新输入，不能称已验证不存在的拖拽操作。inactive 测试目前先 draw 再恢复，须补“失活期间无 draw、迅速恢复”的原生命周期硬边界，避免 render-only gate 假通过。每个新增观察口先有真实阳性/反例对照；不删除已有失败/未执行断言。

下一步复用原 writer，限本卡测试/外部边界支持补齐并停写，父 Agent 独立复审完整 W1–W12 与生产差异后再触发现有 Actions。当前没有有效 RED/GREEN，未进入生产实现；若上述最窄 feature 支持仍不可行，交回具体 API/证据，不降验收、不自建执行设施。

### 5.7 恢复完成的先行测试映射与证据边界

恢复 writer 开工核对原 17 文件（不含 checkpoint）清单 SHA-256 `14d48972284a84f1fe6936cbbe14edfb47f74f69e3e1fd61d769070d2f88212f`，四个关键 hash 均匹配。原 16 个 GPUI 测试未删除、未 ignore、未放宽；当前源文件有 22 个测试属性。该数量仅为静态属性清点，不是 Actions 发现或执行数量。§5.4–5.5 为首次部分包历史；首次部分 handoff 已由当前交接取代，旧范围/hash 不再代表当前内容版本。

| 验收 | 当前测试入口（函数名片段）及可观察断言 | 验证状态/边界 |
|---|---|---|
| W1 | 原 `real_mouse_handler...` / `selection_commits...`；文件断言增加实际可见正文；`repeated_terminal_diff_file_switches...` 验证真实 Terminal/Diff/文件点击、焦点与阅读投影 | 阳性对照及 motion 目标断言待 Actions |
| W2 | 原 `keyboard_tab_shift_tab_enter_space...` 保留；组合场景验证真实 Terminal key/Paste、Diff action 与 Composer focus/input | 不统一生产焦点意图；待 Actions |
| W3 | 原 `rapid_reversal...` / `rapid_third_target...` 保留 120ms default ease_out_cubic 的真实 quads 连续性/最终目标断言 | 不复制 sampler 算法；待 Actions |
| W4 | 原 `initial_mount...` / `hiding_and_restoring...`；新增 `inactivity_without_a_draw_then_quick_restore...`，用 RenderCount 明确无中间 render；组合恢复真实输出 | 首帧真实状态与不补播待 Actions |
| W5 | 原 close/reorder/docking 用例全部保留；新增 session close、File/Diff 弱实体释放与 stale-work 不保活 | reorder 仅输入生产投影的新顺序，不声称存在拖拽 handler；待 Actions |
| W6 | 原 `diff_loading_failure_retry_and_missing_body...` 增加实际 loading/error/空态正文与继承 opacity、键盘 Retry 原断言、真实 Retry 点击和快照恢复；关闭 controller 后空 body 不等动画 | 输入 controller 的快照/失败是外部 Git 边界，不替代真实 render/恢复 handler；待 Actions |
| W7 | `terminal_driver_uses_real_parser_poll_paste_input_scroll_and_close`；`repeated_terminal_diff_file_switches_preserve_reading_entities_and_isolate_input`：真实 terminal parser/30ms poll/paint/key/bracketed paste/resize/scroll；非选中 Terminal 和 Diff 输入隔离、File/Diff/Terminal 实际可见内容与各自滚动恢复、Subagents 类型切换、实体/会话身份和 close 生命周期 | 无 PTY/worker/shell；真实系统集成交用户，进程内断言待 Actions |
| W8 | 原 hover/pressed 用例保留；`actual_text_paint_probe_detects_parent_opacity_and_hidden_content`；`text_close_and_focus_geometry_are_immediate_while_only_fill_interpolates`：浅深主题即时文字颜色、实际父 opacity、关闭按钮内部 paint bounds/opacity、focus border quads 在 0/30/60/120ms 不变 | glyph/SVG 无公开 scene getter；通过实际绘制边界与同框架 opacity 路径观测，不声称像素截图/对比度通过；待 Actions/用户外观 |
| W9 | 原 `offscreen_tab_reveal...` 保留，首 render 消费 reveal、实际 offset/bounds 与关闭 fallback | 待 Actions |
| W10 | 原 reduce/visible/hidden frame 有帧/停止对照保留；新增无 inactive draw 快恢复；组合 complete/reduce/hide/inactive 场景读取 pane/key 归属的真实 sampler status | 当前没有生产 sampler，因此 status=None 必须明确失败；observer 不填目标、不调度帧；待 Actions |
| W11 | `live_workspace_terminal_and_stream_keep_input_reading_and_business_updates_when_decoration_stops`：真实 Workspace + TerminalView + ConversationStream::apply_event；实际输出和正文绘制，业务在装饰停止前后仍消费，导航/输入/Composer 草稿与回焦、阅读锚点不强制跟尾、恢复后终端正文正确，ResumeTail 后最终对话正文真实 paint，原 entity 保留 | 不断言全应用零帧；状态来自未来实际填色 sampler；真实 PTY/present/性能数据未取得；待 Actions/用户手测 |
| W12 | 原 `native_tab_names_and_selected_semantics...` 的 label 和 Some(true/false) 断言原样保留 | HEAD 生产未设 aria_selected，属原验收语义缺陷；None 不是 pass；单独归因，待 Actions |

**PaintLog 的证据含义**：SharedString 的实际 Element paint 与原 selectable text 的转发 paint 后，在同一边界提交 1px、alpha=`1/65536` 的测试专用 quad；从本帧 `Window::painted_quads()` 唯一识别实际 alpha，再归一化为继承 opacity。颜色标识不对应预期 opacity，不读取 crate-private 状态。探针会在 test-support 构建中增加微小 quad（并非零写入光栅），不改布局/命中/调度或普通构建；不是显示器 glyph 像素捕获。父 opacity=0.25 阳性对照必须证明 style alpha=1 仍可检测淡化，恢复后=1；隐藏正文必须不出现记录。Terminal 只在原 line.paint 返回 Ok 时记录，身份从公开 Window::current_view() 取得。关闭按钮探针位于真实按钮内部，避免外 wrapper 错过其内部 opacity。对话叶子通过 feature-gated SelectionDocumentBuilder 转发模块观察，原 selection.rs/算法未复制或修改。

**调度分层**：静态窗口保留 `simulate_next_frame` 阳性/停止负对照。组合场景的 SelectionSamples 每帧先将实际 pane/key 清为 None，只允许未来实际用于填色的 sampler 调用 `observe_selection_sample` 写入它返回的 MotionStatus；测试没有调用该记录函数、没有旁路 sampler 或状态合成。未来接线必须在同一填色采样处完成；锁定 Kit `transition_with_status` 只在 Delayed/Running 调用 request_animation_frame 的源码证据仍适用。隐藏 pane 无 sampler、业务仍更新；完成/Reduce/失活要求 Idle/Finished 并结合背景终态。未取得执行前，不声称观察口阳性对照已通过。

**阶段结论**：测试/支持源码已补齐至可独审 READY，生产 motion 尚未开始；没有有效 RED/GREEN。下一步唯一责任链为父 Agent 组织非写入者审查该停写内容版本，再使用现有 draft PR/Actions 验证。编译/观察/夹具失败先归 harness，不作为产品 RED，不删 W1–W12。#549 共享 token/helper 仍由唯一 writer 处理；本轮不引入缺失共享导入掩盖行为断言。GREEN 留证后独立删除测试/独占 feature/支持，再走最终空集门禁。

## 6. 未决、实施门槛与回滚

| 未决/门槛 | 本草案处理 | 下一步 |
|---|---|---|
| #549 token/可见性/active/reduced-motion 接口尚未落地 | 只定义消费契约，不写共享文件；没有私有 fallback 动画引擎 | 父 Agent 与唯一 writer 冻结接口后同步基线 |
| 120ms/ease_out_cubic 与主题状态组合 | 父 Agent 已冻结；保留 hover/pressed/focus 优先级 | 同补丁阳性对照和行为断言待云端；用户外观验证，不改正文/焦点 |
| 可访问 selected 与 frame-request 观察 | 不把 aria label/quads 等同完整 AX/调度证据 | 先行测试阶段核对实际 API、必要对照；缺口报告 |
| 真终端并发与显示器呈现数据 | 本地不可运行；未取得 | 用户手测或明确授权的外部环境，保留 W11 |
| #344/#535 或基线变化 | 本轮不改其范围 | 未来实施前审最新增量；不同缺陷单独归因，不搭车 |

若后续实施后回滚，只去掉本卡局部背景采样并恢复已有即时填色；不回滚导航、focus、reveal、实体身份或 #549 的其他消费者。本轮没有生产实现；测试与其独占接线须在同一冻结测试云端 GREEN、证据固定后独立删除，不能提前删失败/未执行测试求空集。所有 commit/push/PR/Issue 状态由父 Agent 统一处理；本 Agent 不嵌套委派，交接后停止写入。

## 7. 依据与当前交付

- [Apple HIG Motion](https://developer.apple.com/design/human-interface-guidelines/motion)：沿用原草稿记录的依据，动效服务反馈、不能是唯一状态线索；不规定本卡 120ms。本轮未重新网络读取该网页。
- [Kit Motion 固定来源](https://github.com/longbridge/gpui-kit/blob/96103905ea0c9c199db206ada7a9b2e3114e6339/website/base/motion.md)：沿用原草稿来源；本轮静态读取锁定 registry `gpui-base 0.6.1/src/motion.rs` 复核默认 ease_out_cubic、同 executor clock、第一目标即时和当前样本重定向，不靠换 key 制造入场。
- 实际依赖为 `gpui-kit 0.6.1 / gpui-pre 0.3.4`；不沿用设计守则中历史 0.6.0 作为 API 事实，不混用 Zed 上游。动画不保证 compositor-only。
- 已读本树 [AGENTS](../../AGENTS.md)、[exec-guide](../vega-exec-guide.md)、[Issue workflow](../vega-issue-workflow.md)、[design guidelines](../vega-design-guidelines.md)、[Rust 测试生命周期](../vega-rust-test-delivery-lifecycle.md)、[kanban skill](../../.agents/skills/vega-kanban-delivery/SKILL.md)。核对目标路径的各级规则，无额外 nested AGENTS；没有重新全仓扫描。
- 外部只读状态变化：开工核对 #553 hash 与交接一致；最终静态保护检查读到 `e3045be72c766b2c583c979aca6c6c0f8af2c8b13dc916e05039641d2462e15b`，导致“#553 hash 未变”断言 exit 1。本任务未写 #553，不恢复、不覆盖该外部变化，由父 Agent 核对来源；它不是 #552 产品 RED。
- 当前完成：§5.6 恢复授权内的 22 个先行测试属性、真实控件/内容/滚动/输入/绘制观察、W1–W12 源码映射及静态核对；可供独审，不是已审/GREEN。未取得/未声称：Actions 编译/发现/执行、阳性对照通过、有效 RED/GREEN、生产实现、用户手测或性能收益。精确未提交内容与命令退出码见当前先行测试交接；完整 commit/可访问源码/run/job 仍由父 Agent 固定。

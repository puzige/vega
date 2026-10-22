# #554 侧栏与 Workspace 面板动效：静态评估

- 内容版本：`v1.0-static-study`（2026-10-22），供父 Agent 审阅；不是动画实现授权。
- 原始验收：[Issue #554](https://github.com/puzige/vega/issues/554)，本轮读取正文的 `updatedAt = 2026-10-22T16:39:31Z`。
- 工作分支：`feat/554-panel-motion-study`；源码基线：`871f91f246e90753743a256cee1eec315e74effc`。Issue 原审计基线为 `83fd51d79940516029cdc1eaf8e81566870c877b`；本轮关键调用链已按当前基线复核。
- 父 Agent 已 fetch 并建树；写入前只读 `ls-remote` 确认远端 `master`、本地 `origin/master` 和三卡 HEAD 相同，工作树干净。没有另行修改共享 Git 状态。
- 本轮只新增本文件；未运行 Vega、构建、check、Clippy、测试、bench、profile 或 CUA。未核实用户安装版本，未取得运行或性能数据。

## 1. 验收矩阵（先于方案）

以下是评估交付与未来候选的完整边界；静态证据不表示运行回归通过。

| ID | 场景/要求 | 可接受结果 | 本轮证据或后续验证 |
|---|---|---|---|
| E1 | 侧栏、右侧 Environment/Workspace、底部 Dock 开合与拖动 | 列出状态→尺寸→测高/终端→绘制链，区分必要布局与装饰帧 | §2 静态调用链；present 未观测 |
| E2 | 方案选择 | 比较即时切换、最终布局一次提交＋局部淡入、独立视觉层；允许不做动画 | §3、§4；不预先承诺性能收益 |
| E3 | 开关、连续开合/反向、关闭与回焦 | 真实可见状态、命中、键盘/焦点即时生效；旧任务不能恢复已关闭面板 | 未来生产组件进程内事件/焦点测试＋用户手测 |
| E4 | 拖动分隔线、窄宽窗口及断点变化 | 直接跟手；无弹簧追赶、全宽/全高逐帧装饰插值；只做必要布局 | 静态确认入口；运行待验证 |
| E5 | 对话宽度真实改变 | 不删除/伪装必要重测；保留锚点、选区，阅读历史时不启动底部跟随 | 未来真实 ConversationStream；不接管 #344 |
| E6 | 长对话、单条长回复、表格/代码、流式生成中阅读历史 | 分场景记录布局与滚动结果，不以简化 BenchStreamView 代替生产 | §5 的场景集与指标；全部未测 |
| E7 | 活跃终端输出；右/底部布局；Terminal/Diff/文件内容 | 保持实体、会话及资源身份；不复制正文做 crossfade，不把输入转给旧对象 | 进程内生产组件＋隔离外部边界；真实 PTY 另需环境 |
| E8 | Reduce Motion、隐藏、失活、完成、恢复 | 即时终态；无装饰性帧循环；恢复不补播过期开合 | 未来共享 #549 生命周期回归＋本卡接线验证 |
| E9 | 错误、权限提示及可访问性 | 必要提示不被遮挡/延迟，关闭与回焦即时；浅深主题及键盘入口保留 | 未来行为与用户手测，不靠截图断言焦点 |
| E10 | 权限和并行边界 | 不实现尺寸动画，不升级依赖，不改 #344/#535；窄 Environment 浮层归 #549 | 本轮仅 `docs/554/`；§7 明确授权门槛 |
| E11 | 可复现证据 | 记录构建、场景、屏幕条件、采样来源与局限；缺数据明确未知 | §5、§6；无伪造数值/RED/GREEN |

## 2. 真实调用链与风险归因

路径均相对仓库根，行号固定于上述源码基线。以下“事实”来自静态代码，不是实测性能结论。

### 2.1 侧栏

```text
ToggleSidebar
  → window/render.rs::toggle_sidebar (721–727)
  → sidebar::show_persisted / toggle_persisted
  → Render 条件挂载 sidebar 与 resizer (473–476)
  → workspace.rs::workspace_available_width (931–937)
      扣除实际 sidebar width + resize hit area + 两侧 main gap
  → render_workspace → ConversationStream::set_workspace_width
  → 必要重测/锚点恢复 → GPUI 布局/绘制 → 平台呈现（未采样）

分隔线 mouse-down → begin_sidebar_resize (532–541)
  → resize_sidebar (544–553) → sidebar::set_width(pointer.x)
  → 实际布局随输入变化
mouse-up / 按键释放 → finish/persist_width (548–560)
```

连续拖动引发实际尺寸变化是必要交互成本，不应通过弹簧追赶、延迟真实宽度或停止重测来“优化”计数。装饰动画若自行再生成一串宽度，则是在这条链上追加工作。

### 2.2 Workspace / Environment / Dock

- `crates/vega/src/window/workspace.rs::workspace_toggle_bottom`（406 起）、`workspace_toggle_right`（439 起）、`workspace_hide`（707 起）是现有开合/关闭入口；本卡不改变其焦点、可见性与实体语义。
- `render_workspace`（1406 起）计算可用宽度、右栏宽度、底部高度及 fullscreen/Environment 投影。
- `1476–1488` 将实际对话可用宽度传给 `ConversationStream::set_workspace_width`：右栏时为 `available - width - 5`，Environment rail 时扣 `Layout::ENVIRONMENT_RAIL_WIDTH`，否则为 `available`。
- `1498–1522` 用真实 `.w(px(width))` 布局右面板；`1535–1557` 用真实 `.h(px(height))` 布局底部 Dock。底部高度变化主要影响可见高度与终端 rows，不能一概声称也触发对话宽度重测。
- `1680–1703` 的拖动直接由指针位置写 `workspace.width/height`，并更新标签可见区请求。公式是窗口右缘/底缘减指针位置；不另加装饰性的中间尺寸。
- 窄窗 Environment overlay 的入场生命周期属于 #549；本卡只辨认它与占用真实列宽的 rail 不同，不设计第二套浮层。

### 2.3 对话测高与锚点

`crates/vega_ui/src/conversation_stream/core.rs::set_workspace_width`（259–283）：

1. 保存 `workspace_width`；已有宽度与新值的差绝对值 `> 0.5` 才进入宽度变化条件。第一次设置没有 previous，并不由这一条件触发重测。
2. 宽度变化且 `entries` 非空时：清 `measured_entry_heights`，确保 entry identities，取得 logical scroll top 和 anchor snapshot，执行 `list.remeasure()`，再 `restore_scroll_anchor_at()`，通知更新。
3. 另有 compact 投影变化；不能只数 remeasure 就断言没有其他工作。

**推论**：若装饰动画每帧提交不同的真实列宽，符合条件的帧可能重复进入上述链。不是每帧必定重测，也不能由阈值推算实际重测次数或掉帧率。不能提高阈值、保留错误测高或绕开 anchor 来获得更少计数。#344 的独立问题边界保留。

### 2.4 终端：布局可到达真实 PTY

```text
TerminalView::render 的 canvas (vega_ui/src/terminal.rs:278–281)
  → paint_terminal(bounds) (301 起)
  → 字形度量 → rows / cols (325–330)
  → size 不同且 session 存在 → session.resize (331–336)
  → TerminalSession::resize (vega_conversation/src/terminal.rs:118–121)
      向有界命令通道 try_send(Command::Resize)
  → 后端消费 Resize (305–314)
      pair.master.resize(PtySize) + parser.screen_mut().set_size
```

rows/cols 取 floor 后限制在既有范围；像素变化没有跨单元格边界时不必产生新 resize。`session.resize` 成功仅说明请求入队，不能冒充内核 PTY 已完成调整。绘制链存在真实副作用，但本轮未测队列压力、实际 resize 次数或呈现时延。

### 2.5 绘制与呈现不能混为一谈

GPUI 的布局、prepaint/paint、animation-frame 请求和平台 present 是不同层。`Window::painted_quads()` 可检查进程内最终绘制场景中的矩形，不等于显示器已呈现，也不是帧耗时采样器。GPU 绘制或 alpha 过渡不保证布局、重渲染和业务更新免费；本轮没有 compositor-only 的依据。

## 3. 候选比较

| 候选 | 真实尺寸/内容与交互 | 可能收益与代价 | 决策 |
|---|---|---|---|
| A. 保留即时开合 | 一次提交目标状态与最终几何；框架仍可有必要布局过程；命中/焦点随真实状态 | 无新增装饰帧，风险最小；连贯感依赖清楚的结构与控件 | **当前推荐**；可直接作为评估结论，不要求为本卡造代码 |
| B. 最终布局一次提交，局部标题/边框淡入 | 先完成真实开合；仅新出现标题的非交互视觉或现有边框 alpha 变化；正文、控件、错误及回焦不等动画 | 可以提示新区域；仍会请求绘制帧，需证明没有额外尺寸变更/重测/PTY resize | 仅保留为后续低风险候选；没有用户观感证据前不默认实施 |
| C. 独立视觉层过渡 | 真实布局先完成；不承载输入的装饰层留在自己的固定边界内，不能是 Terminal/Diff/对话截图或复制实体 | 可表达空间关系；额外遮挡、裁剪、命中和生命周期风险；独立层不自动等于合成器层 | 目前无必要性证据，**不推荐进入实现**；须单独证明收益和 API 可行性 |
| D. 全宽/全高逐帧插值、整正文 crossfade | 连续写宽高或保留两份内容 | 放大 §2 链的风险；延迟命中/焦点、重复内容与会话风险 | **拒绝作为默认方案**，不得借本评估实施 |

“一次最终布局”指产品只提交最终尺寸，不承诺框架只执行一次 layout/remeasure。对 B 的正确比较是：在同输入/数据下，装饰采样不得额外改变真实 bounds、失效测高或发出尺寸请求；不能以删除本来必要的布局取得更小数值。

## 4. 保守结论与依赖核对

1. **本卡结论：不做尺寸动画，保留即时开合和直接拖动。** 当前只有风险调用链，尚无证据证明装饰收益值得增加工作；“不做”不是未完成评估。
2. 若后续用户认为即时开合缺少层级提示，优先单独评审 B，不直接跳到 C/D。B 不改变关闭/回焦时序，不保留离场正文或隐藏控件，不新增常驻解释文案。
3. 当前 `Cargo.toml/Cargo.lock` 为 `gpui-kit 0.6.1 / gpui-pre 0.3.4`，不是设计守则历史条目里的 Kit 0.6.0，也不是任意 Zed 上游 API。
4. 当前 `vega_theme::Motion` 仅见 `INTERACTION_DURATION_MS = 120` 和 `THINKING_SHIMMER_MS = 1800`。导航候选时长不等于现成共享导航 token；120ms 只能是产品建议，不是 Apple/Kit 规定或性能结果。
5. 锁定来源 Motion 文档支持 keyed `transition`、有限 keyframes、`Presence`、`MotionTransform` 等；首目标直接采用、后续同 stable key 从当前采样重定向，Reduce Motion 立即终态。`MotionReveal` 会测量并裁切自然尺寸，`Presence` 会保留退出中的挂载，均不构成本卡可以动画化真实面板尺寸/延迟卸载的许可。
6. 隐藏/失活/恢复、防过期任务及共享 token 的唯一未来写入者是 #549。本卡不复制 helper 或策略，不因本卡评估要求 #549 先实现尺寸动画。

## 5. 可复现观测计划（未执行）

### 5.1 冻结记录和生产场景

若获后续观测授权，先记录：完整源码/构建 SHA、云端产物标识与二进制身份、Kit/pre 版本、OS/硬件、屏幕刷新率与可变刷新状态、缩放、窗口逻辑尺寸、浅/深主题、Reduce Motion、前台/失活、面板停靠方向、会话数据版本和操作序列。当前安装构建及这些环境值均未知，不填写假值。

使用脱敏合成数据装载**真实 Workspace、ConversationStream、TerminalView**；不要用独立 demo 或 BenchStreamView 代表生产：

- 多消息长对话；单条超长回复；表格和代码块；记录数据条数、字节数及 fixture hash，不发布用户正文。
- 流式更新同时阅读历史（已脱离底部跟随），以及底部跟随作对照；包含选区。
- 同时有活跃终端输出及对话流；右栏、底部 Dock、侧栏单独开合及组合。
- 连续开→关→开、反向、隐藏/失活后恢复、错误/权限提示可见时关闭。
- 窄/宽窗口及断点两侧；拖动分隔线；最大化/恢复仍按既有契约。

固定数据、操作顺序和测量窗口，候选与即时基线在相同环境交替采样；分别报告有效样本数、失败/中断样本及原始来源，不以重试丢掉较慢结果。具体样本预算和性能门槛须在后续采样 spec 冻结，本轮没有可支持的收益门槛。

### 5.2 指标分层

| 层 | 要记录什么 | 可行来源与边界 | 当前状态 |
|---|---|---|---|
| 业务/交互 | 开合后真实状态、输入目标、焦点、实体身份、scroll anchor、选区、跟随模式 | 生产组件 GPUI 进程内事件与状态；真实系统集成用户手测 | 未执行 |
| 布局 | 实际列宽/终端 bounds 的时间序列，宽度失效、remeasure 与 anchor 恢复次数 | 真实生产链上的只读测试期 observer；不能用镜像判断函数充数 | observer 接线待独立授权与审查 |
| 终端 | 期望 rows/cols、resize 请求、入队/失败、后端应用计数与终态分开记录 | CI 只隔离外部 PTY 边界；真实 PTY 后端/内核计数另需许可环境 | 未执行；真实 PTY 数据缺口 |
| 绘制/调度 | 局部 alpha、bounds、装饰 frame request 在结束/隐藏后的停止 | `VisualTestContext::draw`、`painted_quads`；调度需实际 frame-request 观测，不能数无关流式帧 | API 路线可行，接线未验证 |
| 帧/呈现 | 帧间隔、CPU 布局/绘制耗时、提交与实际 present、超预算帧，采样期间背景负载 | 有显示服务的许可环境及平台追踪；不能从 headless quads/CI 墙钟换算 | 未取得，现有 Actions 不自动提供 |
| 用户体验 | 层级是否清楚、是否闪烁/迟滞、直接拖动、输入和返回可用性 | 用户用核实构建纯手测并给结论 | 未收到 |

算术边界：60Hz 理论间隔为 `1000 / 60 ≈ 16.67ms`，120Hz 为 `1000 / 120 ≈ 8.33ms`。这是刷新率换算，不是 Vega 达标结果；可变刷新率、调度和实际 present 必须另外记录。

### 5.3 进程内测试可行路线与阳性对照

未来如授权执行性观测/行为实现，使用 `TestAppContext / VisualTestContext` 挂真实根组件及真实 controller，注入框架事件，推进 executor clock 并 draw。外部模型/存储错误/PTY 仅在实际外部边界隔离，不 mock 掉导航、测高判断或绘制决策。先在同一测试补丁和首轮现有 Actions 证实以下对照，不另设新 CI 阶段：

- **测高阳性对照**：非空真实 ConversationStream 先具有已知宽度，再提交大于 `0.5` 的真实差值，观察必要 remeasure 与锚点保留；同宽度再次提交不应产生该宽度失效。不能只有永远为零的计数。
- **终端阳性对照**：真实 TerminalView 使用有效 session 边界，先改变 bounds 跨越至少一个实际测得的单元格，再核对 resize 参数和最终网格；相同网格尺寸为负对照。CI 的边界 recorder 不证明真实 PTY 已调整。
- **绘制/时钟阳性对照**：在实际生产组件发生一次被允许的可见状态变化，确认 draw 的 bounds/颜色随真实状态改变；若未来有 B，在中间时间确有局部采样差异，终态完成后无装饰性请求。框架 clock 与锁定动效 clock 的连接必须在 Actions 验证，不能因调用 `advance_clock` 就假设已经推进。
- **焦点/路由阳性对照**：通过生产动作打开并显式聚焦一个真实控件，`contains_focused` 应为真；关闭后按现有规则回焦，旧对象不能再收输入。外部 CGEvent/AX 或截图不能替代该断言。
- quads 的 bounds 是缩放像素，按实际 `scale_factor()` 对齐逻辑 bounds；纯色通过 `Background::as_solid()` 读取。不假设 `capture_screenshot` 已配置 HeadlessRenderer，也不把 quads 当呈现帧。

以上是未来测试设计，不是已存在/已执行的测试。若没有实际 frame-request、glyph 或 PTY observer，先明确观测缺口并补齐被授权的测试接线；不得用源码 regex、toy 状态机、只验证 mock 调用或跳过必需回归换取通过。

## 6. 未决与取得证据的下一步

| 未决 | 本轮能否决定 | 具体下一步/责任 |
|---|---|---|
| 是否值得任何结构动画 | 当前可以决定保留即时方案；不能宣称用户更喜欢 | 父 Agent 审评估，用户若提出观感缺口再评 B |
| 真实 present、PTY 成本和实际掉帧 | 不能；没有许可运行/采样环境或核实构建 | 用户手测，或另行明确授权有显示服务/真实 PTY 的环境和采样工具；本 Agent 不本地绕过 |
| #549 共享生命周期接口 | 尚未交付到本基线 | 父 Agent 协调其 writer 的契约；本卡不写共享文件 |
| 进程内计数/时钟/资源身份观测接线 | 路线已知，实际接线未执行 | 仅在代码授权后，先行测试及阳性对照在既有 Actions 验证 |
| #344/#535 活跃修改影响 | 本轮不接管、不改其范围 | 将来实施前重核最新基线及增量；相关缺陷保留独立归因 |

这些缺口阻止“已证明性能收益”或直接实施候选，不阻止本轮纯静态评估交付。

## 7. 后续符号计划与授权界线

**本轮允许动作已经止于本文件。** 不修改源码、Cargo、工作流、共享 README，不新增测试/benchmark/探针；父 Agent 统一 commit/push/PR/Issue 状态。本 Agent 不嵌套委派。

如果另行授权 B，先冻结其可见对象、时长/曲线、无尺寸变化与失败边界，再按矩阵写测试；具体生产候选入口仅为：

| 文件/符号 | 将来允许评审的最小动作 | 不允许顺带做 |
|---|---|---|
| `window/render.rs::Render / toggle_sidebar` | 消费真实开合后的局部标题/边框视觉投影 | 改侧栏持久化、焦点或尺寸时序 |
| `window/workspace.rs::render_workspace_pane / render_workspace` | 最终布局已提交后消费共享 #549 的局部视觉能力 | 逐帧写 width/height、延迟关闭、复制 body、实现窄 Environment 浮层 |
| `ConversationStream::set_workspace_width` | 测试期只读观测真实必要重测与锚点 | 删除重测、提高阈值以掩盖问题、接管 #344 |
| `paint_terminal / TerminalSession::resize` | 在批准的边界观测参数和终态 | 本地启动真实 PTY、为了动画重建 session 或屏蔽 resize |
| `vega_theme::Motion` 与共享 lifecycle | 由 #549 单一 writer 提供；本卡消费 | 本卡复制常量、另造动画引擎或升级依赖 |

如未来有代码：生产未改时写齐本轮测试 → 独立审查 → 父 Agent draft PR 的现有 `pr-check` 有效 RED → 冻结 → 最小实现 → 同一测试 GREEN → PR 固定源码/完整 SHA/patch/run/job → 独立删 Rust 测试及独占接线 → 最新 head 完整空集门禁。不得恢复历史测试全集；纯文档本轮 **RED/GREEN 不适用**。

现有 `.github/workflows/pr-check.yml` 使用 `macos-latest` 的 Clippy/Nextest job，运行 fmt、全目标 Clippy、保留的 Python 检查及 `cargo nextest list/run --workspace`；仅空集使用 `--no-tests=pass`，汇总为 `check (fmt, clippy, test)`。本轮未触发 CI，未报告任何运行门禁通过。未来回滚只撤回装饰层接线，保留真实开合、布局、锚点和 PTY 行为；不回滚 #549 的其他消费者或 #344/#535。

## 8. 依据与交付说明

- [Apple HIG Motion](https://developer.apple.com/design/human-interface-guidelines/motion)：动效服务反馈、尊重减少动态效果；不提供本卡产品时长或性能保证。本轮未以网页抓取结果冒充产品测量。
- [Kit Motion 固定来源](https://github.com/longbridge/gpui-kit/blob/96103905ea0c9c199db206ada7a9b2e3114e6339/website/base/motion.md)：本轮已读取；其纯采样 benchmark 是上游环境数据，不外推 Vega 生产布局/present 成本。
- [Zed 120fps](https://zed.dev/blog/120fps)：帧与呈现节奏的背景参考；不是锁定 Kit API 或 Vega 性能报告。
- 本仓规则：[AGENTS](../../AGENTS.md)、[exec-guide](../vega-exec-guide.md)、[Issue workflow](../vega-issue-workflow.md)、[design guidelines](../vega-design-guidelines.md)、[kanban skill](../../.agents/skills/vega-kanban-delivery/SKILL.md)。三工作树同基线且 AGENTS 内容一致；未发现作用于本路径的 nested AGENTS。
- 已执行的是 Issue/规则/定点源码静态核对；最终文件 SHA-256、范围/空白/链接检查由 READY 回报绑定。本文件不记录不存在的提交、PR、运行计数或性能改善。父 Agent 审阅后推进，本轮写入者回报 READY 后停止写入。

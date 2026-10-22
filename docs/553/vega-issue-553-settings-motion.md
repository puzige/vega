# #553 Settings 层级导航动效规格草案

- 内容版本：`v1.1-spec-draft`（2026-10-22）；主 Agent 已审，统一候选曲线与生命周期术语；尚未冻结完整先行测试/实现。
- 原始验收：[Issue #553](https://github.com/puzige/vega/issues/553)，本轮直接读取完整正文。
- 分支：`feat/553-settings-motion`；初稿源码基线：`871f91f246e90753743a256cee1eec315e74effc`；续接源码 HEAD：`6615748f91273bfb8ee424011b3b5a09b3777d2b`。Issue 原审计基线为 `83fd51d79940516029cdc1eaf8e81566870c877b`；关键 Settings 文件相对初稿无变化，当前只读差异已核对。
- 初稿由父 Agent fetch 并建树；续接也已 fetch、快进并保留原文档。当前只有本卡文档改动，不动脏主工作区；Git/index 与远端由父 Agent 统一协调。
- 本轮仅新增 `docs/553/` 文档；不实现源码或测试，不改共享文件；不运行 Vega、构建、check、Clippy、测试、bench、profile 或 CUA。

## 1. 守卫、焦点与响应式验收矩阵

先固定完整验收，再讨论符号接线。全部行为测试和真实手测尚未执行；静态证据不表示回归已通过。

| ID | 前置/操作 | 必须可观察的结果 | 后续验证路线 |
|---|---|---|---|
| S1 | 窄窗，无草稿，列表→provider 详情→返回 | guard 允许且真实导航提交后才有局部标题反馈；详情/返回操作即时可用；返回恢复列表 scroll 和合理焦点 | 真实 SettingsView 事件、draw、焦点/scroll |
| S2 | provider 草稿未保存，尝试切其他 provider/返回 | 原详情、草稿、输入、校验和位置保留；确认出现；目的地不提前变亮/淡入 | 真实 `provider_command` 与 guard，不用假决策函数 |
| S3 | 确认框“留在本页”/取消 | 仍在原位置；不播放“已进入”、不消耗未来合法导航的动画，不清草稿/错误 | guard 控件事件＋目标/视觉代次不变 |
| S4 | 放弃修改后切换，或保存成功后继续 | 仅在真正完成导航后生成一次反馈；不在点击保存时提前进入、不重复播放；草稿清理遵循原业务规则 | 生产完成/恢复链，外部保存边界隔离 |
| S5 | 校验不通过、保存失败、正在保存时尝试导航 | 留在原编辑位置；错误/待处理提示及时清楚，原草稿可恢复；禁用原因和已有 guard 行为保留 | 真实验证/恢复逻辑＋失败结果注入；非真实网络证据 |
| S6 | provider→model 基本/思考/定价→返回 provider | 相同守卫原则覆盖 model draft、reasoning、pricing；标题/marker 对应真实 provider/model/section，不能落到无关模型 | `request_settings_destination / resume_settings_navigation` 真实路径 |
| S7 | Tab/Shift-Tab/Enter/Space，过渡中输入、校验、保存/取消/返回 | 没有等待、幽灵节点、旧表单收输入或焦点丢失；按钮名称/命中即时正确 | GPUI 进程内输入与 FocusHandle，不靠截图/系统合成事件 |
| S8 | 快速选 provider/model、连续返回、导航反向 | 只显示最新真实目的地；视觉可以中断，不排队；旧异步结果/动画完成不得恢复旧详情 | 可控 clock＋真实 controller 结果/终态 |
| S9 | 删除当前/非当前对象、列表更新/重排、空列表 | 稳定身份不串对象；沿用现有有效 fallback/空态；可用焦点归属明确；无旧动画复活对象 | 真实 config 投影/删除路径＋身份/焦点断言 |
| S10 | 窄→宽→窄，包括进行中的动效和草稿 | 立即必要最终布局；不动画化断点，不重复初始化 editor、不丢草稿/校验/独立 scroll；不把宽屏自动显示详情当用户导航 | viewport/bounds 改变后的生产 render |
| S11 | 宽窗选择 provider/model 分区 | 仅原位选中 marker/填色短过渡；真实选中、详情及输入立即切换；不整列表/表单移动或淡入 | 实际 row/分区 marker quads＋业务状态 |
| S12 | Reduce Motion、隐藏、失活、结束、恢复 | 直接终态或停止装饰帧；恢复不补播旧进入/返回；真实错误和异步结果不被停止 | #549 生命周期＋本卡消费接线 |
| S13 | 浅/深主题、标题/返回按钮发现性与可访问名称 | 标题真实文本/语义即时；返回按钮/焦点环全程清楚；状态不只靠运动；不增加多余解释文案 | 生产语义/绘制＋用户手测 |
| S14 | 内容滚动到中部再进入、返回、多次编辑/错误恢复 | 列表与详情各自 scroll 保留；返回目标还在则焦点到该对象，否则合理邻项/列表入口；不跳回无关位置 | 真实 scroll handle 与焦点测试；基线缺陷先报告 |

## 2. 单一候选视觉方案与非目标

### 2.1 本草案采用的局部表现

| 模式 | 允许变化 | 不参与动效的内容 |
|---|---|---|
| 窄窗，已批准的进入详情/详情层级切换 | **仅标题/返回区域中的非交互标题文字**做一次短淡入；保留固定布局位置和真实文本语义；不保留旧标题叠层 | 返回按钮、按钮文字、焦点环、表单、字段、错误/权限/待处理提示全程即时、全 opacity |
| 窄窗，返回列表 | 即时恢复列表 scroll/focus；终止离开的标题效果。若目标为已有上级详情，只为其标题产生新的合法进入反馈 | 不让整列表淡入，不为了“退出动画”保留旧详情或延迟回焦 |
| 宽窗 | provider 现有选中填色作为原位 marker；model 分区已有底部 marker 仅改变颜色/alpha，不跨位置移动 | 标题、整行文字/按钮、整张表单和列宽不淡入/滑动 |
| 初次挂载、断点切换、恢复、无真实目的地变化 | 直接当前终态，不新开导航动画 | 不能因为 render/notify/viewport 更新重播 |

候选时长统一 **120ms、Kit Transition 默认 ease_out_cubic、无 delay、无 spring/过冲**，位于 Issue 提议的 100–140ms 范围内；不是 `Easing::EaseOut` 的 cubic-bezier 曲线。这是供共享 #549 和首轮测试冻结的产品建议，不是 Apple/Kit 官方导航时长或观测结论。窄窗标题的 opacity 从 0 到 1 只影响非交互文字，标题文本的语义、占位和目标身份已在导航提交时更新；必要字段标签、操作名、错误标题不得被归入此淡入容器。若当前 render 不能隔离该标题文字，不得把整段 `settings_actions`/返回按钮/表单一起降 opacity，应在先行测试前明确最小结构调整。

宽窗选中强度 `0/1` 仅是视觉参数：provider 端点沿用当前主题的选中填色，model marker 沿用当前 accent；无硬编码 RGB、无 marker 尺寸变化、无“选中 progress”业务字段。旧项淡出期间不再有选中语义，新项即刻对应真实配置对象。

### 2.2 硬边界

- 不对整表单、整列、整页做 crossfade、滑动或缩放；不复制第二份表单/输入实体，不把旧表单留在树中等离场。
- 不插值尺寸，不动画化响应式断点，不改变窗口尺寸，不新增导航等待期。
- 不改 provider/model 配置、草稿归属、保存/取消/确认/校验语义，不延迟错误和必要提示。
- 不用动画代替 scroll/focus 恢复，不从动画结束回调提交导航或保存。
- 不新增常驻说明、成功提示、背景字幕或第二套导航。优先现有标题、返回控件、选中态表达层级。
- 不升级依赖、不引入动画引擎，不写 #549 的共享 Motion/lifecycle/窄 Environment 浮层；#344/#535 活跃范围不动。

## 3. 守卫之后才生成视觉状态

### 3.1 两阶段状态，只有一个业务权威

```text
用户请求（provider/model/返回）
  → 原有 saving / draft / settings guard
      ├─ 不准许/等待确认/校验或保存失败：留在原位，视觉目标不变
      ├─ 取消：清 pending 的原有业务处理；不生成进入反馈
      └─ 准许/成功恢复：原有真实导航提交
           → 对新的已提交目的地作视觉投影
           → 真实内容、命中、焦点与返回位置即时有效
           → 仅标题文字/原位 marker 有有限采样
```

不能在 `Command::Select` 点击回调入口、设置 `pending_command/pending_section` 时就加动画代次；也不能在点击“保存并继续”时推定成功。应在 `provider_command` 的已通过守卫分支、`finish_provider_navigation`、`finish_settings_navigation`/`resume_settings_navigation` 所形成的**真实提交结果**之后统一判定目的地是否变化。具体接线避免重复计数；一条成功导航只产生一个视觉事件。

目的地投影至少区分列表、provider 详情、model 配置及其 section，并用真实 provider/model 身份；表单是否有草稿、pending 请求和视觉 progress 不参与业务决策。最新业务投影是唯一权威，异步结果不能通过旧动画闭包修改它。若保存结果只更新当前表单而未导航，不播层级进入。

### 3.2 稳定身份与快速中断

- 宽窗 provider 当前行 ID 使用 `("provider-select", index)`，**不能直接用 index 做动效 key**。未来动效身份应由当前真实 provider 身份（基线以 name 识别）＋属性/容器命名空间组成。重命名按业务身份变化处理，不做跨对象 morph；若要改交互 ID/focus 映射，先列入首轮测试范围，不顺带重写拖拽/排序。
- model 目的地使用 `(provider, model, section)`；不能只用 label 或 section index 区分跨 provider 的对象。marker 的属性 key 在同一对象内稳定，目标变化由 sampler 处理，不通过每帧换 key 重启动画。
- 窄窗标题需要**已提交导航事件**的一次有限入场，不能简单给新详情创建 `transition(key, 1)`：锁定 transition 第一次 target 会立即采用，这样不会淡入。候选采用有限 `Keyframes/animate_keyframes`，generation 仅在真实、可见、被批准的目的地变更时更新；初次挂载不产生 generation，也不借下一帧伪造导航。
- A→B→返回/再选 C 时，立即停止已离开标题的采样，不保留第二份标题/表单；新目的地使用当前真实状态。相同标题重新出现可重定向/重新开始有限入场，但不能出现先排队完成 B 再显示 C。宽窗 marker 由 stable keyed transition 从当前采样转向最新目标。
- 对象删除/重排后，旧 animation key 不引用旧输入实体。无旧异步 completion 可以重新挂载详情、改 selected 或抢焦点。

### 3.3 断点与生命周期

当前 provider render 在宽窗会令 `detail_open = true`。这是既有响应式投影，不能被误判为用户进入详情；窄↔宽只同步最终布局/视觉终态，不改变既有路由选择政策、不初始化 editor、不清独立滚动句柄。正在进行的标题反馈因模式切换停止；再次窄窗不补播上一轮。

#549 是 Motion token/生命周期唯一未来 writer，本卡消费候选 `SETTINGS_NAVIGATION_MS` 和共享策略，不造第二套 helper。Reduce Motion 立即终态；隐藏、失活、离开目的地、完成时停止本装饰的帧请求；恢复显示当前真实状态。仅在 render 读 active gate 不足以证明无补播：必须覆盖停用期间无额外 draw、直接恢复的顺序，并消费 owner 中断事实或共享停用同步。保存/网络/校验更新仍可继续，不能为停止装饰把业务任务或必要错误更新一并禁掉。

## 4. scroll、focus、实体与错误契约

- `provider_management.list_scroll` 和 `detail_scroll` 保留原实例；不要用新包装层代替真实 track_scroll，也不在动画开始/结束时置零。
- 返回列表时恢复原位置；若原对象仍在，焦点回到该对象的实际列表入口；已删除则采用现有有效邻项/列表入口，不能落到已卸载详情。进入详情的焦点按既有规则，而不是为了淡入强制聚焦标题。
- `TextInput`、provider/model editor 及配置投影实体在本次视觉变化中不复制/重建。草稿切换的保留或放弃仍由原 guard 执行；不能把草稿清理归因于“动画初始化”。
- 验证/保存失败保留草稿及可操作错误；标题的容器不包住错误、确认、保存/取消/返回控件。过渡中输入和保存动作按真实 busy/guard 状态立即处理。
- 当前源码能证明有独立 scroll handle、focusable 行和 guard，不证明返回焦点在所有路径已正确恢复。S7/S14 是硬验收，不得用“保留现有行为”掩盖缺陷；先行生产测试若发现基线缺陷，由父 Agent 记录和冻结必要修复范围/新一轮先测后修，不能在实现后追加测试追认，也不能删掉此回归。
- 系统 AX/外部合成事件对 GPUI 焦点有已知限制；`aria_label`、截图和颜色不单独证明焦点或可访问 selected 正确。使用实际生产语义投影及 FocusHandle，外部系统可访问性缺口明确报告。

## 5. 定点源码证据与符号级实现计划

仅为未来授权后的计划；本轮不改下列文件。

| 文件/符号（当前基线） | 事实 | 最小接线/保留项 |
|---|---|---|
| `crates/vega_ui/src/settings/provider_management.rs::provider_command`（397–460） | saving 先返回；指定命令遇 draft 写 pending 后返回；`BackToList`/`Select` 后才改真实位置 | 只在已提交目的地变化后触发视觉投影；不在 pending 阶段反馈 |
| `finish_provider_navigation / discard_provider_draft / close_model_configuration`（359/374/382 起） | pending 命令恢复、草稿清理及模型退出路径 | 保留原责任与顺序；成功恢复也只产生一次合法视觉事件 |
| `render_provider_management`（1493 起；1520–1528） | 由实际 content width 判断 narrow；宽窗强制详情可见 | 模式变化同步终态，不生成导航反馈，不修改阈值 |
| 同函数 list/row（1533、1564–1607） | list 独立 scroll；provider 行 ID 为 index，已有 selected 填色/drag/drop/键盘 | 宽窗仅原位选中 marker；动效 key 稳定身份化；保留拖拽及 guard |
| 同函数标题/返回（1700–1744）、详情容器（2026–2044） | 窄窗 BackToList 与 provider 标题同区域；detail 有独立 track_scroll | 只隔离非交互标题文字，按钮/错误/表单不降 opacity；不双挂载 |
| `crates/vega_ui/src/settings/model_configuration.rs::request_settings_destination / finish_settings_navigation / resume_settings_navigation`（40–98） | draft 阻塞；resume 要求无 error、无 draft、reasoning/pricing 非 Saving | 真实 commit 后做视觉投影；失败/取消不生成事件；不改保存规则 |
| `render_settings_navigation_guard`（114 起，175–205） | 放弃修改后 finish；留在本页清 pending 并 notify | 作为取消/放弃/保存失败回归入口，不能先播放目标效果 |
| `render_model_navigation`（346–394） | 返回 provider 经 request；各 model section 经守卫；已有底部 marker | 窄窗仅非交互标题；宽窗仅现有 marker；不缩放/滑动整 model 表单 |
| `SettingsView / ProviderManagementState` 的既有所有者 | 本轮未展开其全部字段/初始化，无需全仓扫 | 未来若需记录最小视觉身份/代次，归正确 owner；先核实际初始化/销毁，不增加业务 draft 状态或 public API |
| `vega_theme::Motion` 与共享 lifecycle | 当前仅 interaction 120ms、shimmer 1800ms | #549 唯一 writer，本卡只消费，不复写共享文件 |

按顺序：冻结 §1–§4 和共享接口 → 先写齐行为/失败/响应式/焦点测试与必要观测 → 独立审查与 Actions 有效 RED → 提交后视觉状态投影 → 局部标题/marker render 接线 → 同一冻结测试 GREEN。任何 API/可观察性问题先解决责任层，不在最后用源码 grep 验收补洞。

## 6. 具体进程内测试路线与阳性对照

### 6.1 生产组件路线

使用 `TestAppContext / VisualTestContext` 初始化真实 SettingsView、真实输入实体/配置投影和 guard。通过实际 provider row、返回按钮、导航 guard 控件或键盘路由驱动；只在外部保存/网络边界替换成功/失败结果，不能 mock 掉草稿判断、校验、pending 恢复或选择决策。

1. 在同一生产组件先 draw 列表、详情和带草稿表单，记录真实输入内容/实体身份、scroll、focus 与目的地。
2. 分别经鼠标与键盘触发请求；在不推进 clock 时断言守卫结果、目标/命中、错误、表单可用性及焦点/scroll；只允许局部视觉值未达终态。
3. 通过 executor clock 推进有限时间并 `draw`，检查标题/marker 的中间与完成状态，再做快速反向/取消/对象更新/隐藏/Reduce Motion。不按生产机帧率断言固定帧数。
4. quads 检查宽窗真实 marker/背景及固定 bounds；`Background::as_solid()` 读纯色，按实际 `scale_factor()` 换算逻辑/缩放像素。**quads 不含标题 glyph 的完整像素证据**；窄窗标题 alpha 需核对锁定场景/glyph 的可观察 API，或在真实标题绘制边界保留只读测试期 observer，并结合生产 header 的真实文本、控件与不变几何。仅断言一个 helper 返回 0.5 不够。
5. `FocusHandle::contains_focused`、`window.focus_next` 和真实输入接收者用于焦点；不是截图或 CGEvent/AX 的推断。外部 UI 系统集成仍交用户手测。

### 6.2 首轮先行测试内的必要对照

| 观测 | 阳性/负对照 | 不能伪装成 |
|---|---|---|
| guard | 在真实字段制造草稿再请求另一目的地：确认出现且位置不变；无草稿请求确实进入 | 自制 bool 状态机、只测 pending 字段赋值 |
| 保存失败/恢复 | 在实际外部保存边界返回失败，生产错误显示且草稿保留；成功允许原 resume 进入目标 | 绕过生产保存/校验直接调用视觉完成 |
| 焦点/scroll | 原列表滚动并聚焦一个真实对象后进入返回，确认同位置/有效 focus；删除原对象时确认 fallback | 隐藏节点仍持 focus 但截图“看起来正确” |
| clock/视觉 | 已批准的生产导航确有中间视觉采样并最终完成；取消/同目标/断点变换不生成该采样 | 从未动画导致“取消没有动画”假通过 |
| glyph/marker 观测 | 基线真实标题/marker 可被识别，状态变化影响所读绘制值；返回按钮/输入仍即时可见 | 将 quads 当标题像素、读取无关背景 |
| 生命周期/身份 | 可见合法导航有有限装饰请求，隐藏/完成后停止；重排/删除/连切仍对应真实对象 | 永远零帧、index 复用、暂停真实保存任务 |

已知框架有 `draw/events/focus`、`painted_quads` 与 `executor.advance_clock` 路线；clock 与锁定 motion 的连接、glyph/frame-request observer、实际 Settings 初始化/外部保存隔离仍需在未来首轮 Actions 证实。对照故障归 harness/接线，不算目标行为 RED。若实际锁定 API 无法观察标题绘制，记录具体缺口并先获得合理接线/验收授权，不能用源码 regex、镜像算法或 toy 决策替代，也不能偷偷去掉标题/焦点回归。

### 6.3 云端测试生命周期

未来父 Agent 统一 draft PR，现有 `.github/workflows/pr-check.yml` 的 `macos-latest` Clippy/Nextest job 运行 fmt、全目标 Clippy、保留的 Python 检查及 `cargo nextest list/run --workspace`；只有空集用 `--no-tests=pass`，required check 为 `check (fmt, clippy, test)`。不新增执行设施/真实外部 E2E 门禁。

生产未改先写齐测试 → 独立审查 → 现有 Actions 有效 RED（目标行为或最小缺失 API；环境故障不算）→ 冻结 → 最小实现 → 同一测试 GREEN → PR 固定完整 SHA/源码/patch/run/job/计数 → 独立删除 Rust 测试与独占支持 → 最终 head 空集完整门禁。保留全部必需回归到 GREEN，不恢复历史测试全集，不事后补测或放宽断言。最终 `no Rust tests` 不替代带测试 GREEN。

本轮只有文档，**RED/GREEN 不适用**，未编写/运行测试、未触发 CI。真实输入、保存失败体验、窄宽切换、浅深主题与标题/返回可发现性由用户在核实的云端构建手测；记录构建、OS、窗口/屏幕、Reduce Motion、合成配置与操作序列，不能发布真实 API key。真实 present/性能采样需另获环境许可，不能从 headless clock 推算收益。

## 7. 未决、进入实施的条件及回滚

| 未决 | 本轮结论 | 后续具体动作 |
|---|---|---|
| #549 token/有限入场/生命周期接口 | 本卡只声明消费契约，不新造共享 writer | 父 Agent 协调共享方案、同步最新基线后冻结 |
| 120ms/default ease_out_cubic、标题隔离 | 主 Agent 统一的单一候选；不称官方值或已实施 | 首轮测试先证明返回/错误不被淡入，完整包冻结前核锁定有限入场 API |
| 当前返回 focus 的全部路径 | scroll 句柄已核，不能宣称所有 focus 恢复已通过 | S7/S14 真实组件先测；发现基线缺陷先记验收/范围，不跳过 |
| 标题 glyph、frame-request、clock 观测接线 | quads 不足以独立验收文字淡入；路线及对照已列 | 授权后先核实际锁定 API/最小测试期 observer，现有 Actions 验证 |
| 真实安装构建/用户体验 | 本轮未知且未测 | 云端产物核实后由用户纯手测；不本地启动 Vega |
| #344/#535 等并行任务与基线漂移 | 不修改其范围，不推断已解决 | 将来代码阶段重新审增量和受影响不变量 |

回滚候选只撤掉本卡局部标题/marker 视觉投影，恢复即时呈现；不触碰保存/草稿/验证/guard、scroll/focus 和 #549 其他消费者。本轮没有生产代码可回滚。父 Agent 统一 commit/push/PR/Issue 状态；本 Agent 不嵌套委派，READY 后停止写入。

### 7.1 续接版本说明

本树已 fetch 并快进至 `6615748f91273bfb8ee424011b3b5a09b3777d2b`；保留 #547 图片预算与初始输入行为，以及 #546 Composer 胶囊样式，不从旧审计 ref 恢复文件。本卡关键 Settings 源码相对初稿基线未变。当前只固定规格，#552 的测试在独立树进行，Settings 代码仍未开始；生产 `provider_management` 的 E09 菜单接线归 #549，表单导航接线须停写版本后串行集成。以下历史分析事实不冒充当前行为验证。

## 8. 依据与交付边界

- [Apple HIG Motion](https://developer.apple.com/design/human-interface-guidelines/motion)：有目的的反馈与减少动态效果；不是本产品的时长规范。
- [Kit Motion 固定来源](https://github.com/longbridge/gpui-kit/blob/96103905ea0c9c199db206ada7a9b2e3114e6339/website/base/motion.md)：本轮已读；stable keyed transition、有限 keyframes、generation、reduced motion。`Presence` 的延迟卸载不是保留双表单的理由；动画不保证 compositor-only。
- 当前实际依赖 `gpui-kit 0.6.1 / gpui-pre 0.3.4`；设计守则中历史 0.6.0 不作为当前 API 事实，不混用 Zed 上游或升级依赖。
- 已读 [AGENTS](../../AGENTS.md)、[exec-guide](../vega-exec-guide.md)、[Issue workflow](../vega-issue-workflow.md)、[design guidelines](../vega-design-guidelines.md)、[kanban skill](../../.agents/skills/vega-kanban-delivery/SKILL.md)；当前基线与历史记录按 §7.1 区分，本路径无适用 nested AGENTS。
- 当前完成：Issue 全边界矩阵、守卫后反馈契约、局部视觉候选、符号计划、生产组件测试路线/阳性对照及真实缺口。未声称实现、独立审查、测试、用户手测或性能改善。最终文件 SHA-256 与静态检查结果由 READY 回报绑定，父审后同卡继续推进。

# Issue #403 实现交付记录

依据 [冻结规格](vega-issue-403-conversation-underlay.md)。本记录在代码实现前建立，运行状态只记录实际证据。

## 验收矩阵与实现计划

| ID | 风险与前置状态 | 操作 | 可观察结果 | 证据层级 | 状态 |
|---|---|---|---|---|---|
| U1 | Native / Codex 非空长会话，浅深主题 | 上翻历史并检查生产视口和绘制 | 真实列表覆盖 Composer 所在区域；48px 背景透明过渡绘制在消息之上；正文与 Composer 同轴 | GPUI 布局与 painted_quads；用户视觉手测 | 已实现 / 待 CI / 待手测 |
| U2 | 贴底长会话 | 多行输入、真实附件与附件错误、待发送队列、控制器错误及隐私错误增减，窗口缩放 | 原生列表余量随实际前景高度变化；末项在前景上方完整可见，列表项数与业务项数一致 | GPUI 生产布局 / 原生 ListState | 已实现 / 待 CI |
| U3 | 回看历史 | 改变 Composer 高度并追加流式文本，再点击回到底部 | 业务消息身份和像素读位保留；回底恢复跟随 | GPUI 生产滚动 / 交互 | 已实现 / 待 CI |
| U4 | 消息在前景下方 | 在过渡区滚动，在实色前景点按；打开模型、权限和上下文菜单 | 装饰不拦输入，实色面阻止底层命中，菜单位于前景之上且可关闭回焦 | GPUI 命中 / 焦点 / 绘制；既有菜单回归 | 已实现 / 待 CI / 待手测 |
| U5 | 空会话、较短但可滚动会话、底部必要提示 | 空任务与消息恢复；检查阅读导航和必要状态 | 空态 utility bar 保持布局；导航依实际可读高度显示；必要提示可操作 | 既有回归与新增生产测试；用户手测 | 已实现 / 待 CI / 待手测 |

实现顺序：先新增生产回归，再将非空会话底部栈置于前景，通过实际布局测量提供原生列表 bottom padding，绘制纯装饰渐变并保护实色面的命中；最后核对锚点可读高度及被新需求直接替代的旧位置断言。空态沿用现有路径。不改存储、权限与提交接口，不新增依赖或产品文案。

本地禁止编译、构建、打包、Clippy、test / nextest。仅运行快速格式和差异检查。新增生产测试在云端 PR Check 运行；本地不伪造红 / 绿测试证据。

## 实现

- `conversation_stream/render.rs`：非空消息区域保持全高，底部栈独立固定于前景。48px 透明至当前 `bg_base` 的渐变没有事件处理器或命中遮蔽；导航按钮在该区域之上。实色前景使用 `occlude()`，覆盖 Composer 两侧与下方，阻止被遮消息响应点击。既有模型、权限、文件选择和待发送浮层的 deferred priority 未改动。
- 定位状态和电脑操作提示移入可测量的实色前景，位于阅读导航下方、隐私和 Composer 之前；提交面板保留在 Composer 之后。所有既有文案和动作保留。
- `conversation_stream/core.rs`：保存底部栈实际高度，供原生 `list().pb(...)` 使用。测量只在高度变化时，借助 `cx.defer` 在绘制结束后的普通更新循环回写并通知，不重建 ListState、不插入业务消息、不扫描全量消息、不主动恢复 Tail；既有原生跟随与历史锚点继续生效。
- `conversation_stream/message_anchors.rs`：导航的可读视口高度扣除前景，避免内容仅在前景下溢出时漏掉导航。
- `vega_theme/src/lib.rs`：共享 `Layout::CONVERSATION_BOTTOM_FADE_HEIGHT`，48 逻辑像素。
- 仅按冻结规格替换 `issue98_scroll_button.rs` 与 `issue296_desktop_ui.rs` 四处被需求直接替代的“按钮 / 导航必须在列表之外”位置条件；同轴、尺寸、内距、绘制顺序、点击、Enter、焦点和阅读位置断言保留。空会话继续原布局。

未偏离规格；没有新增依赖、业务消息、配置、产品解释文字或代码注释。

## 生产回归（未在本地执行）

新增文件：`crates/vega_ui/src/conversation_stream/tests/issue403_conversation_underlay.rs`，已注册至既有测试模块。

| 测试 | 可证伪的生产风险 |
|---|---|
| `issue403_transcript_extends_beneath_themed_foreground` | Native / Codex × 浅 / 深主题的真实列表与 Composer 重叠，消息行确实位于遮蔽区，渐变及实色前景的真实 Quad 和绘制顺序 |
| `issue403_tail_clearance_tracks_composer_and_required_states` | 多行、图片、附件错误、队列、控制器错误、隐私状态增减及窗口缩放后，末条完整可见且原生列表项数不变；每阶段轨道在可读区居中，真实 hover 后预览与对应刻度对齐且未被遮住 |
| `issue403_footer_resize_preserves_history_anchor_and_resume` | Composer 增高与流式追加不改变历史身份和像素读位，点击回底恢复原生跟随 |
| `issue403_fade_passes_scroll_and_foreground_blocks_hidden_tools` | 渐变区域内的真实滚轮生效；把真实可点击工具标题移到 Composer 顶部内侧 4px 空白处后点击，不误触底层；模型 / 权限浮层绘制在渐变之后，菜单 Esc 关闭，添加上下文的原有内联菜单保持可达 |
| `issue403_empty_layout_and_short_overflow_navigation_remain_usable` | 空态与 utility bar 无前景渐变；短内容介于实际可读高与完整视口高之间时仍显示锚点导航 |

短内容用两条单行用户消息；当前真实行含顶部 24px、气泡上下共 16px、正文 23px、尾距 48px，合计约 111px / 条。两条约 222px，在 320px 窗口中介于可读高（扣除前景后约 150px）与全视口高之间；测试保留这两个边界断言。

用户禁止本地编译 / 测试，因此没有本地红 / 绿运行记录。上述测试调用生产渲染与 ListState；存储恢复、选区复制、长会话水合、既有菜单及权限回归由现有云端 workspace suite 一并执行，不新增外部 E2E。

## 本地快速检查证据

检查时间：2026-10-16 01:11 UTC / 09:11 Asia/Shanghai。分支 `feat/403-conversation-underlay`，基线由主 agent 更新至当时最新 `origin/master`。下列八个变更代码文件按排序后的相对路径、NUL、文件内容、NUL 逐个拼接的 SHA-256：`47c603e9e24c0c34f1473ef81ade2a7fe7bf7697cab4d37f9fa006e1ee495656`。

| 命令 / 检查 | 退出码 | 原始输出 / 事实 |
|---|---|---|
| 首轮 `cargo fmt --all -- --check` | 1 | 仅报告本卡新增代码格式差异；例如 `foreground_height: px(` 多行格式须折叠。未执行编译或测试 |
| `cargo fmt --all` | 0 | stdout / stderr 为空；修正上述格式 |
| 最终 `cargo fmt --all -- --check` | 0 | stdout / stderr 为空 |
| `git diff --check` | 0 | stdout / stderr 为空 |
| `git diff --no-index --check /dev/null crates/vega_ui/src/conversation_stream/tests/issue403_conversation_underlay.rs` | 1 | stdout / stderr 为空；no-index 的新增文件差异退出码，不是 whitespace 诊断 |
| `python3` 对本卡新增 diff 与新测试文件的静态断言 | 0 | 下列原始输出；仅文本检查，无编译 / 测试 |

```text
Added code comments: 0
Added production unwrap/expect: 0
Issue 403 GPUI regressions: 5 (not executed locally)
```

其他目录、Cargo 配置和 target 未修改；没有创建共享 target。Executor 交付时未提交、push、创建 PR、合并、部署或安装应用；后续集成与云端证据由主 agent 记录在关联 PR 和 Issue 中。

## 云端与剩余验收

本卡及现有 suite 尚未取得完整运行通过证据；首轮云端因下述测试编译错误停止，第二轮 Clippy 发现多余 `mut`，Nextest 有 3 项失败。修复及锚点布局修正待后续 CI。可用过滤命令为 `cargo nextest run -p vega_ui issue403_`，实际现有 PR gate 运行 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings` 和 `cargo nextest run --workspace`。不能把静态检查或未执行的测试标作行为通过。

### 首轮云端编译失败与修复

[PR #407](https://github.com/puzige/vega/pull/407) 的[首轮检查](https://github.com/puzige/vega/actions/runs/38833955465)中，Clippy 与 Nextest 均在新增测试的同一处编译失败，测试尚未执行。日志保留以下原始诊断：

```text
error[E0433]: cannot find `ImagePreview` in `attachments`
  --> crates/vega_ui/src/conversation_stream/tests/issue403_conversation_underlay.rs:129:36
129 |         let preview = attachments::ImagePreview::new(image, "image.png".into(), cx);
```

原因：测试作用域已有同名 `tests::attachments`，未限定的 `attachments::ImagePreview` 解析到测试模块。仅在新增测试显式导入 `crate::conversation_stream::attachments::ImagePreview` 并调用该类型；生产实现、可见性和所有断言不变。

2026-10-16 01:19 UTC / 09:19 Asia/Shanghai，本地快速复查：`cargo fmt --all -- --check` 退出 0、stdout / stderr 为空；`git diff --check` 退出 0、stdout / stderr 为空。未在本地编译、Clippy 或执行测试，未由 executor 提交或 push。第二轮结果见下节。

### 第二轮 Clippy 失败与锚点布局修复

[第二轮检查](https://github.com/puzige/vega/actions/runs/38834555020)的 Clippy 因新增测试中多余的 `mut` 失败，退出 101。原始诊断：

```text
error: variable does not need to be mutable
  --> crates/vega_ui/src/conversation_stream/tests/issue403_conversation_underlay.rs:171:9
171 |     let mut visual = VisualTestContext::from_window(window.into(), cx);
    |         ----^^^^^^
    |         |
    |         help: remove this `mut`
    = note: `-D unused-mut` implied by `-D warnings`
```

按诊断去掉该 `mut`，没有 `allow` 或门禁修改。第二轮 Nextest 随后完成，退出 100；原始汇总：

```text
Summary [176.399s] 2734 tests run: 2731 passed, 3 failed, 5 skipped
```

保留的 3 项失败及后续修复：

| 失败测试 | 原始断言 / 结果 | 证据与修复 |
|---|---|---|
| `issue403_fade_passes_scroll_and_foreground_blocks_hidden_tools` | Esc 后 `debug_bounds(menu).is_none()` 未成立 | 权限 picker 打开时已聚焦输入，却没有 Esc 路径；只在该 picker 打开时向 Composer 挂局部键盘处理，保留 IME guard，Esc 关闭并回焦输入。模型 picker 按现有生产测试契约聚焦真实模型触发器；新测试在 Esc 前后都核对焦点，并保留关闭断言及菜单名诊断 |
| `issue403_tail_clearance_tracks_composer_and_required_states` | `last.bottom() <= foreground.top() + px(0.5)` 失败 | 原测高在 Prepaint 内回写 / notify，依赖下一动画帧才刷新列表 padding；改为高度确有变化时用 `cx.defer` 在绘制完成后更新和 notify。断言强度、跟随条件和测试动作不变，新增阶段、末项、前景、视口、padding 与滚动位置诊断 |
| `issue72_message_anchor_navigation::mouse_and_keyboard_anchor_navigation_emit_real_message_ids` | rail wheel 的读位由 `(56, 26px)` 变成 `(57, 14.5px)` | 锚点点击新增 Searching 状态，增高前景；同一待校正的 padding 原本延到下一次 wheel 事件才应用。上述 defer 修复使校正进入普通更新循环。旧测试和滚轮不移动阅读位的断言完全保留 |

锁定的 `gpui-pre 0.3.4` 源码证据：`WindowInvalidator::invalidate_view` 在绘制阶段只登记 dirty view，只有 `DrawPhase::None` 才将窗口置 dirty；`request_animation_frame` 排入 `on_next_frame`，测试平台的 `schedule_frame` 仅标记待交付，普通 `run_until_parked` 不会自动交付该平台帧。`cx.defer` 在当前 effect cycle 结束后执行，届时可正常使窗口失效。修复不强制恢复 Tail、不在测试里调用 resume_tail、不增加计时循环或假帧。

旧 issue72 变化还有定量对应：fixture 第 56 项为一行 assistant，16px 正文顶距加 23px 行高共 39px；位置变化为 `39 + 14.5 - 26 = 27.5px`。Searching 条目为 12px metadata × GPUI 默认 `phi()` 行高加上下 8px 内距，约 27.416px，与半像素布局约 27.5px 相符。轨道位于 x=4–28，预览从 x=32 开始；wheel 的轨道坐标没有落入预览。这些是源码和旧日志支持的根因判断，后续云端运行仍须证实修复。

主 agent 同时发现本卡引入的锚点基准不一致：几何已扣除前景，但轨道仍按全高父容器居中，而预览使用扣除后的像素位置。现按冻结规格统一为当前可读区：轨道保留相对 `top: 50%`，额外减去前景高度的一半；预览用 `top: 0` / `bottom: 前景高度` 的当前布局容器，以 50% 留白加实际刻度偏移定位。到下边界时只收缩留白，保留预览本体。两者均不依赖上一帧视口高度的像素 top；原有宽度、刻度尺寸、菜单绘制顺序和交互保持。

只更新已由规格替代的旧“轨道在全高列表居中”条件，并在既有动态高度测试中增加初始、多行、必要状态增高、收回及窄窗口五个阶段的生产 hover / 几何断言；没有新增 fixture 或删除原断言。

2026-10-16 01:29 UTC / 09:29 Asia/Shanghai，本轮首个 `cargo fmt --all -- --check` 退出 1，仅报告新增测试 helper 两处格式差异；`cargo fmt --all` 退出 0、无输出；最终 `cargo fmt --all -- --check` 和 `git diff --check` 均退出 0、stdout / stderr 为空。本地仍未编译、Clippy、测试、提交或 push。上述修改待下一轮完整云端检查。

本轮收到 Nextest 完整结果后继续修改，再次 `cargo fmt --all` 退出 0、无输出；最终 `cargo fmt --all -- --check` 和 `git diff --check` 均退出 0、stdout / stderr 为空。没有执行本地编译、Clippy 或测试；第二轮的 2731 个通过不替代这些新增修复的下一轮 CI。

待用户手测：

1. 浅色和深色分别打开 Native、Codex 长会话，上翻历史；观察消息真实延伸至输入框后方，渐隐连续、底部和两侧无消息碎片，正文与输入框同轴。
2. 贴底增减多行输入、图片、待发送消息；改变窗口大小、展开 Workspace；末条仍可读，输入和发送入口可用。回看历史时重复操作并接收新输出，读位应保留；点击或键盘激活“回到底部”恢复跟随。
3. 在过渡区滚动，点击输入框边缘；不误触被遮住的工具或消息。操作模型、权限、添加上下文、文件选择和待发送菜单，验证 Esc、外部点击、Tab 和回焦。
4. 检查定位失败、电脑操作权限 / 暂停提示、隐私检查错误与提交面板仍可见可操作。空任务保留欢迎内容和 utility bar。

LIMIT：本地仅格式及静态证据，未启动或截图真实 Vega。前景高度在实际布局测量后只对变化进行延后更新；首次与多行高度变化的实际绘制、边缘像素和设备输入须由 CI / 用户真实应用确认。没有存储迁移。回滚为回退本卡合并提交；Issue、Project 和分支 / worktree 的交付状态由主 agent 管理，用户手测前不关闭或清理。

## 用户手测退回：活动标题穿入 Composer

此前 [PR #407](https://github.com/puzige/vega/pull/407) 最终 head `8fc744262a3373bd6a3c8db681cb8e3030202a16` 的[云端检查](https://github.com/puzige/vega/actions/runs/38836312543)通过，Clippy 成功，Nextest 原始汇总为：

```text
Summary [201.464s] 2735 tests run: 2735 passed, 5 skipped
```

随后 squash 合入 master `1e90dbe75d60d2402bc658ee8d77e799896c7073`。用户手测截图仍显示真实工具活动图标、标题绘制在 Composer 内，故 [Issue #403 退回 In progress](https://github.com/puzige/vega/issues/403#issuecomment-6207184088)。此前云端通过不等于用户手测通过；不改写上述失败和修复历史，不公开用户截图内容。

本轮分支 `feat/403-composer-occlusion`，基线为上述 master。第一阶段只新增真实生产回归和本节记录，生产代码保持不变，由主 agent 开 PR 留云端失败证据后再决策修复。

| 用例 | 生产前置与操作 | 判据 | 第一阶段状态 |
|---|---|---|---|
| R1 live | ConversationEvent 生成含两个工具的运行阶段；浅深主题，滚入 fade 与 Composer | 存在实际重叠，真实标题 wrapper 的背景 Quad 早于 fade / 实色前景 / shell | 已新增 / 待云端红证据 |
| R1 final | ConversationEvent 确认 Final 并完成会话，形成完成聚合标题；同上操作 | 相同的真实绘制顺序契约；不使用假标题或手工 deferred 元素 | 已新增 / 待云端红证据 |
| R2 | 标题移入输入框空白边缘后点击，再滚回可读区点击 | 被遮住时不展开，回到阅读区域后可展开真实活动内容 | 已新增 / 待云端验证 |
| R3 | 合法菜单、动态高度与锚点 | 保留已有生产回归 | 已有 suite / 待云端验证 |
| R4 | 应用级后绘制遮罩 | 后续实现不得把整个前景抬高到应用遮罩之上 | 后续组合根回归 / 修复评审 |

证据边界：精确标题 bounds + `bg_base` Quad 定位同一真实标题子树的绘制阶段。标题文字与图标属于该子树，但 Quad 顺序不冒充字形或真实像素验收。保持本地禁止编译 / 测试，仅做快速格式和差异检查；第一阶段不修生产代码，不提交、push 或操作 CI。

新增测试均在 `crates/vega_ui/src/conversation_stream/tests/issue403_conversation_underlay.rs`：

- `issue403_live_deferred_header_paints_beneath_foreground`：真实两个工具调用形成 `run commands` 阶段，断言真实图标和标题在同一 header 内，浅深主题分别覆盖 fade 与 shell 的有效重叠。
- `issue403_completed_deferred_header_paints_beneath_foreground`：同一事件流经 Final 元数据和 MessageFinished 形成折叠完成汇总，执行相同绘制顺序断言。
- `issue403_foreground_occludes_real_run_header_clicks`：运行和完成两态均点击 shell 顶部空白边缘与标题重叠处，断言不展开；滚回可读区后点击真实标题，断言可展开。点击位置避开文本输入区域，避免由输入控件吞事件伪装前景遮挡。

预期旧生产代码在两项绘制测试的 `header_index < fade_index` 条件失败：真实标题 wrapper 经 priority 1 全局延后，出现在普通绘制的 fade / surface / shell 后方。测试先断言实际位置与重叠并查找精确背景 Quad；找不到真实标题、没有重叠或主题不匹配都会单独失败，不能冒充已检出层级缺陷。此为源码推断，尚未获得本轮云端失败输出。

本阶段本地实际执行的轻量检查：

```text
cargo fmt --all -- --check
首次 exit 1：仅报告本次新增测试区域的格式差异；按输出手工修正。
最终 exit 0
(no output)

git diff --check
exit 0
(no output)
```

NOT RUN：本轮新增三项测试、Clippy、Nextest 与应用像素手测均未在本地执行；主 agent 后续通过云端 PR Check 留存红证据。当前仅测试和交付记录有本阶段改动，规格改动由主 agent 持有，生产代码未修改。未添加依赖、产品文案或调试 selector，保留原有全部断言。

后续用户手测：让含多条命令的运行阶段及完成汇总分别滚过输入框上沿，浅深主题检查图标、文字随真实渐隐被遮盖；输入区、侧边和底部无标题碎片。遮住时点按不展开，滚回阅读区后能展开并吸顶；模型 / 权限等菜单及命令面板等应用级遮罩仍在上层。当前回归仅观测同一真实子树的背景和命中，字形像素及外部应用集成仍待用户手测。

### 工作流更新后的实际交付边界

回归准备完成时，最新 master 合入 PR #415（`6a2363f8`），明确手测失败的 Bug 由 Codex 分析并发布具体方案，Ready 等 Vega 领取执行。主 agent 已将本轮分支 rebase 到该基线，未出现冲突；上游仅改角色工作流文档，没有 UI 或测试基线变化。

因此本轮不再启动测试草稿 PR，也不由当前子 agent 实施生产修复。前述“主 agent 后续开 PR 留红证据”计划由冻结规格最后的“执行交接 v1：Codex → Vega”取代：发布现有三项回归及精确到 `StickyDisclosure::prepaint` / `paint` 的局部绘制方案，Vega 补生产修复与 R4 组合根回归后交回 Codex。不能将源码推断写成已运行红测试；本轮三项、R4 与旧 suite 的执行结果均待后续 Actions。

当前生产代码与 `origin/master` 一致，只有本卡测试及两份规格/交付文档变化。没有开本轮 PR、CI、合并、打包或更新应用；没有公开用户截图、会话原文、配置或本机数据。子 agent 已停止写入，主 agent 完成方案发布后停止该卡实现；下一执行者为 Vega，当前执行者为待领取，不声称 Vega 已开始工作。

交接后继续保留本卡原分支、新修复分支与 worktree，Issue 保持开放，Project Ready 等领取。后续 Vega 回报只改变执行阶段，保持 In progress 并交回 Codex；通过最新 head 的 required check 合并后才回到 In review。实际 handoff commit 与可访问链接写入 #403 最新交接评论，领取者必须核对该版本。

2026-10-16 03:23 UTC / 11:23 Asia/Shanghai，rebase 后最终快速复查：`cargo fmt --all -- --check` 退出 0、无输出；`git diff --check origin/master` 退出 0、无输出。文本静态审查退出 0，原始输出：

```text
Changed production files: 0
Added code comments: 0
Added ignored tests: 0
Added GPUI regressions: 3 (not executed)
```

未运行 Vega、编译、构建、Clippy、任何测试或打包。当前没有本轮 CI 红/绿结果，新增 R4 组合根测试待 Vega 按方案补充；发布 handoff 不代表修复已实现或 Vega 已领取。

### 用户授权 Codex 继续第二阶段修复

用户随后明确要求“你直接开始修复”，主 agent 已在[同卡评论](https://github.com/puzige/vega/issues/403#issuecomment-6207499799)记录本轮覆盖角色安排，并将 Project 恢复 In progress。此前 Ready / 等待 Vega 的历史保留；本轮按规格末尾授权由 Codex 主会话组织、专用子 agent 独占实现，主会话审查及集成。

本轮实现基线 `b4864e2a` 已包含第一阶段三项真实回归与已发布的具体方案，已由主 agent fetch / rebase 至 `origin/master` 的 `6a2363f8`。允许范围仅 `sticky_disclosure.rs`、本卡测试及本记录，规格由主 agent 持有。R1–R3 的已有断言全部保留；R4 按实现前冻结的以下用例落实到测试：

| 用例 | 前置与操作 | 判据 | 状态 |
|---|---|---|---|
| R4 组合根 | 真正的 ConversationStream 使用现有事件形成 live / final 标题，将标题滚入 Composer，随后挂载普通绘制的不透明兄弟遮罩 | 真实标题与 Composer 有效重叠；标题背景 / fade / surface / shell 均在遮罩之前；点击重叠处由遮罩收到，底层标题不展开 | 已新增 / 待 CI |

修复动作按冻结方案：保留原 header 与自然布局、当前帧 placement / StickyState / 连续区域和 viewport 的交集；sections prepaint 后以相对元素偏移普通 prepaint 原 header，paint 继续先 sections 后原 header。ClippedHeader 两阶段 mask 保留，不将整个前景升为全窗口 deferred。无接口、存储、依赖或产品文案变化。

本阶段修改文件与实际动作：

- `crates/vega_ui/src/conversation_stream/sticky_disclosure.rs`：将 `header.take()` / 全窗口 priority 1 `defer_draw` 改为 `header.as_mut()` 和 `with_element_offset(point(px(0.), offset), ...)` 内局部 prepaint；paint 在 sections 后绘制同一个 header。未改变 placement、StickyState、自然尺寸或 ClippedHeader 的 prepaint / paint mask。
- `crates/vega_ui/src/conversation_stream/tests/issue403_conversation_underlay.rs`：新增 `issue403_later_application_overlay_covers_conversation`。浅深主题、live / final 两态均使用原事件序列，断言实际重叠与遮罩挂载前后标题位置不变；检查真实标题背景、fade、surface、shell 的 Quad 均早于普通后绘制遮罩；实际点击由遮罩计数，底层标题保持折叠。仅抽出共用事件和支持两种测试 root 的 bounds helper，原三项回归的有效断言保留，不需改 tests/mod.rs。
- 本 delivery 追加授权、实现、检查与未执行证据。主 agent 的规格修改保留，未由实现 agent 修改。

锁定 `gpui-pre 0.3.4` 的静态核对：`with_element_offset` 在当前元素偏移上累加；Drawable 在 prepaint 保存含偏移的 bounds，paint 使用保存的 bounds，因此 paint 无需再次累加偏移。原 mask 继续在 ClippedHeader 两阶段生效；标题位于所属展开内容之后、会话普通前景之前，保留原 deferred 菜单路径，不影响后绘制的普通应用级遮罩。

本阶段从 2026-10-16 03:37:34 UTC / 11:37:34 Asia/Shanghai 的实施记录开始；本地仅执行快速检查。首次格式检查 3.453 秒退出 1，输出仅为本次 R4 测试新增区域的格式差异，已按输出手工修正。源码修改后的最终原始结果：

```text
cargo fmt --all -- --check
exit 0
(no output)
wall time: 3.228s

git diff --check
exit 0
(no output)
```

NOT RUN：本地未运行 Vega、编译、构建、Clippy、test / nextest、打包或安装。本阶段未提交、push、开 PR、读取或轮询 CI；云端改前红证据和修复后结果由主 agent 在对应 head 上核实。R1–R4、旧 sticky S01–S06、合法菜单、动态高度、消息锚点、滚轮和阅读保护均待 Actions 验证，不能沿用 #407 的通过结果。本阶段偏离冻结技术方案：无。

LIMIT：R4 是真实 ConversationStream 与普通兄弟遮罩的进程内组合根回归，不代表已操作真实命令面板 / 删除确认 / 环境面板。Quad 顺序不替代标题字形与图标的像素手测；窗口缩放、吸顶末端和菜单交互仍由既有 suite 及后续用户手测确认。用户明确通过前不关闭 Issue、不清理分支 / worktree；本轮回滚方式仍是回退后续修复的 squash 合并提交。

### 修复前云端失败证据

[PR #418](https://github.com/puzige/vega/pull/418) 的[改前运行 38846335746](https://github.com/puzige/vega/actions/runs/38846335746)验证远端仅回归提交 `b4864e2ab0c406eb6b531c9efefde87e00421e46`。生产代码仍为旧实现，Clippy（含 fmt）成功；Nextest 退出 100，只有本轮新增的三项回归失败，原始汇总：

```text
Summary [ 194.181s] 2738 tests run: 2735 passed, 3 failed, 5 skipped
```

运行阶段与完成汇总均完成实际几何和背景定位后，失败于冻结的绘制顺序；点击回归实际展开了被 Composer 遮住的标题：

```text
run-activity-toggle overlapping conversation-bottom-fade: real header quad #23 must precede fade #15, surface #17, shell #19
run-stage-toggle-0 must not expand through the Composer
run-stage-toggle-0 overlapping conversation-bottom-fade: real header quad #22 must precede fade #15, surface #20, shell #21
```

这不是 fixture 或编译失败；三项均命中真实生产行为的预期失败点。旧 suite 的 2735 项通过只描述改前 head，不替代本轮修复后的门禁。当前局部绘制补丁和新增 R4 正待推送后由新一轮 CI 验证。

原始日志临时保存，不入仓库；Nextest SHA-256 为 `1516b542fb3f7548c010858c4dfdbec20fd87e6d6956345950a55d1eca116a30`，Clippy SHA-256 为 `a5bdb3b5c8067115ba6f8fdce1415168902cd8d9ea6390aa4264e0cff640c1c9`。本地仍未运行编译、测试或应用。

# Issue #357 · Markdown 链接的可见性与点击

来源：[Issue #357](https://github.com/puzige/vega/issues/357)（标题「[Bug] Markdown 不能正确点击链接」，正文为空）。
2026-10-14 用户报告：正文里的 Markdown 链接「既不展示链接，也不能点击，不会展示高亮」。

本页只处理**已有的 Markdown 链接节点**在会话正文中的可见性与点击行为；不新增解析语法、不改用户原文、不改选择与复制契约。

## 已核实事实（源码级）

1. 解析侧链接是完整建模的：`Inline::Link { url, title, spans }`（`crates/vega_markdown/src/nodes.rs:23-29`，由 `Tag::Link` 转换于同文件 `:213-225`），包含 `<url>` autolink 与 `[label](url)` 两种来源。
2. UI 侧的样式枚举有 Link 变体：`SpanStyle::Link`（`crates/vega_ui/src/conversation_stream/model.rs:98-110`），注释写着「Link label: underlined, secondary color」。
3. **URL 在展平时被丢弃**：`flatten_inlines` 用 `Inline::Link { spans: inner, .. }` 只取标签 spans（`model.rs:295`），`StreamSpan`（`model.rs:159-164`）只有 `text` 与 `style`，模型里不保留目标地址，渲染层因此无法知道点击目标。
4. **下划线实际不可见**：`apply_span_style` 对 Link 只做
   `style.underline = Some(UnderlineStyle::default()); style.color = colors.text_secondary;`
   （`crates/vega_ui/src/conversation_stream/render_rows.rs:1071-1074`）。
   `UnderlineStyle` 派生 `Default`，字段为 `thickness: Pixels(0.)`、`color: None`、`wavy: false`（`gpui-pre-0.3.4/src/style.rs:820-837`）。GPUI 绘制时 `snap_stroke(px(0.))` 经 `round_stroke_to_device_pixel` 的 `logical == 0.0` 分支返回 `0.0`（`src/util.rs:145-151`），`paint_underline` 生成的 underline primitive 高度为 0 且颜色取 `Hsla::default()`（全透明）（`src/window.rs:4373-4402`）。因此**画不出任何下划线**。
5. 链接标签因此只比正文略灰（`text_secondary` #676767 vs `text_primary` #191C1F），没有下划线、没有链接色，视觉上与普通文字无异 —— 对应用户说的「不会展示高亮」。
6. **没有任何点击路径**：会话正文的文本渲染（`render_line_selectable` / `render_line` / `render_table*`）只构造 `StyledText` + runs，不注册 `on_click`，也没有命中测试。`Inline::Link.url` 在事实 3 已被丢弃，即便注册点击也无目标地址。仓库里唯一的 `cx.open_url` 调用都在设置页与 Computer Use 引导（`crates/vega_ui/src/settings/{mcp,agents}.rs`、`conversation_stream/computer_use.rs`），Markdown 正文从未接入。
7. 两条渲染路径都存在且都覆盖正文：可选文本路径 `render_line_selectable`（会话正文与思考全文，`render_rows.rs:764-910`）与普通路径 `render_line`（文件预览、更新说明，`render_rows.rs:1113-1234`）；表格另走 `render_table_selectable`（`:912-992`）与 `render_table`（`:1236-1294`）。
8. 命中测试与坐标换算已有现成能力：`TextLayout::index_for_position` 已在 `SelectableStyledText` 的多击选词中使用（`selection.rs:437`），`position_for_index` 已用于选区绘制（`selection.rs:477-482`）；`window.insert_hitbox` / `set_cursor_style` / `CursorStyle::PointingHand` 均可用。
9. `cx.open_url(&url)` 是既有外部打开入口（`gpui-pre-0.3.4/src/app.rs:1500`），测试平台记录打开结果（`opened_url()`，`src/app/test_context.rs:369-372`）。

## 根因

链接在解析层完好，但在 **模型 → 渲染** 的映射里丢了目标地址（事实 3），同时下划线样式退化成零厚度不可见（事实 4/5），且正文渲染从未注册点击（事实 6）。三者叠加即「不展示链接、不能点击、无高亮」。

## 冻结契约

1. **C1 保留目标地址**：链接目标地址随行内模型传到渲染层；相邻、不同目标的链接不得被合并成一段（`coalesce` 只在目标地址相同或都为空时合并）。标签文字本身仍是唯一可见文本，不把 URL 追加到正文、不改写用户原文。
2. **C2 可见的链接样式**：链接标签使用主题 `accent` 色（Light #3478D8 / Dark #8FC7FF），下划线**1px 实线**、颜色取同色（`thickness: px(1.)`、`wavy: false`）。不使用 `UnderlineStyle::default()`。字号、字重、字体、行高与正文一致，不因链接改变行盒高度与换行。
3. **C3 全容器覆盖**：段落、标题、列表项、引用与表格单元格内的链接都适用 C2 与 C4；两条渲染路径（可选文本 / 普通）行为一致，共用同一映射与命中实现，不产生样式分叉。
4. **C4 点击打开外部地址**：鼠标左键在链接标签的命中区域内按下并抬起（无拖选）时，用 `cx.open_url` 打开该地址。仅 `http`、`https`、`mailto` 三种 scheme 可打开；其它 scheme 与相对路径不打开、不弹窗、不报错，保持纯文本行为。只有用户主动点击才打开，任何流式到达、悬停、恢复历史都不自动打开。
5. **C5 不破坏选择与复制**：拖动选择、双击/三击选词、右键菜单、`Cmd-C` 与「复制原文」语义完全不变。拖选（存在活动选区）后释放不触发打开；点击链接不改变、也不清空既有选区行为以外的任何状态。可见复制文本仍只含标签文字，不含 URL（#147 契约不变）；「复制原文」仍保留完整 Markdown 源（含 URL）。
6. **C6 指针反馈**：指针悬停在链接命中区域内时为 `PointingHand`，离开恢复；悬停不改变字形、下划线几何或布局（不做按 hover 重新 shaping，避免每帧重排与选择失效）。不新增常驻提示条或 tooltip 噪声。
7. **C7 范围与不变式**：不改解析器与 `vega_markdown` 的节点语义，不改运行状态、协议、持久化、配置、全局字号与既有主题色值（只新增对 `accent` 的引用），不加依赖、不加非测试 `unwrap/expect`、不加代码注释。行内代码底与表格列宽/估高（#296）不变；流式未闭合语法与历史恢复（#119）不变。

### 非目标

- 裸 URL 自动识别（GFM autolink literal）：当前解析选项不启用该扩展，本轮不实现，也不自写扫描器；`<url>` autolink 与 `[label](url)` 已覆盖。
- 链接的键盘 tab 焦点与快捷键打开：正文当前是可选文本语义，不新增 tab stop；如需键盘入口另开卡片。
- 悬停时的颜色变化动画、预览卡片、复制链接地址等附加交互。
- 其它渲染宿主（文件预览、更新说明）保持 C2/C3 的视觉一致；其点击行为与正文同规则，不额外扩展。

## 实现前验收矩阵

| ID | 需求/风险 | 前置状态 | 实际操作 | 预期可观察结果 | 测试层级 | 证据 | 状态 |
|---|---|---|---|---|---|---|---|
| T01 | C1 地址保真与不误合并 | `Inline::Link` 三种来源（inline / autolink / 带标题） | 展平到 `StreamSpan` 后比较 | 每个链接 span 带正确 URL；相邻同 URL 合并、不同 URL 不合并；顺序与标签文字逐字不变 | vega_ui 单元测试 | NOT RUN |
| T02 | C2 样式映射 | 浅/深主题 | `apply_span_style(Link)` | color=`accent`，underline thickness=1px 且 wavy=false，color=`accent`；字号/字重/行高不变；先红复现 `UnderlineStyle::default()` 零厚度 | vega_ui 单元测试 | NOT RUN |
| T03 | C2/C3 实际绘制 | 段落、标题、列表项、引用、表格 cell 各含链接；两条渲染路径 | GPUI 画帧后检查 scene | 每条链接产生 underline primitive，宽度等于标签文字范围，不覆盖相邻普通文字；两条路径几何一致 | GPUI visual 测试 | NOT RUN |
| T04 | C4 点击语义 | 同一段落含链接与非链接文字 | 左键点击链接中心 / 非链接文字 / 拖选后释放 / 双击选词 / 非白名单 scheme | 仅链接点击打开且 `opened_url` 等于目标；其余不打开 | GPUI visual 测试 | NOT RUN |
| T05 | C6 指针反馈 | 悬停链接与非链接区间 | 移动指针 | 链接区间 `PointingHand`，其余不变；字形与布局无变化 | GPUI visual 测试（能力受限则用户手测） | NOT RUN |
| T06 | C5/C7 回归 | #147 选择复制、#119 解析保真、#296 行内代码与表格 | 既有定向用例 | 原有断言全部保留通过，不放宽 | 现有定向回归 | NOT RUN |
| M01 | 端到端可读可点 | 合并后的 master，浅深主题，真实模型输出 | 手测含 `[标题](url)` 与 `<url>` 的消息 | 链接有色有下划线，点击打开默认浏览器，拖选复制只含标签文字 | 用户手测 | NOT RUN |

## 执行计划

- 复用本卡 worktree（`~/Workspace/worktrees/vega-357-markdown-links`），从最新 `origin/master` 拉 `feat/357-markdown-links`。主控负责规格、审查、集成与 PR；实现由专职子 agent 独占。
- 预期改动面：`crates/vega_ui/src/conversation_stream/{model.rs,render_rows.rs,selection.rs}` 与必要的新内部模块（链接命中/样式共享），以及本卡相关测试文件；不扩公开 API、不动 `vega_markdown`。
- 顺序：先写有辨识力的 T01/T02/T04 并保留红测（当前实现必然失败），再实现 C1/C2/C4，再补 T03/T05 与 T06 回归。
- 本地只做快速静态检查与差异检查；本卡功能点测试交云端 `pr-check`（Vega 本地验证限制优先）。不本地跑 workspace 全量、不打包、不安装应用。
- 交付记录保存精确命令、原始输出与 hash、与 spec 的偏离（必须为无）、未测项。回滚方式：整体回退本轮共享 Markdown 链接展示改动。

## 沿用的实现取舍

1. 链接色取 `accent`（品牌蓝/冰蓝）+ 1px 下划线，而非保持灰色只加下划线。
2. 不做键盘 tab 焦点（保持正文可选文本语义），键盘入口另开卡片。
3. 不做裸 URL 自动识别，本轮只覆盖已有链接节点。

## 2026-10-17 继续实施修订（v2）

用户本轮明确要求实现 #357，继续已有 worktree，并由专职子 agent 独占代码实现；Codex 主会话负责方案、审查和 PR/CI 集成。旧交接中仅产出 handoff、不得同步或提交的会话限制已由本轮实施请求取代。任何仓库改动前仍须 fetch 并更新基线；本地不运行、编译或测试 Vega。

C5 的「选择与复制语义不变」具体指沿用窗口 `TextSelectionLayer` 的普通点击、拖选和多击语义：普通点击清除此前选区是既有行为，链接无需额外保留旧选区，也不能因为点击前有选区而拒绝打开。链接层不得抑制、替换或恢复窗口选择事件。旧 handoff 将保留此前选区视为必须条件的推论撤回，不修改窗口根节点或 GPUI 依赖。

| 前置/操作 | 选择结果 | 打开结果 |
|---|---|---|
| 无选区，单击链接 | 保持普通单击语义 | 打开一次对应目标 |
| 已有选区，单击链接 | 窗口按既有行为清除旧选区 | 仍打开一次对应目标 |
| 从链接拖选、拖至链接、离开后返回 | 保留正常拖选及复制标签 | 不打开 |
| 双击/三击链接文字 | 保留选词/选段 | 不打开；首击待定动作须能取消 |
| Shift 扩选、右键、点击非链接、按下后移至不同链接 | 保留既有行为 | 不打开 |

执行次序：先修复未完成草稿的字段/构造点与事件状态机，取消旧选区快照比较；再核对普通/可选路径、inline-code fragment、表格及唯一 ID；最后补齐模型、样式、真实生产渲染/鼠标事件的进程内回归用例。延迟打开若用于区分多击，须在后续按下、多击、失焦或组件卸载后失效，不能保留过期任务。只增加白名单 scheme 判断所需逻辑，不自建完整 URL 解析器或增加依赖。

当前锁定 GPUI 没有公开系统多击时限 API。本轮用命名的 500ms 仲裁窗口区分单击与多击；后续按下必须立即使此前待打开动作失效。超出此窗口的两次点击视为独立点击；不宣称与系统自定义的更长双击间隔完全一致，不为读取该设置引入平台依赖。

点击允许 4px 内的指针微移：`dx² + dy² > 4² = 16` 时锁定为拖动，离开同目标链接区域也锁定取消，返回不恢复；抬起时存在非空可见选文仍不得打开。1px 水平微移的距离平方为 1，小于 16，且没有选中文字时应作为普通点击。不得用坐标完全相等或仅 `has_selection` 标志拒绝这类有效点击。

T03/T05 证据边界：锁定 GPUI 的公开测试 API 可读取文本布局与 painted quads，但不公开 underline scene primitive 或当前光标读取入口。自动测试应验证生产 run 的链接样式、实际布局和交互命中，不能以 helper 单测冒充像素绘制或光标验收；真实下划线外观与手形反馈由 M01 用户手测核实，不新增公开测试 API 或真实外部应用 E2E。

Actions 验证命令：`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo nextest run --workspace`。本地只运行格式及 diff 静态检查，所有自动测试状态在云端执行前为 NOT RUN。M01 始终由用户在合并后的版本手动验收。

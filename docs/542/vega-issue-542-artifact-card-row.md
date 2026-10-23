# #542 产物卡片：参考 WorkBuddy / ChatGPT App 收敛为「回合结束聚合」

## 需求与用户裁决

用户在实际使用中反馈产物卡片"较丑"，初始诉求为"临时禁用或优化样式"。2026-10-22
用户对方向追问的裁决原文：

> 先看一下 Workbuddy 或者是 ChatGPT APP 是怎么实现的，我们参考他们的样式，优化一点，还有他们的逻辑。

用户随后指出参考实现的关键时序差异：

> 还有一个点是他们的这个菜单出现，它是在一轮对话结束之后才出现吧？

该判断经源码核实成立（见「出现时机」一节）。用户最终裁决交付范围：

> 完整对齐参考

因此本卡交付 **样式 + 出现时机 + 聚合逻辑**，不采用"临时禁用展示"。禁用卡片会连带
失去预览入口与打开入口；参考实现的共识不是移除产物，而是"运行中不占正文、回合结束后
在末尾聚合成紧凑卡片"。

范围限定在对话流中的产物卡片展示与交互。不改文件写入/编辑权限、产物捕获校验、
Git 边界、数据模型安全投影，不顺带扩展跨项目/worktree 产物识别。

## 参考调研（本机静态读取应用包，非猜测）

两个参考应用均为 Electron 打包，结论来自其 `app.asar` / `app.asar.unpacked` 内的
真实样式与组件源码，未使用屏幕录制或 CUA。

### WorkBuddy（WorkBuddy.app 5.8.3）

- 样式：`renderer/assets/src-CBaV_A3S.css`
  - `.cb-rounded-file-change`：**固定 32px 单行**、`border-radius: 8px`、
    `padding: 0 12px`、`gap: 8px`、`justify-content: space-between`、
    `background: var(--cb-file-change-row-bg)`（浅色 `#f7f7f7`）、`overflow: hidden`。
  - `.cb-rounded-file-change__main`：`flex: 1`、`gap: 8px`、`min-width: 0`。
  - `.cb-rounded-file-change__icon`：`16×16` 文件类型图标（按扩展名解析，见
    `resolveIconName`，取 basename 的扩展名）。
  - `.cb-rounded-file-change__name`：**等宽字体**、`font-size: 14px`、
    `white-space: nowrap`、`text-overflow: ellipsis`、`min-width: 0`。
  - `.cb-rounded-file-change__diff`：`+N`/`-N` 使用 10px 等宽字体，
    颜色 token `--cb-diff-added-color` / `--cb-diff-removed-color`。
  - `.cb-rounded-file-change__hover-actions`：**默认 `display: none`**，父级
    `:hover` 时才 `display: inline-flex`；同时 `.cb-rounded-file-change:hover
    .cb-rounded-file-change__diff { display: none }`——**悬停时用操作按钮替换
    diff 统计**，两者不并排。
  - 状态：`--accepted` `opacity: .7`、`--rejected` `opacity: .6`；
    `__pending` 用 12px 次要色文本表达"生成中"。
- 逻辑：`renderer/assets/src-0FGg1seX.js`（`FileChangesPanel`）
  - 一轮内**多个文件汇总**：`cb-snapshot-top` 只放一行摘要
    （`fileChanges.filesCount` 计数 + 折叠 chevron + 生成中状态），
    逐文件行放在 `cb-snapshot-content` 内，`max-height: 280px` 可滚动
    （`--ardot` 变体 320px）。
  - 展开区顶部有累计 `+N -N`（`cb-snapshot-cumulative`）。
  - 操作按钮（Keep / Discard / View Diff）在**摘要行**右侧，`size: small`
    （`.cb-button--small` 高 22px），不在每个文件行重复。

### ChatGPT App（ChatGPT.app 26.1016.61520）

- 内联评审卡：`webview/assets/inline-file-diff-review-57a17cb059f2.js`
  - 非 compact 形态为「单文件头部 + diff 视口 + 底部操作条」；
    操作条 `border-t`、`justify-end`、`px-4 py-2`，按钮 `size: toolbar`，
    顺序固定为 **wrap 切换 / Edit / Reject / Accept**，全部带 `aria-label`
    （`inlineFileDiffReview.edit` / `.reject` / `.accept`）。
  - 超限保护：内容 > 256 KiB 或 > 5000 行时只显示
    "Change is too large to preview"，仍保留操作按钮。
  - compact 形态：`max-h-80 overflow-y-auto rounded-lg border`，底部操作条
    `sticky bottom-0`。
- 汇总文案：`webview/assets/conversation-markdown-cfdd0a9a3dc4.js` 的 `Je()`
  - 一轮工具活动摘要形如 `Created N files, Edited N files, Ran N commands`；
    数量为 1 时用单数，为 0 时该段整体省略（`X()` 返回 `null`）。
  - **没有任何"来源标签"**（如 agent artifact / workspace change）出现在正文。

### 两条共同结论（本卡采用的"逻辑"）

1. **产物在正文里只占一行**：文件图标 + 等宽文件名 +（可选）diff 统计，
   高度固定约 32px，路径省略而非换行。
2. **次要操作不常驻**：多个打开目标收进菜单或悬停态，正文不出现整排编辑器按钮；
   来源/内部标签不进入正文。

### 出现时机（决定性差异，本卡新增范围）

两个参考应用都**不在运行中出产物卡**，而是等回合结束后才出现。

**WorkBuddy** — `renderer/assets/main-content-core-ghO_qqAx.js` 的
`assistantMessageSlot`（两处挂载点条件一致）：

```js
if (!isLastMessageOfRequest || !isMessageEnd || isSessionActive || Um || !isLastAssistantMessage) return null;
```

即必须同时满足：本轮请求的最后一条消息 + 消息已结束 + **会话不在运行中** +
是最后一条 assistant 消息。卡片本体是 `ArtifactSlotPanel`
（`artifact-slot-panel-D3lczJVR.js` + `artifact-slot-panel-BsA1UFez.css`）：

- `.artifact-slot-panel { margin: 4px 0 8px }`，网格 2 列 `gap 12px`，
  单张时 `--single` 变为一列；最多 `maxDisplayItems: 6` 张。
- `.artifact-slot-panel__card`：**高 64px**、`radius 16px`、`padding 8px 12px`、
  `gap 8px`；图标 28×28；标题 15px/600，副标题 13px 三级色（即截图里的
  `914 B` / `270.5 MB`）。
- `.artifact-slot-panel__card-actions { display: none }`——操作只在 hover 出现。
- 超出时底部一行按钮：i18n key `artifactSlot.viewAll`，文案
  `查看所有产物 ({count})`，`count` 取**全部产物数**（`allArtifacts`），
  不是当前展示数。

**ChatGPT App** — `webview/assets/local-conversation-turn-*.js`：

```js
za = !q && gr != null && !wr && ...   // q = isTurnInProgress
```

回合末尾资源卡 `Sc` 只在非进行中出现；运行中的 `patch` 不建卡，而是作为
`patch` 活动条目进 agent-activity 组（`agent-activity-item-*.js` 的
`case 'patch'`）。卡片结构为「单文件行 + `Show {count} more`」：

```js
resources.filter(Ec); ... e.slice(0, Bi(e.length))
...(0, jsx)(G, { id: `localConversation.endResource.showMore`,
                defaultMessage: `Show {count, number} more` })
```

### 第三条共同结论

3. **运行中不出产物卡，回合结束才在末尾聚合**：产物不是过程行的一部分，而是
   回合结果。超出展示上限时给一个"看全部"入口，而不是把全部条目铺进正文。

## 现状与差距

Vega 当前 `crates/vega_ui/src/artifact_card.rs`：

- `row_count() = 2 + preview_lines + error`，即**恒定两行**：
  - 第 0 行：文件标签（等宽）+ 右侧来源标签 `agent artifact` / `workspace change`；
  - 第 1 行：`Preview` + `VS Code` + `Cursor` + `Zed` + `Terminal` + `Default`
    + `Finder` **七个常驻按钮**。
- **时机与参考实现相反**：`observe_artifact_event` 在 `ToolCallFinished` 后排队
  capture，`finish_artifact_terminal` 拿到 `result.captured` 就调
  `apply_artifact_card`，即**每个 `Write`/`Edit` 写完立刻在过程里插一张卡**，
  运行中一直显示。
- 预览走 `ArtifactPreviewRequested` → `vega/src/window/artifact.rs`
  `request_artifact_preview` → `self.workspace.open(TabKey::Artifact(card_id))`，
  即**在右侧 workspace 面板打开标签页**，不是内联展开。
- 同一文件多次修改会出现多张卡（`existing_call_id` 只按 call id 去重）。

差距有两层：正文被两行 + 七个按钮占据；且**时机不对**——参考实现运行中不出卡、
回合结束才在末尾聚合，Vega 是每次写入都往过程里插卡。来源标签是内部实现词汇
（违反仓库 UI/UX 规则"内部字段名、配置来源不直接拼成副标题或状态条"）。

## 本卡方案（v2.1：v2 + 本轮用户明确裁决）

本轮实现前核对实际 API 后，用户明确选择：

- **预览也删除来源标签**：对话聚合卡与 workspace 预览都不显示来源标签；
  删除 `source_label` / `source_color`。只移除预览标题的来源内容，不改
  `ArtifactCard::row_count()`、预览操作行或内容行。
- **允许独立错误条目**：首次捕获失败、尚无投影时，聚合区显示真实的独立错误
  条目，不伪造文件卡、文件能力或大小；错误不受六个文件的折叠限制，明确计入行数。

下文按这两项裁决修订，其余 v2 验收保持不变。测试与生产代码均尚未开始。

参考证据边界纠正：上文是前轮调研记录，不应把文件变更评审行、diff 或输出卡当作
同一种组件；WorkBuddy 两个挂载分支的条件并不相同，ChatGPT 的 `!q` 直接证明的是
完成态 diff，而不是所有资源卡。上文「两处条件一致」「两个应用所有产物均只在回合末尾
显示」不作为已证实事实。Vega 的**回合末尾聚合**来自用户明确选择的产品要求，
不依赖这些过度概括；本轮不重新解包或重新开展竞品调研。

### 出现时机：运行中不出卡 → 回合结束聚合

与参考实现对齐，分两个阶段：

1. **运行中**：捕获链路照常执行（`Write`/`Edit` 的产物能力、预览可用性、错误分类
   全部不变），但**不插入对话流条目**。卡片实体仍创建并记录在
   `artifact_controller` 里——`TabKey::Artifact` 预览标签页与
   `request_artifact_preview` 的 fence 校验都依赖它，不能因为不渲染就丢弃。
2. **回合结束**（现有 `finish_run_activity` 的三个调用点：`MessageFinished` /
   `Interrupted` / `Error`）：把本轮捕获到的产物聚合成**一个**
   `StreamEntry::ArtifactGroup`，插入到该回合最后一条 assistant 条目之后、
   摘要卡之前。

聚合行模型：

```text
file_rows = if expanded { files.len() } else { min(files.len(), MAX_VISIBLE) }
overflow = files.len() - file_rows
ArtifactGroup::row_count() = file_rows + usize::from(overflow > 0) + error_rows

row 0..k   : 单个产物行（图标 + 文件名 + 大小）
row k      : "Show N more"（仅 overflow > 0 时）
其后       : 未解决的真实错误条目，始终可见，不受文件折叠影响
```

- `MAX_VISIBLE = 6`，沿用参考实现的 `maxDisplayItems: 6`。
- 同一文件多次修改**合并为一行**（以最后一次捕获为准）。这是两个参考实现共有的
  行为，也正是本卡要消除的"同一文件多张卡"噪声。
- `expanded: bool`；展开后显示全部（上限沿用路由容量），不再截断。
- 六行上限只计文件，`Show N more` 只计隐藏文件。独立捕获错误及已有卡片的
  预览/打开错误均保持可见；它们不伪造文件行，也不藏在溢出入口后。
- 捕获失败结果必须保留可验证的原调用/回合归属。允许在 controller 的结果交付层
  携带归属和已有错误码以显示错误，不改变 capture_candidate 的安全决策、Git
  校验、fence、取消或错误分类。未知归属不得归入当前活跃回合。

### 竞态：捕获晚于回合结束（实现必须处理）

捕获不是同步的。`observe_artifact_event` 在 `ToolCallFinished` 时把
`ArtifactTerminalWork::Capture` 推进队列，`launch_artifact_terminal_for` 起独立
线程跑 Git 校验，再在 GPUI 侧轮询 `try_recv`，最后才进 `finish_artifact_terminal`。
因此**完全可能出现「`MessageFinished` 已到，而最后一个捕获还在 worker 里」**。

若聚合只发生在 `finish_run_activity` 那一刻，最后一个产物会被永久漏掉——
这是本卡最容易写错的地方。要求：

- 聚合是**增量**的，不是一次性快照：回合结束后每次 `finish_artifact_terminal`
  新捕获到产物，都把它并入该回合已存在的 `ArtifactGroup`，并 `cx.notify()`。
- 归属键用 `message_id`（回合标识），不是「当前活跃回合」的隐式状态；否则
  迟到结果会串到下一回合。
- 该回合的 `ArtifactGroup` 已存在时只更新内容，不重复插入条目。
- 若迟到产物属于已经离开屏幕的回合（历史回合），不回溯插入历史，也不串到当前
  回合；丢弃是允许的，但不得显示在错误位置。

### 展示：两行 → 紧凑单行


- 删除来源标签（`agent artifact` / `workspace change`）与 `source_label` /
  `source_color`；`ArtifactSource` 仍保留在投影里用于捕获语义，不在正文渲染。
- 单个产物行 = 文件图标 + 等宽文件名（`flex_1`、`truncate`）+ 次要信息位：
  - 默认显示文件大小（参考实现 `914 B` / `270.5 MB`），数据来自
    `ArtifactEvidence.size`，需新增到 `ArtifactCard` 投影；
  - hover 或键盘 focus 时显示操作入口（参考
    `.artifact-slot-panel__card-actions { display: none }`）。
- 容器：`bg_elevated`、`border_1` + `border_subtle`、圆角取
  `Layout::TOOL_DETAIL_RADIUS`（12，与既有工具详情卡一致）。

### 操作：七按钮 → hover 态 + 菜单

- 行内默认不常驻按钮；hover 或 focus 时显示两个入口：
  - `Preview`（`Icon::Document`，可访问名称 `Preview file`）；
  - `Open in…` 菜单触发器（`Icon::More`，可访问名称 `Open file in`）。
- 六个目标收进菜单，行沿用 `crate::menu_list::row_container` +
  `MENU_ROW_HEIGHT`，与 composer 既有菜单一致；菜单项文本沿用
  `VS Code` / `Cursor` / `Zed` / `Terminal` / `Default` / `Finder`。
- 菜单关闭语义复用仓库已验证的捕获阶段方案：触发器
  `capture_any_mouse_down(stop_propagation)`，菜单本体
  `on_mouse_down(stop_propagation)` + `on_mouse_down_out` 关闭，外层
  `gpui_kit::deferred` 包裹。
- **键盘可达**：hover 才显示的控件必须能被 Tab 聚焦，聚焦时同样显示操作；
  否则键盘用户无法发现入口（AGENTS.md UI/UX 规则要求）。
- `opening.is_some()` 或 `current_file_id.is_none()` 时对应控件禁用，禁用原因
  通过 tooltip 就近提供。

### 视觉规格（全部使用 token，无硬编码颜色/字号）

| 项 | 值 | 依据 |
|---|---|---|
| 容器 | `bg_elevated` + `border_subtle` + `Layout::TOOL_DETAIL_RADIUS` | 与工具详情卡一致 |
| 单行高 | `ROW_HEIGHT`（24） | Vega 13px 字号体系；参考 32/64px 属 Electron 15px 体系，按比例收敛 |
| 行内边距 | `px_3` / `py_1` | 沿用现状 + 参考 `padding 8px 12px` |
| 文件名 | `MONOFONT` + `Typography::CODE` | 参考用等宽文件名 |
| 大小 | `Typography::METADATA` + `text_tertiary` | 参考副标题 13px 三级色 |
| 图标 | 16px `Icon::Document` | 参考 28×28，按字号体系收敛 |
| hover 背景 | `bg_hover` | 参考 `.artifact-slot-panel__card:active` |

明确**不做**：diff `+N/-N` 统计（投影无该数据，需要新的计数捕获通道，超出本卡
范围）；`file-changes` 分类过滤（WorkBuddy 特有）。两者作为后续独立卡记录。

### 行模型契约（实现必须逐条满足）

```text
ArtifactGroup::row_count() = file_rows + usize::from(overflow > 0) + error_rows
file_rows = if expanded { files.len() } else { min(files.len(), MAX_VISIBLE) }
overflow = files.len() - file_rows
```

- `ArtifactCard` **保留**自己的 `row_count()` 语义：workspace 预览标签页用
  `uniform_list` 按 `row_count()` 逐行渲染 `ArtifactCard::render_row`
  （`crates/vega/src/window/workspace.rs`），改动它会破坏预览。因此
  `ArtifactCard` 的行模型不动，变化的是**它不再作为对话流条目渲染**。
  根据本轮用户裁决，workspace 预览标题也移除来源标签；除此之外预览形态保持不变。
- `StreamEntry::Artifact` 由 `StreamEntry::ArtifactGroup` 取代。
- 调用方需同步：`render_rows.rs`（条目渲染）、`run_activity.rs`
  （`RunActivityChild::Artifact` 子项）、`phase.rs`、`model.rs`
  （`row_count` 分发）、`composer.rs`（插入时机）、`message_anchors.rs`。

### 数据模型变更（最小）

- `vega_conversation::types::ArtifactCard` 新增 `pub size_bytes: Option<u64>`，
  由 `ArtifactEvidence.size` 填充；无证据时为 `None`，**不伪造大小**。
- 该字段是安全投影：只有字节数，不含路径、内容或摘要。

### 测试注入缝

渲染级断言需要构造 `ArtifactProjection`，而 `ArtifactCardId` /
`WorkspaceFileId` 在 `vega_conversation` 外不可构造（有意设计）。本轮沿用仓库
最小测试接线，新增 `#[doc(hidden)]` 构造缝（如 `ArtifactCardId::for_test` /
`WorkspaceFileId::for_test`），**仅为测试观察接线，GREEN 后随测试一起删除**，
恢复交付树 0 发现/0 执行。`#[doc(hidden)]` 不提供编译隔离，构造缝必须用临时
非默认测试 feature 隔离，仅由测试 dev-dependency 启用，不能向正常生产构建开放。

已核实现有 `vega_ui/Cargo.toml` 无 `[dev-dependencies]`，`vega_conversation`
也无测试 feature。交接 prompt 的「已经接线」不成立；允许临时启用当前已锁定
`gpui_kit` 的 `test-support` feature 与上述测试 feature，不引入新 crate。
测试、独占观察接线、构造缝与这些 dev-dependency/feature 均在 GREEN 后删除。
GPUI snapshot 的 AX label/value 不等于实际绘制文本，不能用它冒充文本/颜色断言。
观测路线与证据边界见 [测试观测接线核查](vega-issue-542-test-observation.md)。

**本轮用户明确批准测试位置例外**：UI 行为测试仍放 `vega_ui/src/artifact_card.rs`；
controller 的登记、真实 ingress 顺序和迟到失败归属测试可放
`vega/src/window/artifact.rs`，证据到 `size_bytes` 的安全投影测试可放
`vega_conversation/src/artifact/mod.rs`。该例外只为覆盖原有验收，并非新增产品范围。
允许必要的临时无 I/O fixture、外部 worker 执行隔离及非默认测试 feature；如需跨 crate
复用 UI 观察接线，仅通过临时测试 feature 启用。所有独占支持在 GREEN 后删除。
不 mock 被测登记/归属/fence 决策，不绕过权限、不产生真实外部应用/Git/网络执行，
不通过单纯字段检查或 mock 转发次数代替可观察结果。

### 保持不变的契约

- `ArtifactPreviewRequested` / `ArtifactOpenRequested` / `ArtifactCleared` 事件与
  `vega/src/window/artifact.rs` 的请求处理、fence、取消、错误分类全部不变。
- 捕获链路（`prepare_capture` → `ArtifactTerminalWork::Capture` →
  `capture_candidate`）、Git 边界、安全校验、路由容量、取消全部不变。
- 预览仍在 workspace 面板打开，入口仍可发现。
- 错误仍可见且不折叠：回合结束时若捕获失败，错误必须在聚合区独立错误条目中
  可见，即使本回合没有任何成功文件卡；已有文件的打开/预览错误也始终可见。
  不伪造文件能力、不把取消改为新的失败类型，不改变已有错误码语义。

## 验收用例

1. **运行中不出卡**：进行中的回合即使已有 `Write`/`Edit` 成功捕获，
   对话流里也不出现产物条目。
2. **回合结束出卡**：`MessageFinished` 后出现**一个** `ArtifactGroup`，
   位置在该回合 assistant 条目之后。
3. **聚合与去重**：同一文件写入 3 次 → 聚合卡只有 1 行；3 个不同文件 → 3 行。
4. **溢出入口**：7 个产物 → 6 行 + `Show 1 more`；点击后全部可见。
5. **无来源标签**：对话聚合卡与 workspace 预览的渲染中均不出现
   `agent artifact` / `workspace change`，预览行数和操作行保持不变。
6. **无整排按钮**：默认不渲染 `VS Code`/`Cursor`/`Zed`/`Terminal`/`Default`/
   `Finder`；hover 或 focus 后出现 `Preview file` 与 `Open file in`。
7. **菜单可达**：打开菜单后六个目标各一行，点击发出匹配的
   `ArtifactOpenRequested`。
8. **预览不变**：点击预览发出 `ArtifactPreviewRequested`，workspace 预览标签页
   仍按 `ArtifactCard::row_count()` 正常渲染。
9. **错误可见**：捕获/打开/预览失败后错误文本可见且为 `colors.danger`，不被折叠。
   首次捕获即失败时也能显示独立错误条目；7 个文件加 1 条错误时，收起为
   6 文件行 + 1 溢出行 + 1 错误行，展开为 7 文件行 + 1 错误行；两态均为 8 行。
10. **禁用状态**：`current_file_id` 为 `None` 时预览与打开均禁用且不发事件，
    禁用原因可获取。
11. **中断/失败回合**：`Interrupted` / `Error` 也走聚合，不因非 `Completed`
    丢弃已捕获产物。
12. **键盘**：Tab 能聚焦产物行并让操作显现，`Enter` 触发预览或菜单。
13. **浅/深色**：所有新增颜色取自 `ThemeColors`，`LIGHT`/`DARK` 均成立
    （无硬编码 `rgba`/hex）。

## 测试目标（TEST-FIRST，开发期保留，GREEN 后删除）

按仓库规则，行为变化先写测试并在云端 Actions 取得有效 RED。本轮测试目标：

- `artifact_cards_stay_out_of_the_stream_while_the_run_is_active`
  - 运行中捕获成功但流内无产物条目（当前会立即插入 → 有效 RED）。
- `finished_turn_groups_captured_artifacts_after_the_assistant_entry`
  - `MessageFinished` 后恰好一个聚合条目，位置在 assistant 之后（当前无此条目
    → 最小接口缺失 RED，需在报告中标注行为断言尚未执行）。
- `artifact_group_collapses_repeated_edits_of_one_file`
  - 同文件 3 次 → 1 行（当前 3 张卡 → 有效 RED）。
- `artifact_group_offers_show_more_beyond_the_visible_cap`
  - 7 个 → 6 行 + `Show 1 more`；展开后 7 行。
- `artifact_group_omits_source_labels_and_editor_row`
  - 渲染文本不含来源标签与六个编辑器名（当前包含 → 有效 RED）。
- `artifact_group_keeps_failures_visible_after_the_turn_ends`
  - 失败产物的错误文本在聚合区内可见且为 danger 色；覆盖首次捕获失败无投影、
    超出六文件上限后的错误不折叠，以及预览/打开错误，不伪造文件能力。
- `artifact_captured_after_the_turn_finishes_joins_that_turn_group`
  - 先 `MessageFinished`，再投递迟到捕获 → 该产物并入同一回合的聚合卡，
    条目数仍为 1（当前实现无聚合条目 → 最小接口缺失 RED）。

七个原始目标保留；在生产未改的本轮测试补丁中同时覆盖验收 7/8/10/11/12/13：
菜单六个目标的强类型事件、预览事件与 workspace 行模型、禁用/忙碌/阅读状态无事件、
中断/失败终态聚合、键盘显现与激活、浅深主题。迟到测试同时覆盖已有组更新、首个
结果迟到和下一回合已开始时不串回合；验证 Summary 前的位置。测试可以按可观察
行为拆分，不以七个函数为数量上限，不能等实现后补写这些已有验收。

断言只用公开/稳定边界（`row_count`、渲染输出、发出的强类型事件），不读私有字段、
不做源码字符串断言。测试与独占接线在 GREEN 后由独立提交删除，最终树保持
0 发现/0 执行。

## 非目标

- 不临时禁用/隐藏产物能力（捕获、预览、打开全部保留）。
- 不新增 diff `+N/-N` 统计（投影无此数据，另开卡）。
- 不改 `ArtifactSource` 语义、捕获校验、Git 探测或打开白名单。
- 除用户明确批准的移除来源标签外，不改预览的 workspace 标签页形态。
- 不改文件写入/编辑权限、审批、审计或数据安全边界。
- 不扩展跨项目/worktree 产物识别。
- 不改历史消息的产物渲染策略（历史页当前没有产物条目，不新增回溯）。

## 执行记录

- 原始基线：`83fd51d79940516029cdc1eaf8e81566870c877b`。
- 本轮实现前已 fetch 并同步基线到 `871f91f246e90753743a256cee1eec315e74effc`；
  保留其 Markdown 引用渲染修复。同步前后 v2 文件 SHA-256 均为
  `6c163b7f483e66a5dbbd2554ba9325457c7cf63d494ad7b7b7017260c5a30039`。
- v2.1：实现前用户明确批准预览也删来源标签、无投影捕获失败使用不折叠的独立
  错误条目。同步修正展开/错误行数公式与测试接线事实，不改变其余 v2 验收。
- 分支 / worktree：`feat/542-artifact-card-row` /
  `~/Workspace/worktrees/vega-542-artifact-card`。
- 本地限制：按 Vega 规则本地不编译、不运行测试，仅做静态检查；
  RED/GREEN 与最终门禁全部交 GitHub Actions。
- v2 变更：用户指出参考实现的菜单在回合结束后才出现，经源码核实后把
  "出现时机 + 聚合"纳入本卡范围（原 v1 只做样式收敛）。

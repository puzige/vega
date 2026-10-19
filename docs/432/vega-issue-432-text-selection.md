# Issue #432 · 聊天文本光标与 Composer 选择

用户反馈：[Issue #432](https://github.com/puzige/vega/issues/432)。CUI 已明确为聊天正文/消息区；Composer 双击、三击和选区颜色不完整。此卡继续 #357 已接通的链接与选择事件，不扩大到终端、解析器或运行时。

## 已确认根因

- `text_input/state.rs::on_mouse_down` 忽略 click_count，双/三击均执行单击的 move_to/select_to，缺少词/逻辑行选择。
- `text_input/element.rs::TextElement` 选区绘制使用普通 bg_active 活动背景，未采用已有的文本选区 token。
- `conversation_stream/selection.rs::SelectableStyledText` 未为可选文字注册 IBeam 光标；窗口 TextSelectionLayer 不自动替每个参加者设置光标。#357 仅为链接注册手形。
- Composer 的 TextInput 已声明 IBeam；保留并验证实际控件 hover、焦点和独立编辑选择，不能让窗口级只读文字选择层接管输入框手势。

## 实现前验收矩阵

| ID | 操作/前置 | 可观察结果 | 层级/证据 |
|---|---|---|---|
| T01 | 普通消息、Markdown 标题/列表/引用/表格、展开思考中的文字悬停 | 可选文字为 IBeam；#357 链接仍为 PointingHand；按钮/菜单保持各自指针 | 生产 Element/命中静态与 GPUI 回归；系统光标由用户手测 |
| T02 | Composer 输入 `alpha beta`，对 beta 的首/末字形双击，随后抬起/微移 | 选中完整 beta；单击仍只移动插入点 | GPUI 实际 TextInput 渲染/鼠标事件、selected_range/剪贴板 |
| T03 | Composer 有显式换行和软换行，对一段三击 | 选中所在逻辑行（两端不含换行），覆盖其所有软换行；不选邻行 | GPUI 实际布局事件 |
| T04 | 双/三击后拖动、反向拖动；普通拖选、Shift 扩选、右键 | 多击选区不因无按键移动丢失；拖动按词/逻辑行扩展；普通选择及方向保持，右键不重置 | GPUI 事件、选区/复制断言 |
| T05 | 浅/深主题下鼠标和 Cmd-A/Shift 选区 | 真实 painted quads 使用现有文本 selection token，与普通活动背景有别；区域对应字形 | GPUI painted_quads；不增加颜色字面量 |
| T06 | 中英混排、emoji、空白/标点、空文本、末尾点击、超过可见行的内容 | 范围是有效 UTF-8 边界；不 panic、不改原文，保持光标随行和视口规则 | 单元与生产 UI 回归 |
| T07 | masked 单行字段选区/复制/剪切；IME 标记、粘贴、替换选文、Enter/Cmd-Enter | 只绘掩码，仍拒绝秘密复制/剪切；IME/编辑/发送契约不变 | 保留既有断言，增加必要回归 |
| T08 | TextSelectionLayer 与聊天和 Composer 同窗 | 点击输入框后编辑选择归输入框，正文复制归正文，焦点不被根选择层抢走 | GPUI 共存窗口回归 |
| M01 | 合并后用户用真实鼠标在聊天和 Composer 操作 | I 字/链接手形正确，多击与高亮清楚，复制和编辑自然 | 用户手测；自动化不能冒充真实系统光标 |

所有测试在 Actions 执行前为 NOT RUN；本地只允许快速格式、差异和静态检查。

## 冻结行为

1. 共享可选文字元素为自身真实文本区域建立 Normal hitbox 并申请 IBeam；在同一次 paint 中先申请文本光标，再注册 #357 链接手形，后者优先。不要给整条消息或包含按钮的外层统一设 IBeam，不改变布局、字形、鼠标传播或选择/复制状态。
2. Composer/共享 TextInput：一次点击保持已有 caret/Shift 扩选；双击选词，三次及更多点击选逻辑行，逻辑行范围不包含换行。软换行不切断词或逻辑行。鼠标多击命中按实际字形而非最近 caret 边界，避免末字形右半边选到后面的空格。
3. 词边界沿用仓库现有文本选择分类：ASCII 字母数字/下划线及已有 Latin/Cyrillic/combining mark 类连接，水平空白可成组，其他文字/标点以有效字符边界选择；不增加分词或 grapheme 依赖，不扫描/改写用户文本。空文本、文本末尾和换行边界必须确定且安全。
4. 私有鼠标选择状态可记录锚点范围及字符/词/逻辑行粒度。按住左键拖动时保留起始选择单元并向两侧扩展，抬起后无按键移动不能重写选区。仅右键不改变选区。Shift 延续原有扩选规则。
5. 输入控件使用 GPUI Base 既有 `Theme::global(cx).tokens.colors.selection` 绘制实际选区；不改主题色值、不硬编码新颜色。masked 的选区几何按内容/显示坐标映射，不能绘制、复制或剪切真实凭据。
6. 输入框自己管理编辑选择，可在自身 mouse-down 冒泡处理中使用既有 GlobalState::suppress_text_selection 让窗口选择层不接管输入手势；不使用全窗口 stop_propagation，不改变原生输入/IME handler 或点击前清除旧只读选区的行为。
7. 不新增可见说明文案、tooltip 或 tab stop；不改解析器、协议、持久化、配置、依赖或生产 public API。代码不加注释或非测试 unwrap/expect，不削弱断言/ignore/重试换绿。

## 执行计划与边界

- 根因与验收先冻结；专职子 agent 独占实现，Codex 主会话仅负责文档、审查、集成和 PR/CI。
- 本卡独立 worktree `vega-432-text-selection`、分支 `feat/432-text-selection` 暂基于 #357 的 `f8afe904`，以准确接续链接手形优先级。#357 合并后，主 agent 将本卡自有提交重放到最新 origin/master，不把 #357 的三个旧提交重复带入 PR。
- 允许实现文件：`crates/vega_ui/src/text_input/{mod.rs,state.rs,element.rs,tests.rs}`；可新增其私有辅助模块；`crates/vega_ui/src/conversation_stream/selection.rs`；相关 `conversation_stream/tests/` 测试及测试模块注册。若确有必要仅调整同目录 `links.rs` 的光标申请顺序/共享 hitbox，先报告，不改变 #357 链接状态机。
- 已审查允许在 `links.rs` 现有 PointingHand 请求处增加最小 `cfg(test)` 私有观测，与正文 IBeam 请求比较同一 hitbox 的申请顺序；生产逻辑、命中和顺序保持不变，不把请求观测冒充真实系统光标读取。
- 顺序：先补多击和选区绘制的回归，再实现 TextInput 私有鼠标状态/选择单元；接上 selection token 和根选择层抑制；最后补共享正文 IBeam 与链接优先级并核对既有 #357/#147 回归。
- 交 Actions：`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo nextest run --workspace`。本地不运行 Vega、不编译、不执行测试或打包。
- 回滚：整体回退本卡提交，保留 #357 链接修复。合并后 In review 等用户纯手测；手测前不关闭 Issue、不 Done、不清理任务分支/worktree。

## CI 夹具前置修订

[Actions 38953752206](https://github.com/puzige/vega/actions/runs/38953752206) 的 Clippy 成功，Nextest 为 2832 passed / 1 failed / 5 skipped。唯一失败是 T06 超长输入最后一行的三击，原期望仍为 `1301..1306`，实际保留了 set_text 的折叠末尾 `1306..1306`；其余 15 个新增用例和既有回归均通过。

静态调用链：set_text 先按显式换行更新逻辑视口，3 行旧视口遇到只有 2 条逻辑行的长文本仍保留 3 行；prepaint 学到 8 条可见视觉行后，paint 才写回 rows=8 并 request_animation_frame。GPUI 测试没有平台帧循环，run_until_parked 不会投递 on_next_frame，故此时 last_lines 已是 8 行而真实布局/命中仍是此前 3 行。测试按第 8 行生成的点击落在旧输入边界外。

只修本卡测试前置：动态长文本更新后用已有 window.simulate_next_frame 投递已请求的下一帧，排空后 refresh/draw，并遵守返回的 ArenaClearNeeded::clear，再读取命中几何。两个长文本分支点击前必须断言已是 8 行可见视口、仍存在溢出首行，以及目标字形中心位于实际 TextElement bounds 和窗口 viewport 内。保留所有原字串、三击最后一行范围、300 字长词完整选择和复制断言；不改生产布局/事件/选择逻辑、不缩短输入、不直接调用鼠标 handler、不忽略或放宽失败。

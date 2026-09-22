# Issue #98 · 「回到底部」改为悬浮按钮（规格冻结）

> 状态：**SPEC FROZEN**
> 基线：`master @ 8cff36b`
> 来源：用户 2026-09-21 实机截图（`puzige/vega#98`）+ 一句「参考 codex 实现」
> 前置：S8-T44（可变高度列表原生 Tail 跟随）、R19（route identity 移入窗口 header）
> 涉及组件：`crates/vega_ui/src/conversation_stream/render.rs`

---

## §1 需求与现状

### 用户诉求

Issue #98 正文只有一张截图 + 「参考 codex 实现」。截图上用户用红箭头标出参考实现里的一枚**悬浮按钮**，位置在对话正文下方、输入框上方，水平居中。

### 现状（Vega）

`render_resume_tail`（`crates/vega_ui/src/conversation_stream/render.rs:77-100`）渲染的是：

- 纯文字按钮 `回到底部`；
- `.absolute().top_2().right_3()` —— 固定在对话区**右上角**；
- 仅当 `!self.following_tail()` 时挂载（`render.rs:1155-1157`）。

与参考实现的差异：**位置**（右上角 → 悬浮于输入框上方）、**形态**（文字 pill → 圆形图标按钮）、**图标**（无 → 向下箭头）。

---

## §2 参考实现实测（Codex，2026-09-22 像素量测）

量测源：Issue #98 截图（`/tmp/issue98.png`，2786×1836 @2x → 1393×918 逻辑）。

| 量 | 2x 像素 | 逻辑 px | 说明 |
|---|---|---|---|
| 按钮外接方框 | 1688..1751 × 1496..1559 | **32 × 32** | 63–64px / 2 |
| 按钮中心 x | 1719.5 | **859.75** | 与 composer 卡片中轴（859.75）重合 |
| 按钮下沿 y | 1559 | 779.5 | — |
| composer 卡片顶沿 y | 1608 | 804.0 | — |
| 按钮下沿 → 卡片顶沿 | — | **≈24** | 悬浮间距 |
| 填充 | `(255,255,255)` | 白 / elevated | — |
| 边框 | `(236,236,236)` | 1px | 中性细边 |
| 图标 ink | `(25,28,31)` | 深色 | 文字主色 |
| 图标字形 | 25 × 27 px 外接 | ≈13 × 13.5 | 竖直杆 + 底部箭头 → **向下箭头**（非纯 chevron） |

**结论**：参考实现是**水平居中于正文/composer 内容列、悬浮于 composer 卡片上方约 24px 的一枚 32px 圆形向下箭头按钮**，白底 + 1px 中性细边，仅脱离底部时出现。

---

## §3 冻结规格

### R1 · 形态：圆形图标按钮

- 用 `vega_ui::icons::Icon::ArrowDown`（向下箭头；`icons.rs` 已映射 `IconName::ArrowDown`），固定 16px 光学网格。
- 交互面 **32 × 32px** 正圆（`size(px(32.)).rounded_full()`）。
- 填充 `colors.bg_elevated`；1px `colors.border_subtle`；图标 ink `colors.text_secondary`。
- 允许克制轻阴影（`shadow_sm()`）——设计规范 §6.2 允许浮层/临时控件使用轻阴影。
- hover 表面 `colors.bg_hover`；hover 必须带 `.id()`（R67：`.hover()` 不带 `.id()` 不重绘）。
- 不再渲染 `回到底部` 文字；`render_resume_tail` 改名为语义一致的实现（保留函数名亦可，但内容替换）。

### R2 · 位置：内容列中轴，悬浮于正文视口底部

- **水平**：与正文/composer 共用同一中轴。按钮挂进正文列包裹层（`render.rs` 的 `conversation-column`，`.max_w(CONTENT_MAX_WIDTH).mx_auto`），该层加 `.relative()`，按钮 `.absolute().bottom(px(Layout::SCROLL_TO_BOTTOM_GAP)).left_0().right_0()`，内层再 `mx_auto` / `justify_center` 居中，从而与 `Layout::CONTENT_MAX_WIDTH`（=768，与 composer 同宽）同轴。
- **垂直**：锚定正文视口底沿，留出 `Layout::SCROLL_TO_BOTTOM_GAP`（**24px**）的下边距；因正文视口下方还有 context-status band 与 composer 的 `COMPOSER_PADDING_TOP`，按钮实际落在 composer 卡片上方一段距离（参考实测 ≈24px 量级），不承诺与卡片顶沿的精确像素差。
- 不随滚动内容移动（是视口层，不是列表 item）。

> 实现提示：现挂载点是根元素（`.relative()`）。直接对根做 `bottom` 锚定会耦合动态 composer 高度与 context band。改为把按钮挂进 **`conversation-column` 包裹层**（正文列，已 `mx_auto`），给该层加 `.relative()`，按钮用 `bottom(px(GAP))` 锚定其底沿，即可稳定悬浮于正文视口底部并与正文列同轴。挂载点变更须在实现计划中说明。
>
> 数值可验算性：`bottom = 24px` 是**按钮底沿到正文视口底沿**的间距，这一条可在 GPUI 测试里用几何断言（R2/A3）。不要再写“到 composer 卡片顶沿 == 24”这类不可直接控的断言。

### R3 · 可见性与行为（不变）

- 仅当 `!self.following_tail()` 时可见（沿用现谓词）。
- 点击（`on_mouse_up`）与键盘（`Enter`/`Space`）均触发既有 `resume_tail`（`core.rs:518`），回到 `FollowMode::Tail`。
- 保留 `resume_tail_focus` 焦点句柄与 `ResumeTailButton` key context，`tab_stop` 可达。

### R4 · 可访问性

- `aria_label("回到底部")`（文案保持，便于键盘/无障碍识别；视觉上不再显示文字）。

### R5 · 新增 token

`vega_theme::Layout` 新增：

```rust
/// 悬浮「回到底部」按钮的交互面直径（参考 Codex 实测 32px）。
pub const SCROLL_TO_BOTTOM_SIZE: f32 = 32.0;
/// 悬浮「回到底部」按钮底沿到正文视口底沿的间距（参考实测 ≈24px）。
pub const SCROLL_TO_BOTTOM_GAP: f32 = 24.0;
```

冻结测试：断言两值（与既有 token 冻结测试同文件）。

---

## §4 非目标

- 不改 Tail 跟随状态机 / 原生 list 行为。
- 不改 composer 几何、正文列宽（#100 已冻结 768）。
- 不新增滚动方向/速度相关交互，不引入入场动画（tech-spec §5.4 动效禁令）。
- 不删除键盘路径，不引入新依赖。

---

## §5 验收矩阵（测试先行）

| ID | 需求/风险 | 前置状态 | 操作 | 预期可观察结果 | 层级 | 证据 |
|---|---|---|---|---|---|---|
| A1 | R1 形态 | 脱离底部 | 渲染 | 按钮为 32px 正圆、ArrowDown 图标、无文字；`painted_quads` 可断言尺寸/圆角 | 生产测试 | GPUI test |
| A2 | R1 颜色 | 脱离底部 | 渲染 | 填充=bg_elevated、边框=border_subtle、图标=text_secondary（Light+Dark） | 生产测试 | 像素/token 断言 |
| A3 | R2 位置 | 脱离底部、composer 可见 | 渲染 | 按钮中轴 == 正文列中轴；按钮底沿距正文视口底沿 == 24 | 生产测试 | 几何断言 |
| A4 | R3 可见性 | 贴底跟随中 | 渲染 | 按钮**不**渲染；上翻后渲染 | 生产测试 | 谓词/树断言 |
| A5 | R3 行为 | 脱离底部 | 点击 / Enter | `following_tail()` 变真，按钮消失 | 生产测试 | 事件断言 |
| A6 | R3 键盘 | 脱离底部 | 焦点到按钮后 Enter | 触发 resume，焦点语义保留 | 生产测试 | 焦点断言 |
| A7 | R4 无障碍 | 脱离底部 | 渲染 | 节点 aria_label = 回到底部 | 源码审查 | 平台无 a11y-tree 查询，SKIP（见下） |
| A8 | R5 token | — | 编译期 | `SCROLL_TO_BOTTOM_SIZE==32`、`GAP==24` | 单元 | 冻结测试 |
| A9 | 回归 | 既有滚动跟随 | 运行既有 `scroll_follow`/`e2e_variable_height` | 全绿 | 单元/E2E | 既有套件 |
| A10 | 真实 UI | 真实 Vega、脱离底部 | 原生截图 | 悬浮圆钮出现在输入框上方居中，Light+Dark 各一张 | E2E-REAL | 截图 + SHA-256 |

**E2E-first 边界**：A1–A8 用 GPUI `TestAppContext`（可观测绘制/焦点，见 `AGENTS.md` §原生 UI 验收）；A10 用真实构建的原生截图 + 像素量测（悬浮位置/尺寸/配色）。合成键盘事件不可用于焦点路径（工具限制），焦点以生产测试为准。

---

## §6 实现计划（轻量）

1. `vega_theme::Layout` 新增 `SCROLL_TO_BOTTOM_SIZE=32.0`、`SCROLL_TO_BOTTOM_GAP=24.0`，并在该文件既有 token 冻结测试补两条断言。
2. `render_resume_tail`（`render.rs:77`）重写：`div().id("scroll-to-bottom")`（hover 需要 id）、32px 正圆、`bg_elevated`/`border_subtle`/`shadow_sm`、`Icon::ArrowDown`（`text_secondary`）、`aria_label("回到底部")`；保留 `resume_tail_focus`、`ResumeTailButton` key context、`on_action`/`on_mouse_up` → `resume_tail`。
3. 挂载点从根元素改到 `conversation-column` 包裹层（`render.rs:1173` 一带）：该层加 `.relative()`；按钮 `.absolute().bottom(px(GAP)).left_0().right_0()` 内层居中；移除根元素上的 `.when(!following_tail, ...)` 挂载。
4. 新增 GPUI 测试文件 `crates/vega_ui/src/conversation_stream/tests/issue98_scroll_button.rs`，覆盖 A1–A8；登记进 `tests/mod.rs`。
5. 受影响包：`vega_theme`、`vega_ui` 及其传递依赖方；门禁走 `python3 scripts/verify.py`。
6. 回滚：单文件级 revert（spec + render.rs + theme + 测试）。

**允许修改范围**：`crates/vega_theme/src/lib.rs`、`crates/vega_ui/src/conversation_stream/render.rs`、`crates/vega_ui/src/conversation_stream/tests/issue98_scroll_button.rs`、`crates/vega_ui/src/conversation_stream/tests/mod.rs`、本 spec、`README.md` 文档索引一行。**不得**改动 Tail 状态机、composer 几何、正文列宽或其他测试断言。

---

## §7 变更记录

- v1 (2026-09-22)：首版冻结。基线 `8cff36b`；参考几何来自 Issue #98 截图像素量测。

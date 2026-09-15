# R68 · 两个下拉弹窗：点击外部关闭 + 修正宽度与内边距（规格冻结）

> 状态：**SPEC FROZEN**（v2 —— §3 的结论相对 v1 已**反转**，见该节开头的更正说明）
> 基线：`master @ 1c62455`
> 来源：用户 2026-09-15 实机截图 + 两条口头反馈
> 前置：R62（两个下拉的列表结构）+ R64（deferred 绘制）
> 涉及组件：`crates/vega_ui/src/menu_list.rs`、`conversation_stream/utility_bar.rs`、`branch_selector.rs`

---

## §1 用户的两条反馈

| # | 用户原话 | 结论 |
|---|---|---|
| 1 | *"它弹出了之后悬浮在那里，我点其他地方它不消失。"* | 两个弹窗都缺"点击外部关闭"。 |
| 2 | *"这两个弹窗真的做的太他妈丑了 … 它的边距大小，你这也太窄了吧？"* | **主因是卡片太窄**（实测 183 vs 参考最小 260，差 30%），把项目名挤到截断；行内边距只差约 2px。详见 §3。 |

用户第一张图里，项目名显示成 `r13-alp...` / `r12-link...` / `r13-bet...` —— **全部被截断**。这是第二条反馈里最刺眼、也是最能直接验证的一处（A8）。

---

## §2 问题 1：点击外部不关闭

### 现状

`close_composer_popovers`（`composer_actions.rs:246`）**存在**，但它只在"打开另一个 composer 弹层"时被调用（`utility_bar.rs:283`、`core.rs:901`、`render.rs:579`、`composer_actions.rs:357`）。**没有任何一条路径把它接到"点击别处"上。**

全仓库只有两处用了 `on_mouse_down_out`（`workspace.rs:1476` 的工作区菜单、`sidebar/row_helpers.rs:40`），两个 composer 弹窗都不在其中。

### 机制（本轮实测，不要凭推断）

`on_mouse_down_out`（`gpui-pre-0.3.4/src/elements/div.rs:263`）在 **capture 阶段**、且指针**不在**该元素 bounds 内时触发。四个组合实测（匿名/有 id × 有/无 `deferred`）：

```
[stateful=false deferred=false] click INSIDE : out=0 in=1
[stateful=false deferred=false] click OUTSIDE: out=1 in=1
[stateful=true  deferred=false] click INSIDE : out=0 in=1
[stateful=true  deferred=false] click OUTSIDE: out=1 in=1
[stateful=false deferred=true ] click INSIDE : out=0 in=1
[stateful=false deferred=true ] click OUTSIDE: out=1 in=1
[stateful=true  deferred=true ] click INSIDE : out=0 in=1
[stateful=true  deferred=true ] click OUTSIDE: out=1 in=1
```

**结论**：`on_mouse_down_out` **不需要 `.id()`**（与 R67 的 `.hover()` 不同），且在 `deferred` 包裹下照常工作。加它不会破坏 R64 的绘制顺序修复。

### ⚠️ 陷阱：只加 `on_mouse_down_out` 会让"点触发器"变成关不上

触发器的开关跑在 **mouse-up**（`utility_bar.rs:97` 的 `toggle_utility_projects`、`branch_selector.rs:854` 的 `toggle`），而 `on_mouse_down_out` 跑在 **mouse-down**。于是点触发器时：down 先把它关掉 → up 再把它打开 → **看起来完全没反应**。

实测复现：

```
1. click chip        -> open=true
2. click chip again  -> open=true   ← 应该是 false，关不掉
3. click chip (open) -> open=true
4. click INSIDE menu -> open=true
5. click OUTSIDE     -> open=false
```

**修法（已实测通过）**：让触发器在 **capture 阶段**抢先声明这次手势，弹窗的 out 处理器就不会看到它：

```rust
// 触发器上（替代现在的 on_mouse_down(stop_propagation)）
.capture_any_mouse_down(|_, _, cx| cx.stop_propagation())
```

现在的 `.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())` 是 **bubble 阶段**，比 capture 晚，**挡不住** out 处理器——这就是必须换成 `capture_any_mouse_down` 的原因。

实测（换上 capture 之后）：

```
1. click chip        -> open=true
2. click chip again  -> open=false   ← 正确
3. click chip (open) -> open=true
4. click INSIDE menu -> open=true    ← 正确
5. click OUTSIDE     -> open=false   ← 正确
```

**另一条走过的弯路（记录以免重犯）**：把 `on_mouse_down_out` 挂到**包含触发器的外层 wrapper** 上也能修好"点触发器关不上"，但会**破坏弹窗内部点击**——因为弹窗是 `absolute` 定位，落在 wrapper 的布局 bounds **之外**，于是弹窗内部的点击被当成"外部"。实测该方案第 4 步 `open=false`。**不要用这个方案。**

### 契约

**R1（必须）** 项目弹窗与分支弹窗**都**在点击弹窗外部时关闭。

**R2（必须）** 关闭必须走各自的**既有**关闭路径，不得新造状态机：

- 项目弹窗：`self.utility_projects_open = false`（经 `cx.notify()`）。
- 分支弹窗：`BranchSelector::request_close(cx)`（`branch_selector.rs:667`）——它还会 emit `BranchSelectorClosed`，pending 清理归 controller，**不要**绕过它直接改 `model.status`。

**R3（必须）** 两个触发器都要用 **`capture_any_mouse_down(stop_propagation)`** 替换现有的 bubble 阶段 `on_mouse_down(stop_propagation)`，使"弹窗打开时点触发器"能正常关闭（§2 实测）。

**R4（必须）** 点击弹窗**内部**（含搜索框、行、末尾动作行）**不得**关闭弹窗。

**R5（必须）** 关闭动作只作用于**自己的**弹窗。分支弹窗关闭时不得顺手清掉项目弹窗的状态（两者互不隶属：项目弹窗状态在 `ConversationStream`，分支弹窗状态在 `BranchSelector`）。

**R6（不得）** 不得改变 R64 的 `deferred` 包裹与 priority（绘制顺序契约）。

---

## §3 问题 2：真正窄的是**弹窗宽度**，不是行内边距

### ⚠️ 我第一版规格在这里搞错了，已更正

第一版从参考实现的 CSS 里取了 `--menu-item-height: 9×spacing = 36`、`--menu-item-padding: 6/10`、`--menu-gutter: 10`，据此写下"参考 20px vs Vega 12px，内边距差一倍"。

**但这组 token 定义在 `[data-vega-window-type=browser]` 块里，是浏览器变体**，而 Codex 桌面截图才是产品真身。实测两张截图（方法已用 Vega 自身的 32px 行高校准为 **2×**：64px 行距 ÷ 32 逻辑 = 2.0）：

| | 参考（Codex，2× → 逻辑） | Vega 现状 | 差 |
|---|---|---|---|
| **卡片宽度** | 261（CSS 另有 `min-w-[260px]` 佐证） | **183** | **-30%** |
| 卡片边 → 行填充（gutter） | 4.5 | 4 | ≈0 |
| 行填充 → 图标（padding-x） | 9.5 | 8 | ≈1.5 |
| **卡片边 → 图标** | **14** | **12** | **≈2** |
| 行高 | 28.5 | 32 | Vega 反而**更高** |

**结论（推翻第一版）**：

- **行内边距基本一致（12 vs 14），不是缺陷。** 第一版的"20 vs 12"是拿浏览器变体当基准造成的假差距。
- **行高 Vega 已经比参考更高**，把 32 改成 36 只会更糟。**R7 撤销。**
- **真正的缺陷是宽度**：183 vs 参考的 **最小** 260，窄了 30%。项目名被截断成 `r13-alp...` 就是这个的直接后果。

### 用户说的"边距太窄"指的是什么

用户原话：*"它的边距大小，你这也太窄了吧？"* 配的是项目弹窗截图，里面**每一个项目名都被截断**。结合上面的测量，用户看到的是：卡片太窄 → 文字被挤掉 → 整体显得又窄又挤。**宽度是主因，行内边距是次因（差 2px，肉眼几乎不可辨）。**

因此本规格**把重点从"加内边距"改成"修正宽度"**，只保留**温和**的间距微调，不做第一版那种激进改动。

### 保留的微调（有实测依据的部分）

- 卡片边 → 行填充：参考 4.5，Vega 4 —— 已在误差内，**不改**。
- 行填充 → 图标：参考 9.5，Vega 8 —— 差 1.5，**改为 10**（顺带与参考的 `--menu-item-padding` 水平分量 10 吻合，两条独立证据同向）。
- 搜索框独立灰底：参考**没有**灰底（`/tmp/codex-search.png` 实测，只有放大镜 + 占位文字）。Vega 有。**去掉**——这处差异在截图里很显眼，去掉后弹窗立刻"轻"很多。

### 契约

**R7（撤销）** ~~`MENU_ROW_HEIGHT` 32 → 36~~。**不改行高。** 实测 Vega(32) 已高于参考(28.5)，改大是反向优化。

**R8（必须）** 行内水平内边距 `px_2`(8) → **10**，与参考实测的 9.5 及参考 token 的水平分量 10 一致。卡片边到行边的 4px **不动**（参考 4.5，已在误差内）。

**R9（必须）** 搜索框去掉独立灰底，与参考一致（只有放大镜 + 占位文字，直接坐在卡片面上）；高度仍取 `MENU_ROW_HEIGHT`，水平内边距与行同为 10。

**R10（必须）** 以上改动**必须落在 `menu_list.rs` 的共享部件里**，让项目弹窗与分支弹窗同时生效——两个弹窗共用这套 chrome，不得各改一份。

**R11（不得）** 不得改变：`MENU_MAX_WIDTH`(350)、`MENU_RADIUS`(18)、选中行的 `selected_row_bg` 配色、图标尺寸、字号（`Typography::SIDEBAR` = 13）、`MENU_ROW_HEIGHT`(32)。

**R12（必须）** R64 的基线测试（`r64_popup_deferred.rs`）钉住了改动前的像素值。**项目弹窗的宽度基线必须按 R13 的新值更新**（旧基线记录的宽度就是被钳死的那个值，见 §4 的注释），并在测试注释里写明"这是 R68 的**有意**变更，不是 deferred 的回归"。R64 A1 的契约是"`deferred` 不移动几何"，不是"几何永久冻结"。

行高**不变**（R7 已撤销），所以依赖 `MENU_ROW_HEIGHT` 算出的高度基线（如分支弹窗的 `BRANCH_CHROME_HEIGHT`）**应当保持不变**；若它们变了，说明行高被误改，须回退。

---

## §4 项目弹窗被压窄（名字全被截断的根因）

`render_utility_projects_menu`（`utility_bar.rs:183`）把弹窗挂在 **chip 的 `relative()` 容器**里，并写了 `.w(px(Layout::MENU_MAX_WIDTH)).max_w_full()`。

`max_w_full` 的 containing block 是那个 chip 容器，而 chip 的宽度由项目名决定 —— 于是弹窗宽度被 chip **钳死**。生产测试实测（本轮探针）：

```
composer-utility-project-chip: w=40      ← chip 只有 40px
composer-utility-project-menu: w=40      ← 弹窗被钳到 40px
composer-utility-project-search: w=30
```

实机测量弹窗约 **183px**，而 `MENU_MAX_WIDTH` 是 **350**。名字当然全被截断。

### 契约

**R13（必须）** 项目弹窗宽度取 **`MENU_MAX_WIDTH`(350)**，**不得**被 chip 宽度钳制。这是本规格**最重要**的一条：实测宽度差 30%（183 vs 参考最小 260），项目名被截断是它的直接后果。

依据（本轮查证）：参考实现的项目下拉走 `contentWidth: workspace`，其宽度表在 `app-initial-cadbd12d4a15e.js`：

```js
workspace:`min-w-[260px]`
```

即参考实现是 **`min-width: 260px`**（不是固定宽），而 Codex 截图实测卡片正是 261 逻辑 px —— 两者互相印证。Vega 的对应常量是 `Layout::MENU_MAX_WIDTH = 350`，且 Vega 自己的模型列表也用 350 —— 用 350。

> ⚠️ 不要引用 `w-80`：那是参考实现里**邮件收件人**弹层的宽度（`data-writing-block-email-recipient-popover`），与 composer 项目下拉无关。我第一版规格误引了它，已更正。

**R14（必须）** 弹窗仍须**贴住 chip 左边缘**向上弹出（R49 §2.5 / R61 的锚定语义不变），且**不得**溢出窗口左右边界。窗口过窄时以视口为界收缩。

**R15（不得）** 不得改变弹窗的锚定方向（向上）、`bottom(relative(1.0))` 的贴边关系，或 R64 的 `deferred` 包裹。

---

## §5 验收

| # | 证据 | 判据 |
|---|---|---|
| A1 | 生产测试 | 项目弹窗打开后，点击弹窗**外部** → `utility_projects_open == false`（selector 消失） |
| A2 | 生产测试 | 分支弹窗打开后，点击弹窗**外部** → 弹窗关闭（`is_open() == false`） |
| A3 | 生产测试 | 点击弹窗**内部**（搜索框中心、某一行、末尾动作行）→ 弹窗**仍打开** |
| A4 | 生产测试 | 弹窗打开时点击**触发器** → 弹窗**关闭**（R3 的 capture 声明生效；这条专门钉住 §2 的陷阱） |
| A5 | 生产测试 | 项目弹窗宽度 == `MENU_MAX_WIDTH`(350)，且**不随** chip 宽度变化（用两个不同长度项目名的 chip 各测一次）。**这是本规格的核心断言** |
| A6 | 生产测试 | 行内水平内边距 == 10；`MENU_ROW_HEIGHT` 仍为 **32**（R7 已撤销，行高**不得**变） |
| A7 | 实机像素 | 两个弹窗与 Codex 截图的**卡片宽度**接近（Vega 350 vs 参考 261 起——Vega 更宽是可接受的，因为 Vega 用 `MENU_MAX_WIDTH`；**关键是不再是 183**） |
| A8 | 实机像素 | 项目名**不再被截断**（`r13-alpha-project` 等完整显示）。这是用户最直接能看到的一条 |
| A9 | 实机像素 | 搜索行**无**独立灰底 |
| A10 | 实机像素 | 点弹窗外部，两个弹窗都消失（含点侧栏、点主区域、点 composer 输入框三种外部位置） |
| A11 | 门禁 | `scripts/cargo-lock.sh test --workspace` 0 失败；`cargo fmt --all -- --check` 与 `clippy --workspace --all-targets -- -D warnings` 干净 |

**A7–A10 必须实机验证**：`debug_bounds` 证明的是布局，不是观感（圆角、阴影、层叠）。两者不可互相替代。

> **A5 是"太窄"的主证据，A8 是用户能直接看到的结果**。A6 反向钉住"行高没被顺手改掉"——第一版规格曾错误地要求改行高，实现者若照旧版做会做错。

---

## §6 待实测

| # | 项 | 状态 |
|---|---|---|
| M1 | `on_mouse_down_out` 是否需要 `.id()` | **已解决** —— 不需要（§2 实测四组合） |
| M2 | `on_mouse_down_out` 在 `deferred` 下是否失效 | **已解决** —— 不失效（§2） |
| M3 | 只加 out 处理器时"点触发器关不上" | **已解决** —— 确实关不上，须配 `capture_any_mouse_down`（§2 实测） |
| M4 | 参考实现 desktop 变体的菜单 token | **已解决（结论反转）** —— 我第一版误用 `[data-vega-window-type=browser]` 的 token；以 Codex 桌面截图实测为准，见 §3 更正表 |
| M5 | 搜索框去掉灰底后是否仍够"可点" | **未测**。参考实现就是无底色，实现者若认为需要额外提示须在报告里说明 |
| M6 | 分支弹窗宽度（现 320）是否也要改 | **不改**。本规格只动**共享 chrome 的内边距**与**项目弹窗的宽度** |
| M7 | 用户说的"边距太窄"是否还有别的成分 | **基本解决**。实测行内边距只差 2px，宽度差 30%；宽度是主因。若实现后用户仍觉得挤，再按实机截图继续收敛 |

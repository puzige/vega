# R67 · 标题行高亮改为 hover 态 + Off 档标签改 `Off`（规格冻结）

> 状态：**SPEC FROZEN**
> 基线：`master @ 38d3abb`
> 来源：用户 2026-09-15 实机截图 + 两条口头修正
> 前置：R66（标题块合并）+ R65（胶囊填充）
> 取代：R66 **R3**（常驻高亮）与 **R6**（`OFF_LABEL = "None"`）

---

## §1 用户的两条修正

| # | 用户原话 | 结论 |
|---|---|---|
| 1 | *"这里的阴影不是说鼠标不移上去它就展示，而是说鼠标移上去。"* | 标题块的浅灰底是 **hover 态**，不是常驻态。R66 R3 做反了。 |
| 2 | *"并且这个，null不能改成off吗？"* | 最低档标签 `None` 改为 **`Off`**。 |

截图证据（`image-4754ba9ffc99597f319de9509adbc6bc.png`，1008×352 @2x）显示卡片当前是 `None ›` + `glm-5.3-flash`，其中 `None` 就是 [`OFF_LABEL`]，因为该模型的 `efforts` 是 `low/medium/high`，**没有** `none` 这一档。

---

## §2 问题 1：高亮必须是 hover 态

### R66 做错了什么

R66 R3 把浅灰底做成了**常驻**：卡片一打开就有灰底，与鼠标无关。用户明确否定了这个读法。

**结论**：灰底只在鼠标进入标题块时出现。

### 关键实现陷阱：`.hover()` 必须有 `.id()`（本轮实测）

**这是本条最容易做错的地方，必须先看。**

GPUI 只在元素拥有 **element state** 时才会在 hover 状态翻转时 `cx.notify` 重绘（`gpui-pre-0.3.4/src/elements/div.rs:2783-2806`）；而 element state 只有在 `Element::id()` 返回 `Some` 时才存在，即链上有 `.id(...)`（`div.rs:1848`）。

`gpui-pre-0.3.4/src/elements/div.rs:3391-3405` 的绘制分支因此有两种来源：

- 有 hitbox → `hitbox.is_hovered(window)`；
- 无 hitbox → 退化到 `element_state.hover_state`，而匿名元素**没有** element state。

**实测**（`crates/vega_ui/tests/probe_hover.rs`，本轮临时探针，已删除）：

```
ANONYMOUS: base       renders=1 bg=BLUE
ANONYMOUS: hover-child renders=1 bg=BLUE   ← 永远不变
ANONYMOUS: hover-block renders=1 bg=BLUE   ← 永远不变
STATEFUL:  base       renders=1 bg=BLUE
STATEFUL:  hover-child renders=2 bg=RED    ← 生效
STATEFUL:  left        renders=3 bg=BLUE   ← 退出还原
```

**所以**：把 `render_title_block` 从"常驻 bg"改成"`.hover(bg)`"时，**必须同时给它一个 `.id(...)`**。只加 `.hover()` 不加 `.id()` 会得到一个**永远不亮**的灰底——比 R66 的常驻灰底更糟。

> ⚠️ R66 R5 的注释写着"容器没有 `id`、没有 cursor、没有 handler"，并且 `r66_r5_the_highlight_container_is_inert` 测试**断言容器惰性**。给容器加 `.id()` **不违反** R5：R5 约束的是**点击行为**，不是 hitbox 的存在。加 `id` 只带来一个 `HitboxBehavior::Normal` 的 hitbox，不注册任何监听器，所以点击穿透行为不变（探针里子行带 `id` + `on_mouse_up`，父块 hover 仍生效，且子行点击仍照常触发）。

### 契约

**R1（必须）** `render_title_block` 的浅灰底（`colors.bg_hover`）改为 **`.hover(...)` 触发**，卡片打开时**无底色**。

**R2（必须）** 该容器**必须带 `.id(...)`**，否则 hover 不重绘（§2 实测）。建议 id 与既有 debug selector 同名：`"thinking-slider-title-block"`。

**R3（必须）** hover 命中范围是**整个标题块**（两行 + 块的水平内边距），不是一个块内的某一行。两行**一起**变色——这正是用户 R66 的原话 *"模型跟 thinking level 要一起被选中"*。因此：

- 指针在**档位名行**上 → 两行同时有灰底；
- 指针在**模型名行**上 → 两行同时有灰底；
- 指针在块的**水平内边距**上 → 同样有灰底；
- 指针移出块 → 灰底消失。

**R4（必须）** `render_title_row` **不恢复**自己的 `.hover(bg_hover)`。块的 hover 已经覆盖它（子元素 hover 会同时点亮父块，§2 实测），行级 hover 是冗余的。R66 删掉它的理由（"块已常驻同色"）不再成立，但结论仍成立——理由换成"块的 hover 已覆盖"。

**R5（不得）** 不得改变 R66 的**几何**：卡片宽度 254.5、`CARD_TEXT_ROW_HEIGHT`、`CARD_ROW_GAP`、`TITLE_HIGHLIGHT_PADDING_X`、块的贴内容宽度规则、两行行高与顺序。

**R6（不得）** 不得改变两行的**交互层级**（R62 R3 / R66 R5）：点档位名行仍触发 `ThinkingSliderTitleActivated`，点模型名行仍无动作，块本身仍不处理点击。

**R7（不得）** 不得改变 hover 的**配色**。`bg_hover` 实测 `0xF3F3F3`（`vega_theme/src/lib.rs:107`），与截图里量到的 `(243,243,243)` **逐位一致**。颜色本来就是对的，错的只是**何时**出现。

---

## §3 问题 2：`None` → `Off`

### 为什么 R66 R6 选了 `None`，以及为什么现在要改

R66 R6 的依据是参考实现的 `composer.mode.local.reasoning.none.label` 的 `defaultMessage` 就是 `None`（本轮复核，`app-primary-6cd7b8b3f5e3.js`）：

```js
none:{id:`composer.mode.local.reasoning.none.label`,defaultMessage:`None`,
      description:`Reasoning effort label for a given model: none`}
```

但**参考实现的 `none` 是一档 reasoning effort**，它把"关闭推理"表达为梯子最低那一档。

**Vega 的 Off 不是一档 effort**。它是独立的 `ReasoningChoice::Disabled`，通过 profile 的 `disabled_wire` 生效，永远不出现在 `efforts` 序列里（R58 R1/R3，`thinking_slider.rs` 模块文档）。所以参考实现里**根本没有**与 Vega 的 Off 对应的标签可抄——把 `none` 这一档的显示名借来当 Vega 的 Off 标签，是把两个不同的概念混为一谈。

**用户决策（2026-09-15）**：Vega 的 Off 用**自己的**英文标签 `Off`。

### 契约

**R8（必须）** `OFF_LABEL` 从 `"None"` 改为 **`"Off"`**。

**R9（必须）** `OFF_CHOICE_NAME`（持久化名 `"disabled"`）**不得**改动（R58 R3 存储契约）。

**R10（必须）** `PROVIDER_DEFAULT_LABEL`（`"Default"`）**不得**改动。R66 R7 的依据（参考实现 `composer.modelPicker.default.label` 的 `defaultMessage` 就是 `Default`，与 `selectEffort.label` 同属 composer 命名空间）仍然成立，且用户本轮**没有**提出异议。

**R11（必须）** `tier_display_label` 的映射表**不得**改动。它是**档位**的显示名，取自参考实现 `composer.mode.local.reasoning.<tier>.label`，与 Off 标签是两个独立的东西。特别注意：

- `none` → `None` **保持不变**（这是**档位** `none` 的显示名）；
- `low` → `Light`、`xhigh` → `Extra High` 保持不变。

> 这是本条最容易做错的地方：用户说的"`null` 改成 `off`"指的是卡片上那个 **Off 档标签**（`OFF_LABEL`），**不是**映射表里的 `"none" => "None"` 那一行。改错地方会让 `none` 这一档显示成 `Off`，而真正的 Off 档不变。

**R12（必须）** 本轮**只**动 `OFF_LABEL` 一个显示字符串。`关闭` 早已在 R66 被改掉，不得回退。

---

## §4 验收

### 测试平台现在能观测绘制（本轮实测，纠正旧注释）

既有代码注释断言 *"The test platform has no headless renderer, so the grey fill ... [is] not observable here"*（`thinking_slider.rs:1808`、`:2762`）。**这句话只对了一半**：

- `VisualTestContext::capture_screenshot` → `window.render_to_image()` 确实需要 `HeadlessRenderer`，未配置时 `bail!`（`gpui-pre-0.3.4/src/platform/test/window.rs:441`）；
- 但 **`Window::painted_quads()`**（`window.rs:2618`）直接读 `rendered_frame.scene.quads`，**不需要渲染器**。

**实测**（同一探针）：`painted_quads()` 在本仓库的 `TestAppContext` 里可用，返回 14 个 quad，每个带 `bounds` / `corner_radii` / `background`，且 hover 翻转时 `background` 的 `Hsla` 真的变了。**所以本轮的高亮行为可以用生产测试断言，不必只靠截图。**

| # | 证据 | 判据 |
|---|---|---|
| A1 | 生产测试 | `OFF_LABEL == "Off"`；`OFF_CHOICE_NAME == "disabled"`；`PROVIDER_DEFAULT_LABEL == "Default"` |
| A2 | 生产测试 | `tier_display_label("none") == "None"` **且** `tier_display_label("low") == "Light"`、`("xhigh") == "Extra High"`（证明映射表没被误改） |
| A3 | 生产测试 | 卡片打开、指针不在块上时，`painted_quads()` 里**没有** `bg_hover` 色的标题块 quad（R1：非常驻） |
| A4 | 生产测试 | 指针移到**档位名行**上 → 出现一个 `bg_hover` 色 quad，其 bounds **同时包含**两行的 bounds（R3：两行一起） |
| A5 | 生产测试 | 指针移到**模型名行**上 → 同上，同一个块亮起（R3） |
| A6 | 生产测试 | 指针移出块 → 该 quad 消失（R1/R3） |
| A7 | 生产测试 | 点模型名行不触发 `ThinkingSliderTitleActivated`；点档位名行触发一次（R6 回归） |
| A8 | 生产测试 | 卡片宽度、两行行高、滑块 bounds、块的贴内容宽度与 R66 一致（R5 回归） |
| A9 | 实机像素 | 卡片打开时标题两行**无**灰底；鼠标移到标题上两行**同时**出现一个连续灰底 |
| A10 | 实机像素 | Off 档显示 `Off`（不是 `None`，也不是 `关闭`） |
| A11 | 实机像素 | 档位名仍首字母大写（`High`），`low` 档仍显示 `Light`（R11 回归） |
| A12 | 门禁 | `scripts/cargo-lock.sh test --workspace` 0 失败 |

**A3–A6 是本轮的核心证据**，因为它们把"hover 态"这件事从"看截图差不多"变成了可证伪的断言。实现者必须在报告里贴出实际读到的 quad 颜色与 bounds。

**A9/A10 仍需实机像素**：`painted_quads()` 证明的是"画了什么"，不是"合成后的观感"（阴影、圆角裁切、层叠）。两者不可互相替代。

---

## §5 待实测

| # | 项 | 状态 |
|---|---|---|
| M1 | `.hover()` 是否需要 `.id()` | **已解决** —— 需要。无 `id` 时永不重绘（§2 实测） |
| M2 | 子元素 hover 是否点亮父块 | **已解决** —— 是（§2 实测） |
| M3 | 测试平台能否观测绘制 | **已解决** —— `painted_quads()` 可以；`capture_screenshot` 不可以（§4） |
| M4 | hover 的圆角半径 | **未测**。块沿用 `rounded_md()`，与 R66 一致；如需精确值须实机取形 |
| M5 | 块的 hover 过渡是否带动画 | **未测**。R66 无过渡，本轮也不引入（避免改变观感） |

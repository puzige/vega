# R66 · 滑块卡片标题行的三处对齐（规格冻结）

> 状态：**SPEC FROZEN**
> 基线：`master @ 57fdeb2`
> 发现：2026-09-15，用户实机截图对照 Codex
> 前置：R62（三行布局）+ R65（胶囊填充）

---

## §1 三个问题

用户对照 Codex（参考实现）截图提出：

| # | 问题 | 用户原话 |
|---|---|---|
| 1 | 高亮范围 | *"选中的时候，模型跟 thinking level 要一起被选中。你参考我们第三张，我们只高亮了那个 thinking level。"* |
| 2 | 语言不一致 | *"关闭为什么是中文，其他的都是英文。"* |
| 3 | 大小写 | *"Max 它是 M 大写，但是我们的全小写，我们参考要首字母大写。"* |

---

## §2 问题 1：高亮范围 —— 两行必须作为**一组**一起高亮

### 用户原话

> *"选中的时候，模型跟 thinking level 要一起被选中。你参考我们第三张，我们只高亮了那个 thinking level。"*

**含义**：模型名与 thinking level 应**一起**被高亮（参考 Codex）；而 Vega 目前**只**高亮了 thinking level 一行。

### 实测对比（像素证据，2026-09-15）

用同一判定（浅灰 `240..249` 中性色块）量两张图：

| | 高亮块尺寸 | 覆盖行 |
|---|---|---|
| **Codex**（`/tmp/u-codex.png`，684×290） | **172 × 80 px**，x 287..458，y 84..163 | `Max ›` **和** `GPT-5.6 Luna` —— **两行** |
| **Vega**（`/tmp/u-highlight.png`，602×314） | **403 × 40 px**，x 90..492，y 64..103 | **仅** `high ›` 一行 |

**高度 80 vs 40 = 两行 vs 一行**，这是最直接的证据。

**宽度差异同样重要**：Codex 的块 172px **贴着文字内容**（两行中较宽者 + 内边距），而 Vega 的 403px **拉满了行宽**（`render_title_row` 带 `.w_full()`）。两者不是同一个设计。

### Vega 现状（代码）

`render_title_row`（`thinking_slider.rs:1122-1131`）有 hover 背景且是 `w_full()`：

```rust
.justify_center()
.w_full()                    // ← 高亮因此拉满行宽
.min_h(px(CARD_TEXT_ROW_HEIGHT))
.cursor_pointer()
.rounded_md()
.hover(move |style| style.bg(colors.bg_hover))
```

`render_model_row`（`:1190+`）**完全没有背景**。

两行是各自独立的 flex 子元素，**没有共同的容器**——这是必须改的结构问题。

### 契约

**R1（必须）** 档位名行与模型名行被**同一个高亮容器**包住，高亮块覆盖两行（含档位名行的 chevron）。

**R2（必须）** 高亮块**横向贴内容**，宽度取两行中较宽者 + 内边距，**居中**于卡片；**不得**拉满卡片宽度。

依据：Codex 实测 172px（内容宽），Vega 现状 403px（行宽）。实现者须让高亮块宽度由内容决定（如 `w_fit` / 内层 inline 容器），并保留 `justify_center` 的居中效果。

> 若实现上无法让宽度贴内容（GPUI 的 flex 限制），**必须在报告中说明**并给出实际宽度，不得静默改成拉满。

**R3（必须）** 高亮是**常驻**的（卡片打开即显示），不是仅在 hover 时出现。依据：Codex 截图里高亮块在无鼠标交互痕迹时即存在，且语义是"当前选中项"。

**R4（不得）** 不得改变卡片宽度、两行行高、两行垂直间距、滑块位置或卡片总高。

**R5（必须）** 两行仍是**上下两级**（R62 契约）：点档位名行仍触发 `ThinkingSliderTitleActivated`（进二级列表），点模型名行仍**无动作**。合并的是**视觉容器**，不是交互。

> ⚠️ 这条最容易做错：把两行合并成一个容器后，若把点击处理挂在容器上，模型名行会变得可点，破坏 R62 R3（模型行 inert）。**点击处理必须仍只挂在档位名行上。**

---

## §3 问题 2：`关闭` 应改为英文

### 现状

`OFF_LABEL`（`thinking_slider.rs:194`）= **`"关闭"`**。

而同一行的模型名是 `glm-5.3-flash`（英文）、档位名在参考实现里也是英文（`Max`/`High`）。用户指出**同框内语言不一致**。

### 参考实现的对应标签（权威）

`app-primary-6cd7b8b3f5e3.js` 的 `composer.mode.local.reasoning.*` 表，**最低档是 `none` → `None`**：

```js
none:{id:`composer.mode.local.reasoning.none.label`,defaultMessage:`None`,
      description:`Reasoning effort label for a given model: none`}
```

参考实现**没有**独立的 "Off"/"Disabled" 推理档位标签（`Disabled`/`Off` 的 message id 全属于其它功能：网络设置、外观、通知等）。

### 契约

**R6（必须）** `OFF_LABEL` 从 `"关闭"` 改为 **`"None"`**。

**R7（必须）** `PROVIDER_DEFAULT_LABEL`（`thinking_slider.rs:199`）= `"提供方默认"` 改为 **`"Default"`**。

依据（已查证）：参考实现 `composer.modelPicker.default.label` 的 `defaultMessage` 就是 **`Default`** —— 它是 **composer 自己的** message 键，与 `selectEffort.label`（`Select effort`）、`modelList.heading` 同属一个命名空间，正是同一语境。

> 不要用 `settings.agent.configuration.modelDefault` 的 `Model default` —— 那是 Settings 页的键，不是 composer 的。

**R8（必须）** `OFF_CHOICE_NAME`（持久化名 `"disabled"`）**不得**改动 —— 它是存储契约，与显示标签无关。

**R9（不得）** 不得改动任何其它中文字符串（本规格只涉及滑块卡片标题行的两个标签）。

---

## §4 问题 3：档位名首字母大写

### 参考实现的权威标签表

`app-primary-6cd7b8b3f5e3.js`，`composer.mode.local.reasoning.<tier>.label`：

| effort id | 显示标签 |
|---|---|
| `none` | `None` |
| `minimal` | `Minimal` |
| `low` | **`Light`** |
| `medium` | `Medium` |
| `high` | `High` |
| `xhigh` | **`Extra High`** |
| `max` | `Max` |
| `ultra` | `Ultra` |
| `persistent` | `Persistent` |

**注意两处非平凡映射**：
- `low` → **`Light`**（不是 `Low`）
- `xhigh` → **`Extra High`**（不是 `XHigh`）

### Vega 现状

`ThinkingSliderModel::label()`（`:431-438`）直接返回 effort id 原文：

```rust
Some(Selection::Tier(tier)) => tier.as_str(),   // "max" 原样显示
```

### 契约

**R10（必须）** 档位名显示走一张**显式映射表**，取值与 §4 表格逐字一致：

```rust
"none" => "None",  "minimal" => "Minimal",  "low" => "Light",
"medium" => "Medium", "high" => "High", "xhigh" => "Extra High",
"max" => "Max", "ultra" => "Ultra", "persistent" => "Persistent",
```

**R11（必须）** 未知 effort id 的兜底：**首字母大写化**（`xhigh` 之外的自定义值），而不是原样小写输出。参考实现的兜底是 `other {Other}`，但它用的是预定义枚举；Vega 的 effort 是 `Vec<String>`（可配），所以需要真实兜底。

**R12（不得）** 不得改动 effort id 本身（持久化契约）——只改显示。

**R13（必须）** 该映射必须是一个**纯函数**（如 `tier_display_label(effort: &str) -> String`），放在模型层而非渲染层，以便生产测试直接断言。

---

## §5 验收

| # | 证据 | 判据 |
|---|---|---|
| A1 | 生产测试 | `tier_display_label` 对 §4 表格九个 id 逐一返回规定字符串 |
| A2 | 生产测试 | `tier_display_label("somecustom")` 返回 `"Somecustom"`（首字母大写兜底） |
| A3 | 生产测试 | `OFF_LABEL == "None"`；`PROVIDER_DEFAULT_LABEL` 为英文 |
| A4 | 生产测试 | `OFF_CHOICE_NAME == "disabled"` 未变（R8 回归） |
| A5 | 生产测试 | 高亮容器的 debug bounds **同时包含**档位名行与模型名行的 bounds |
| A6 | 生产测试 | 卡片宽度、两行行高、滑块 bounds 与改动前一致（R4 回归） |
| A7 | 生产测试 | 点击模型名行**不触发** `ThinkingSliderTitleActivated`；点击档位名行**触发**（R5 回归，防止合并容器后交互被一起合并） |
| A8 | 实机像素 | 高亮块纵向覆盖两行（高度约为原单行高亮的两倍），横向为圆角矩形 |
| A9 | 实机像素 | 标题行文本为大写开头（`Max` 而非 `max`） |
| A10 | 实机像素 | 关闭档显示 `None`，与同框的英文模型名语言一致 |
| A11 | 门禁 | `scripts/cargo-lock.sh test --workspace` 0 失败 |

**A8/A9/A10 的绘制结果在测试里不可观测**（GPUI 测试平台无 headless renderer），由实机像素扫描证明。**不要拿 A5 冒充 A8。**

---

## §6 待实测

| # | 项 | 状态 |
|---|---|---|
| M1 | Codex 高亮块的横向宽度规则 | **已解决** —— 见 §2 实测（172px，贴内容；Vega 现状 403px 拉满行宽） |
| M2 | `PROVIDER_DEFAULT_LABEL` 的英文取值 | **已解决** —— `Default`（`composer.modelPicker.default.label`），见 R7 |
| M3 | Codex 高亮块的圆角半径 | **未测**。用户截图里可见明显圆角。实现者可目测近似（建议与档位名行现有 `rounded_md` 一致），但须在报告中说明取值 |
| M4 | Vega 是否有常驻高亮 | **已解决** —— 没有。标题行只有 `.hover(move \|style\| style.bg(colors.bg_hover))`，所以 R3 是**新增行为**而非修正 |

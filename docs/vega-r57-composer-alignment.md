# R57 · Composer 对齐参考实现 — 规格（冻结）

> 状态：**SPEC（待实现）**
> 基线：`master @ efc8ec0`
> 权威事实来源：`/Users/puzige/Workspace/vega-design-reference/`（源码）+ 用户实机截图（2026-09-14）
> 目标：`/Users/puzige/Workspace/vega`
> 用户决策：**B 方案——直接对齐参考实现**；**不要语音按钮**；思考档位面板参考 WorkBuddy 的「编辑模型」配置方式

---

## §1 用户决策记录

| 项 | 决策 |
|---|---|
| 总体方向 | **B：直接对齐参考实现**（含移除 Vega 独有控件） |
| 语音按钮 | **不做** |
| 思考档位 | 参考参考实现的 7 档滑块，但**档位按模型能力配置**（参考 WorkBuddy「编辑模型」面板） |

---

## §2 底部控制行：Vega vs 参考实现

### 2.1 当前差异（实测）

**参考实现**（图2，4 个元素）：
```
+   ⚠ Full access        [留白]        GPT-5.6 Luna Max ▾   🎤   ⏺
```

**Vega 现状**（5 个元素）：
```
+   Execute ▾   确认 ▾     [留白]     glm-5.3-flash ▾   提供方默认 ▾    ↑
```

| Vega 控件 | 代码位置 | 参考实现 | 处置 |
|---|---|---|---|
| `+` 按钮 | `render.rs:150-161`，selector `composer-add` | 有（`+`） | **保留** |
| `Execute ▾`（ThreadMode） | `render.rs:162` → `render_compact_settings(false)`，selector `composer-mode` | **无** | **移除下拉** |
| `确认 ▾`（PermissionMode） | `render.rs:163` → `render_compact_settings(true)`，selector `composer-permission` | 静态文本 `⚠ Full access` | **改为静态文本**（不可点） |
| `glm-5.3-flash ▾`（模型） | `render.rs:165` → `render_model_selector`，selector `composer-model` | 有，格式 `模型名 + Max` | **保留** |
| `提供方默认 ▾`（思考 chip） | `render.rs:166` → `render_thinking_control`，`.id("thinking-control")` | **无**（档位在模型弹窗内） | **移除独立 chip**，改为模型弹窗内的滑块 |
| `↑` 发送 | `render.rs:171-215`，selector `composer-send` | 蓝色圆形按钮 | 保留（样式可对齐） |

### 2.2 关键澄清：「提供方默认」不是 provider 选择器

调研确认（`core.rs:830-845`、`render.rs:762-766`）：该 chip 是**思考强度**的循环按钮，不是 provider 下拉。

```rust
let label = match level {
    "provider_default" => "提供方默认",
    "disabled" => "关闭",
    effort => effort,
};
```

**Vega 没有 provider 选择器**——provider 由模型推导（`app_agent.rs:419-436` `unique_provider_for_model`）。

### 2.3 契约

**R1（必须）** 移除 `Execute` 下拉（`render_compact_settings(false)`）。ThreadMode 的入口改由 `+` 菜单承载（已有 `/ask`、`/plan`、`/execute` 三项，`composer_actions.rs:5-9`）。

**R2（必须）** `确认` 从下拉改为**静态文本**，样式对齐参考实现的 `⚠ Full access`（warning 色文字 + 图标，不可点击）。权限修改入口移到 `+` 菜单或设置。

**R3（必须）** 移除独立的思考 chip（`render_thinking_control`），档位选择并入模型弹窗。

**R4（必须）** 不做语音按钮。

**R5（必须）** 底部行最终形态：`+` | 权限静态文本 | 留白 | 模型按钮 | 发送按钮。

---

## §3 思考档位面板（核心）

### 3.1 参考实现的机制（源码实证）

**档位全集**（`app-initial-cadb12d4a15e.js`）：
```js
['none','minimal','low','medium','high','xhigh','max','ultra','persistent']
```

**per-model 能力字段**（这是关键）：
```js
// 每个模型对象有：
supportedReasoningEfforts: [{ reasoningEffort: 'low' }, { reasoningEffort: 'medium' }, ...]
defaultReasoningEffort: 'medium'
```

**滑块只显示该模型支持的档位**：
```js
e.supportedReasoningEfforts.some(({reasoningEffort: t}) => t === e.reasoningEffort)
```

**档位不被支持时的降级** —— 参考实现里有**两种**，不是一种：

**主流路径（4+ 处使用，`Fxt` 函数）**：
```js
function Fxt({model, reasoningEffort}) {
  let n = model?.supportedReasoningEfforts ?? [];
  return (reasoningEffort != null && n.some(e => e.reasoningEffort === reasoningEffort))
    ? reasoningEffort
    : model?.defaultReasoningEffort ?? n[0]?.reasoningEffort ?? null;
}
// 顺序：当前档位（若支持）→ 模型默认档位 → 第一个支持的档位 → null
```

**边缘路径（1 处，`prepareRuntime`）**：
```js
let u = ['none','minimal','low','medium','high','xhigh','max','ultra','persistent'],
    d = u.find((e,t) => t < u.indexOf(c) && l?.supportedReasoningEfforts.some(...))
// Array.find 从索引 0 向上扫 → 返回**最低的**受支持档位（不是"最近的"）
```

> **⚠️ 本规格此前的错误**：§3.1 正文曾把上面第二种写成"降级到最近一档"，与所引代码（返回最低档）矛盾，且两种都不是参考实现的主流行为。**以本节为准。**
>
> **实现采用的语义**（P2a 切片，已由测试固定）：当前档位 → 最近的低档 → 配置默认档 → 第一个支持的档 → 无。其中「最近的低档」是规格正文的原意，而「配置默认档」与参考实现主流路径一致；第 3–5 步覆盖了参考实现未定义的边界。


**默认档位选择**：
```js
function Fxt({model, reasoningEffort}) {
  let n = model?.supportedReasoningEfforts ?? [];
  return (reasoningEffort != null && n.some(e => e.reasoningEffort === reasoningEffort))
    ? reasoningEffort
    : model?.defaultReasoningEffort ?? n[0]?.reasoningEffort ?? null;
}
```

### 3.2 面板视觉（用户截图实测）

**Ultra 态**（紫，knob 最右）与 **Medium 态**（蓝，knob 第 2 档）对比得出：

| 元素 | 值 |
|---|---|
| 卡片圆角 | 大圆角（≈16-20px） |
| 左上 | 闪电图标（灰） |
| 中上 | 档位名 + chevron，**颜色随档位变化**（Ultra 紫 `#A76FFF` 系 / Medium 蓝 `#3478D8` 系） |
| 右上 | 重置图标（圆形箭头） |
| 中 | 模型名（灰，如 `GPT-6 Astra`） |
| 下 | 滑块轨道 + **7 个圆点** + 可拖 knob |
| 已填充区 | 渐变/纯色，**点变白** |
| 未填充区 | 浅灰底，**点变灰** |

**档位与点数**：Medium 态显示「2 档填充 + 5 灰点 = 7 档」。

**触发器**：composer 里显示 `Select effort ▾`；已选档位时显示档位名。

### 3.3 Vega 现状与差距

| 项 | Vega 现状 | 差距 |
|---|---|---|
| 档位定义 | `["minimal","low","medium","high","xhigh","max"]`（6 档，`reasoning_state.rs:6-7`） | 参考实现 9 档全集；Vega 缺 `none`/`ultra`/`persistent` |
| per-model 能力 | **已有** `ReasoningProfile.efforts: Vec<String>`（`vega_store/src/reasoning.rs:145-171`） | **已具备**，无需新建 |
| 默认档位 | `ReasoningProfile.preference: String` | 已有 |
| 设置 UI | **已有** Reasoning 页（`reasoning_render.rs`），可逐档 toggle | 已有 |
| 面板形态 | 无（chip 循环切换） | **需新建滑块面板** |

**结论：Vega 的能力配置层已存在**（`reasoning.toml` 的 `ReasoningProfile`），本轮只需**新建滑块 UI** 并接到已有数据。

### 3.4 契约

**R6（必须）** 新建思考档位弹窗组件，从模型按钮打开（或模型按钮旁）。

**R7（必须）** 滑块档位数 = **该模型 `ReasoningProfile.efforts` 的长度**（不是固定 7）。参考实现固定 7 是因为其模型支持 7 档；Vega 必须按模型能力渲染。

**R8（必须）** 档位名颜色随档位变化（对齐参考实现：高强度用紫、中低用蓝）。**具体色值需实测确认**，不得凭空指定。

**R9（必须）** 未填充区的点显示为灰色，已填充区显示为白色。

**R10（必须）** 档位不被支持时的降级顺序（见 §3.1 修正后的定义）：
1. 当前档位（若被支持）
2. 否则「比当前低且被支持」的**最近**一档
3. 否则配置的默认档位（若被支持）
4. 否则第一个被支持的档位
5. 否则不渲染滑块

**R11（必须）** 重置按钮：重置到 `ReasoningProfile.preference`（即配置的默认档位）。

**R12（必须）** 模型不支持任何档位时（`efforts` 为空），不显示滑块，改为普通开关或隐藏——**需实测参考实现的降级形态**。

---

## §4 实测数据（2026-09-14，从用户截图取色/测量）

> 截图源：`Ultra` 态 632×318（2x → 逻辑 316×159）；`Medium` 态 642×260（逻辑 321×130）。
> **证据等级**：截图取色只能证明**视觉结果**，不能证明内部 token 或合成算法。以下作为**视觉目标值**使用。

### 4.1 档位名颜色（实测）

| 档位 | 实测色值 | 采样 |
|---|---|---|
| Ultra | **`#924FF7`** = `(146,79,247)` | 264 px |
| Medium | **`#3983F7`** = `(57,131,247)` | 280 px |

**规律**：档位名颜色随强度变化——高强度紫（`#924FF7`），中低强度蓝（`#3983F7`）。

### 4.2 轨道颜色（实测）

**Ultra 态（渐变，多段）**：

| 位置 | 逻辑 x | 色值 |
|---|---|---|
| 起点 | 33 | `(51,70,205)` = `#3346CD` 深蓝 |
| 中段 | 163 | `(174,118,255)` = `#AE76FF` |
| 中段 | 198 | `(190,155,255)` = `#BE9BFF` |
| 末端 | 235 | `(126,90,240)` = `#7E5AF0` |

即**深蓝 → 紫 → 末端回落**，是多段渐变，非简单两色线性。

**Medium 态（纯色）**：

| 区域 | 色值 |
|---|---|
| 已填充 | **`#3983F7`** = `(57,131,247)` |
| 未填充 | **`#E9E8E8`** = `(233,232,232)` |

**关键**：Medium（低档）用**纯色**，Ultra（最高档）用**渐变**。这说明轨道样式与档位强度相关。

### 4.3 几何（Ultra 态，实测，逻辑 px）

| 项 | 值 | 备注 |
|---|---|---|
| 轨道左缘 | **33.0** | |
| 轨道右缘（含 knob） | **236.0** | |
| 轨道总宽 | **203.0** | |
| 轨道上缘 | **82.0** | |
| 轨道下缘 | **106.0** | |
| **轨道高度** | **24.0** | 82..106 |
| knob | 直径 ≈24（与轨道等高），纯白圆 | |
| 卡片左缘 | 20.5 | |
| 卡片右缘 | 275.0 | |
| **卡片宽度** | **254.5** | |
| 卡片圆角 | ≈20（据轮廓） | |

### 4.4 结构（两态对比确认）

| 元素 | Ultra 态 | Medium 态 |
|---|---|---|
| 档位名 | `Ultra` + `>` chevron | `Medium` + `>` chevron |
| 模型名 | `GPT-6 Astra`（灰） | `GPT-6 Astra`（灰） |
| 左上 | 闪电图标（灰描边） | 同 |
| 右上 | 圆形箭头（重置） | 同 |
| 轨道填充 | 全满（渐变） | 前 2 档（蓝） |
| 未填充点 | 0 个 | **5 个**（灰） |
| 已填充点 | 7 个（白） | 2 个（白） |
| **总档位** | **7** | **7** |

**关键规律**：已填充区域的点渲染为**白色**，未填充区域的点渲染为**灰色**。

### 4.5 仍待实测

| # | 待确认 | 为什么需要 |
|---|---|---|
| M1 | 7 个点的**间距与直径** | 需逐点定位 |
| M2 | 各档位的**颜色映射规则**（哪个档位开始变紫） | 只测了 Ultra 紫、Medium 蓝两个点，中间档未知 |
| M3 | 模型不支持档位时的降级形态 | 参考实现源码有降级逻辑，但 UI 形态未见 |
| M4 | `Select effort` 触发器已选档位后的文案 | 参考实现源码未明确 |
| M5 | 卡片与轨道的精确内边距 | 影响还原度 |

> **实现约束**：M1–M5 未确认前，不得凭空指定。可先实现**结构与逻辑**（档位数、降级、交互、颜色按档位强度分级），视觉细节待补测后校准。


---

## §5 明确不做

- 语音按钮（用户明确）
- Cloud / Remote 运行位置（用户明确只做 local）
- `ultra` / `persistent` 档位**语义实现**（Vega 的 provider 是否支持需另定；本轮只做 UI，档位来自已有 `ReasoningProfile.efforts`）
- 不改 provider / 数据库契约
- 不改 `reasoning.toml` 的 schema

---

## §6 验收

| # | 证据 | 判据 |
|---|---|---|
| A1 | 生产测试 | 底部行元素数量与顺序符合 R5 |
| A2 | 生产测试 | 移除 `composer-mode` / `composer-permission` 菜单后，ThreadMode / PermissionMode 仍可通过新入口修改 |
| A3 | 生产测试 | 档位滑块按 `ReasoningProfile.efforts` 长度渲染（构造 3 档 / 6 档 / 0 档模型验证） |
| A4 | 生产测试 | 降级逻辑：当前档位不被支持时降到正确的档 |
| A5 | 原生截图 | 弹窗视觉与参考实现对齐（色值/几何按 §4 实测值） |
| A6 | 门禁 | `scripts/cargo-lock.sh test --workspace` 0 失败；fmt/clippy 干净 |

**既有测试影响**：`r21_composer_popovers_are_exclusive_and_model_labels_keep_menu_width`（`crates/vega_ui/src/conversation_stream/tests/core_flow.rs:78`，函数体约 81 行）断言了 `composer-mode` 与 `composer-model` 菜单互斥——移除 mode 菜单后须更新并说明理由。

---

## §7 已知未覆盖

- 思考档位的**实际 wire 发送**已存在（`openai/mod.rs:281-308`），本轮不改。
- 模型按钮的 `模型名 + Max` 复合格式（参考实现）与 Vega 的纯模型名——是否对齐待定。

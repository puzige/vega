# R59 · 滑块与模型列表改为两级下钻（规格冻结）

> 状态：**SPEC FROZEN**
> 基线：`master @ 4ac6bfb`
> 用户决策（2026-09-14）：按参考实现的操作逻辑修正
> 前置：R57（滑块）+ R58（Off 档）

---

## §1 问题：当前实现把两层混成一个菜单

**用户实机反馈（2026-09-14）**：

> 第一次点击 composer 下面的模型，它会弹出一个滑块，然后你再点模型名，它才会出列表让你去调整模型。

**当前实现的缺陷（用户截图实证）**：

| # | 现象 | 根因 |
|---|---|---|
| D1 | 滑块卡片浮在模型列表上，"非常混乱" | 滑块被挂成模型菜单的**最后一个 child**（`render.rs:535-544`），两者叠在同一容器内 |
| D2 | utility bar 无法点击 | 菜单 `.absolute().bottom(28.)` 向上展开（`render.rs:483-487`），加滑块后总高约 218px，**无 `max_h` 约束**，溢出压住 utility bar |
| D3 | 滑块卡片内部重叠 | 卡片内容行高与轨道定位冲突 |

## §2 参考实现的真实交互（源码实证）

**两级下钻，一次只显示一层：**

**第一级** —— 点模型按钮 → **只弹滑块**
```
⚡  GPT-6 Astra Light  >        ← 标题：模型名(主色) + 档位名(次级色)
[●●●◉○○○]                       ← 滑块
```

**第二级** —— 点滑块里的 `GPT-6 Astra Light >` → **弹模型列表**
```
Select model
  Default  ✓   Recommended set of models
  GPT-6 Astra
  GPT-5.6 Sol
  GPT-5.6 Terra
  ...
```

**源码依据**（`app-primary-6cd7b8b3f5e3.js`）：
```js
composer.modelPicker.modelList.open.ariaLabel:
  "Accessible label for the selected-model action above the model-picker
   slider, which opens the list of available models."
  → 滑块在模型列表之前；滑块内的模型名是打开列表的入口

composer.modelPicker.selectEffort.label: "Select effort"
  "Placeholder shown in the model picker trigger while adjusting the
   reasoning effort of an explicitly selected model."
  → 调整档位时，触发器显示 "Select effort" 而非模型名

composer.modelPicker.modelList.heading: "Select model"
composer.modelPicker.default.description: "Recommended set of models"
```

## §3 契约

### R1（必须）滑块是模型按钮的**第一级**

点模型按钮 → **只显示滑块**，不显示模型列表。

**当前行为**（滑块 + 列表同屏）→ 改为只显示滑块。

### R2（必须）滑块标题是打开模型列表的入口

滑块卡片内的标题（`模型名 + 档位名 + >`）点击后**切换到模型列表**。

- 标题文字**分两色**：模型名用主色，档位名用次级色（参考实现截图实证）
- 箭头 `>` 表示"可下钻"

### R3（必须）模型列表是**第二级**，且与滑块互斥

- 从滑块标题进入模型列表后，**滑块隐藏**
- 选择模型后 → 回到滑块（或关闭，由实现者判断并在报告中说明）
- 两态**不得同屏**

### R4（必须）滑块卡片不得嵌在模型列表容器内

滑块与列表是**两个独立的浮层**，各自有自己的边框/背景/阴影。当前"卡片套卡片"的视觉必须消除。

### R5（必须）菜单不得溢出压住 utility bar

- 菜单与滑块都要有高度约束，**不得覆盖 utility bar**
- utility bar 必须保持可点击

### R6（必须）修卡片内部行距

滑块卡片内 `模型名` 行不得与轨道重叠。

### R7（必须）触发器文案

- 调整档位时（滑块态）：触发器显示 `Select effort`（参考实现）
- 已选模型时：显示模型名（现有行为）
- **中文文案由实现者按现有 i18n 惯例处理**；Vega 现有界面为中文，可译为「选择强度」等。若不确定，沿用现有模型名显示并说明。

## §4 不做

- 不改 R57 已冻结的底部行形态
- 不改 R58 的 Off 档语义（仍走 `Disabled`）
- 不改 `reasoning.toml` schema 与 wire 编码
- 不做语音按钮
- 不做重置按钮

## §5 验收

| # | 证据 | 判据 |
|---|---|---|
| A1 | 生产测试 | 点模型按钮 → 只渲染滑块，**不渲染**模型列表 |
| A2 | 生产测试 | 点滑块标题 → 渲染模型列表，**不渲染**滑块 |
| A3 | 生产测试 | 两态互斥（同一时刻只有一个 mounted） |
| A4 | 生产测试 | 选择模型后回到滑块态（或按实现者选择的行为） |
| A5 | 原生截图 | 滑块与列表不同屏；utility bar 可见且可点击 |
| A6 | 门禁 | `scripts/cargo-lock.sh test --workspace` 0 失败 |

## §6 已知未实测

| # | 项 |
|---|---|
| M1 | 选择模型后参考实现是回到滑块还是关闭 |
| M2 | 滑块标题的确切字号与两色色值（需从截图取色） |
| M3 | `Select effort` 的中文对应文案 |

> 依据：M1–M3 无实测样本，实现者按最小侵入处理并标注。

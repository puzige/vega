# R61 · 模型选择浮层改回触发器锚定（规格冻结）

> 状态：**SPEC FROZEN**
> 基线：`master @ bb45c36`
> 用户决策（2026-09-14）：**选 B —— 允许遮住 utility bar**
> 前置：R59（两级下钻已实现）

---

## §1 问题

R59 为了满足「不遮 utility bar」，把浮层从触发器锚定改为 **composer column 顶边锚定**。副作用（用户实机反馈 + 截图）：

| # | 现象 | 根因 |
|---|---|---|
| **D1** | 滑块卡片离模型按钮**太远** | 锚点是 column 顶边，中间隔了整个 utility bar 的高度 |
| **D2** | 卡片**整体偏右**，不对齐任何参照物 | 浮层 `items_end` 右对齐到 `max_w(COMPOSER_MAX_WIDTH=736)` 容器，而卡片仅 `THINKING_CARD_WIDTH=254.5` |

**R59 报告的空间测算**：
```
触发器顶 1022 − utility bar 底 961 = 61px 可用
滑块卡片需要 84px
缺口 23px
```

即：**触发器锚定必然遮住 utility bar 约 23px**——这是几何上的硬约束，不可调和。

## §2 用户决策

**允许遮住 utility bar**（方案 B）。优先「卡片紧贴模型按钮」，而非「utility bar 完整可见」。

## §3 契约

### R1（必须）浮层锚定回模型触发器

- 锚点：`composer-model` 触发器的**上缘**
- 水平：右对齐到**触发器的右缘**（不是容器右缘）
- 间距：触发器上缘与卡片下缘之间留 `PICKER_TRIGGER_GAP`（建议 8px；实现者可按视觉调整并在报告中说明）

触发器已有 `.relative()` 上下文（`render.rs:500`），可直接用 `absolute().bottom(relative(1.0))`。

### R2（必须）移除 column 顶边锚定

删除 `render_model_picker_overlay` 中的 `.bottom(gpui_kit::relative(1.0))` + `.left_0()` + `.right_0()` + `max_w(COMPOSER_MAX_WIDTH)` + `mx_auto` + `items_end` 这套 column 锚定结构（`render.rs:591-617`）。

### R3（必须）保留高度上限

`COMPOSER_PICKER_MAX_HEIGHT = 320`（列表内部滚动）**必须保留**——它防止 40 个模型时弹层无限增高。这是 R59 的正确改进，不得回退。

### R4（必须）R59 的两级下钻语义不变

- 点模型按钮 → 只显示滑块
- 点滑块标题 → 显示模型列表
- 两态互斥（`ModelPickerLevel` 保证）
- **不得**回退为「同屏显示」

### R5（必须）接受 utility bar 被部分遮挡

- 不再断言「浮层边界不越过 utility bar」
- **改写** R59 的相关测试：从「不得遮挡」改为「允许遮挡，但卡片必须紧贴触发器」——即断言**卡片下缘与触发器上缘的距离 ≤ 阈值**（如 ≤ 12px）
- **不得**直接删除该测试

### R6（必须）遮住 utility bar 时仍可关闭

浮层打开时，用户必须能关闭它（点外部、Esc、或再点触发器）。**若遮住 utility bar 导致其 chip 不可点，这是可接受的**（用户可通过关闭浮层恢复）。但**关闭路径必须存在且可用**。

## §4 明确不做

- 不缩小滑块卡片（用户未选方案 A）
- 不改两级下钻语义
- 不改底部行形态
- 不改 `reasoning.toml` / wire 编码

## §5 验收

| # | 证据 | 判据 |
|---|---|---|
| A1 | 生产测试 | 卡片下缘与触发器上缘距离 ≤ 12px（紧贴） |
| A2 | 生产测试 | 卡片右缘与触发器右缘对齐（±2px） |
| A3 | 生产测试 | 两级互斥仍成立 |
| A4 | 生产测试 | 列表仍有 `COMPOSER_PICKER_MAX_HEIGHT` 上限 |
| A5 | 生产测试 | 浮层打开时存在可用关闭路径（Esc 或外部点击） |
| A6 | 原生截图 | 卡片紧贴模型按钮上方 |
| A7 | 门禁 | `scripts/cargo-lock.sh test --workspace` 0 失败 |

## §6 待实测

| # | 项 |
|---|---|
| M1 | `PICKER_TRIGGER_GAP` 的最终值（8px 是建议值，需视觉确认） |
| M2 | 遮挡 utility bar 后，其文件夹/分支 chip 是否仍可见（若完全遮住，用户需先关浮层） |

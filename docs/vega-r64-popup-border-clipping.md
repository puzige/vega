# R64 · composer 弹出层被卡片边框切穿（规格冻结）

> 状态：**SPEC FROZEN**
> 基线：`master @ 3739586`
> 发现：2026-09-14，用户实机截图（红框标出模型卡片上的一条横线）
> 影响：**所有**从 composer 卡片内浮起的弹出层

---

## §1 现象

模型卡片浮在 composer 之上时，卡片中部有一条**贯穿的细横线**，并向左右延伸到卡片之外（用户截图红框）。

2026-09-14 在 `master @ 3739586` 的打包构建上复现，并用像素扫描定量确认：

| 属性 | 实测 |
|---|---|
| 颜色 | `rgb(232,232,232)` = `border_subtle` `#E8E8E8`（`vega_theme/src/lib.rs:110`） |
| 厚度 | 2 物理像素（= 1 逻辑像素 @2×） |
| 位置 | 物理 y=1482–1483 → 窗口内逻辑 y≈741 |
| 水平跨度 | 逻辑 x≈401..1052，**比弹出层宽得多** |

关键证据：该线在**弹出层关闭时同样存在**（`/tmp/L3-closed.png` 与 `/tmp/L1.png` 在 x=1100 列逐像素完全相同）。所以它不是弹出层自己画的，而是**常驻布局元素**——composer 卡片的上边框。

## §2 根因（GPUI 源码核实）

`gpui-pre-0.3.4/src/style.rs:688` 的 `Style::paint` 顺序是：

```rust
window.paint_drop_shadows(...);
window.paint_quad(background);      // 1. 背景
window.paint_inset_shadows(...);
continuation(window, cx);           // 2. 子元素（弹出层在这里画）
if self.is_border_visible() {
    window.paint_quad(border);      // 3. 边框 ← 在子元素之后
}
```

**边框画在子元素之后**。composer 卡片带 `.border_1()`（`render.rs:177-178`），而所有弹出层都是它的后代（挂在 `render_model_selector` / `render_permission_status` 的 `.relative()` 包装器里），所以卡片的边框必然覆盖弹出层。

**`occlude()` 不能解决这个问题**。`gpui-pre-0.3.4/src/elements/div.rs:1208` 的 `Div::occlude` 只做一件事：

```rust
fn occlude(mut self) -> Self {
    self.interactivity().occlude_mouse();   // 只设 HitboxBehavior::BlockMouse
    self
}
```

它只影响**鼠标命中测试**，不影响绘制顺序。R61/R62 在三个层上都写了 `.occlude()` 并据此认为层级已处理——这个推断是错的。

## §3 对照实验（决定性）

同一个仓库里已有正确的做法：**分支下拉**用了 `gpui_kit::deferred(popup).with_priority(2)`（`branch_selector.rs:997`），它不被划过。同一行的像素扫描：

| 弹出层 | deferred | row 1482 在弹出层 x 范围内 |
|---|---|---|
| 分支下拉 | ✅ `with_priority(2)` | `rgb(255,255,255)` — **无边框线** |
| 权限下拉 | ❌ | `rgb(232,232,232)` — **被切穿** |
| 模型卡片 / 列表 | ❌ | `rgb(232,232,232)` — **被切穿** |
| 项目菜单 | ❌ | `rgb(232,232,232)` — **被切穿** |

`deferred` 的语义（`gpui-pre-0.3.4/src/elements/deferred.rs:11`）：*"delays the painting of its child until after all of its ancestors, **while keeping its layout as part of the current element tree**"*。绘制延后由 `window.rs:3343` 的 `paint_deferred_draws()` 承担，它在 `root_element.paint()`（画完整棵树，含卡片边框）**之后**执行。

**关于坐标**：`defer_draw` 接收并记录 `absolute_offset`（`window.rs:4105`），布局与坐标完全保留。R59 记录过"deferred 导致卡片落在 x 1232.5"，但那是 R59 把 deferred 包在 `max_w(COMPOSER_MAX_WIDTH) + mx_auto` 的**外层盒子**里所致（R61 已删除该结构），**不是 deferred 本身的问题**。本规格要求用实验证伪这一点，见 §5 A3。

## §4 契约

**R1（必须）** 以下四个弹出层加 `gpui_kit::deferred(...).with_priority(2)`：

| # | 函数 | 文件 |
|---|---|---|
| 1 | `render_permission_picker` | `render.rs:423` |
| 2 | `render_picker_slider_layer` | `render.rs:878` |
| 3 | `render_picker_list_layer` | `render.rs:913` |
| 4 | 项目菜单（`render_utility_projects_menu` 的 `menu` 值） | `utility_bar.rs:182` |

**R2（必须）** 每处只包 `deferred`，**不得**改动锚定结构：`.absolute().bottom(relative(1.0)).mb(COMPOSER_PICKER_TRIGGER_GAP).right_0()` / `.left_0()` 全部保持原样。R61 的触发器锚定契约不变。

**R3（必须）** 优先级统一用 `2`（与 `render_file_dropdown` 和分支下拉一致）。弹出层之间互斥（`close_composer_popovers`），不存在同优先级竞争。

**R4（必须）** 保留各层既有的 `.occlude()`——鼠标命中测试仍需要它，只是它不再被当作层级方案。

**R5（必须）** 不得为了绕开这个问题而删除 composer 卡片的边框，也不得改成"边框画成背景"之类的 token 改造。

**R6（不得）** 不得把 `+` 菜单（`render_composer_actions`）纳入本次改动。它是**流内元素**（无 `.absolute()`），打开时把输入行往下推、不覆盖卡片边框，因此没有本缺陷；它的形态问题是另一件事（见 §7）。

## §5 验收

| # | 证据 | 判据 |
|---|---|---|
| A1 | 生产测试 | 四个层各自渲染时，其 debug_selector 的 bounds 与改动前**逐像素相同**（deferred 不改变布局） |
| A2 | 生产测试 | 弹出层打开时，卡片边框不再穿过：断言层的 bounds 与卡片 bounds 相交区域内不存在「边框绘制在层之上」——若测试平台无法观测绘制顺序，改为断言 `deferred` 已挂载（见下） |
| A3 | 实机像素 | 模型卡片打开时，扫描卡片中部那一行，**不得**出现 `rgb(232,232,232)` 连续 run；对照：弹出层关闭时该行仍应有卡片边框 |
| A4 | 实机像素 | 权限下拉、项目菜单同样不得被切穿 |
| A5 | 实机像素 | 分支下拉（本来就正确）保持无切穿——回归对照 |
| A6 | 实机像素 | 弹出层的**位置与尺寸**与改动前一致（R61 锚定契约未破） |
| A7 | 门禁 | `scripts/cargo-lock.sh test --workspace` 0 失败 |

**关于 A2**：GPUI 测试平台不带 headless renderer（`render_to_target` 报 "no HeadlessRenderer configured"），**无法**在测试里读绘制顺序。因此 A2 的实现是：断言层的 debug bounds 与改动前一致（A1 覆盖），绘制顺序由 A3/A4 的实机像素扫描证明。**不要把 A1 当成 A2 的替代**，报告里要写明这一点。

## §6 为什么 R62/R63 验收没抓到

我的 R62 验收截图（`/tmp/accept-permission-picker.png` 等）**已经拍到了这条线**，但我当时把注意力放在"三行内容对不对"，看到卡片与 composer 重叠就归结为"阴影让它浮起来了"，没有逐行扫描像素。

**教训**：弹出层类的验收不能只看"内容对不对"，必须做一次**像素行扫描**，确认层内没有其他层画上去的元素。已写入 `AGENTS.md`。

## §7 附带记录（不在本次范围）

`+` 菜单（`render_composer_actions`）不是浮层：它无 `.absolute()`，在输入行的 `.relative()` 容器里作为**流内兄弟**渲染（`render.rs:196`），打开时把输入框往下推。参考实现是浮层。这是形态差异，**另立任务**，本次不动。

---

## §8 待实测

| # | 项 |
|---|---|
| M1 | `deferred` 加入后，弹出层的 shadow 是否仍完整绘制（deferred 层是否影响阴影） |
| M2 | 弹出层的 `snap_to_window` 行为是否受影响（模型列表靠近窗口边缘时） |
| M3 | 四个层同时只开一个的互斥契约是否仍成立（`close_composer_popovers`） |

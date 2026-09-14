# R64 实机验收报告（composer 弹出层被卡片边框切穿）

> 日期：2026-09-14
> 基线：`master @ 8689f52`（R64 修复提交，基于规格 `0af289c`）
> 受测构建：`dist/Vega.app` → `/Applications/Vega.app`，可执行文件 `md5 04fe97f0ab44f76c0ca62e64c3e7a460`
> 签名：`codesign --verify --deep --strict` 通过
> 修复前证据：`/tmp/L1.png`（模型卡片被切穿）、`/tmp/L2-line-zoom.png`（线特写）
> 修复后证据：`/tmp/accept-r64-*.png`

---

## §1 结论

**修复通过实机验证。** 三个受影响的弹出层（模型卡片、权限下拉、项目菜单）都不再被 composer 卡片的边框切穿，且弹出层的位置与尺寸未变。

| 验收项 | 结论 | 证据类型 |
|---|---|---|
| A1 四个层几何与改动前逐像素相同 | ✅ 通过 | 生产测试（4 条，我独立证伪） |
| A3 模型卡片不再被切穿 | ✅ 通过 | 实机像素扫描 |
| A4 权限下拉不再被切穿 | ✅ 通过 | 实机像素扫描 |
| A4 项目菜单不再被切穿 | ✅ 通过 | 实机像素扫描 |
| A5 分支下拉（本来就正确）无回归 | ✅ 通过 | 生产测试 + 未改动 |
| A6 弹出层位置尺寸不变 | ✅ 通过 | 生产测试（基线字面量比对） |
| A7 门禁 0 失败 | ✅ 1149 passed / 0 failed | 生产测试 |
| A2 绘制顺序 | ⚠️ **测试不可观测**，由 A3/A4 的像素证据承担 | 见 §4 |

---

## §2 缺陷与根因

### 现象

模型卡片浮在 composer 之上时，卡片中部有一条贯穿的细横线，并向左右延伸到卡片之外。

### 根因（GPUI 源码核实，R64 规格 §2）

`gpui-pre-0.3.4/src/style.rs:688` 的 `Style::paint` **先画子元素、后画边框**：

```rust
window.paint_quad(background);   // 背景
continuation(window, cx);        // 子元素（浮层在这里）
if self.is_border_visible() {
    window.paint_quad(border);   // 边框 ← 在子元素之后
}
```

composer 卡片带 `.border_1()`（`render.rs:177`），所有弹出层都是它的后代，所以卡片边框必然覆盖弹出层。

**`occlude()` 不是层级方案**：`div.rs:1208` 的 `Div::occlude` 只设 `HitboxBehavior::BlockMouse`（鼠标命中测试）。R61/R62 给每个层都加了它并据此认为层级已处理——该推断错误。

### 决定性对照实验

同仓库的**分支下拉**用了 `gpui_kit::deferred(...).with_priority(2)`，它不被切穿。同一行像素扫描：

| 弹出层 | deferred | row 1482 在弹出层 x 范围内 |
|---|---|---|
| 分支下拉 | ✅ | `rgb(255,255,255)` — 干净 |
| 权限下拉 | ❌ | `rgb(232,232,232)` — 被切穿 |
| 模型卡片/列表 | ❌ | `rgb(232,232,232)` — 被切穿 |
| 项目菜单 | ❌ | `rgb(232,232,232)` — 被切穿 |

---

## §3 修复内容

四个浮层各包一层 `gpui_kit::deferred(...).with_priority(2)`：

| # | 函数 | 文件 |
|---|---|---|
| 1 | `render_permission_picker` | `conversation_stream/render.rs` |
| 2 | `render_picker_slider_layer` | 同上 |
| 3 | `render_picker_list_layer` | 同上 |
| 4 | 项目菜单值 | `conversation_stream/utility_bar.rs` |

**锚定契约逐字节未变**（R64 R2）。我用空白不敏感 diff 核对，改动只有：`deferred(` / `.with_priority(2)` 两行、配套的括号闭合、rustfmt 缩进，以及一处**注释订正**（`render_model_picker_layers` 原注释断言"这些层故意不用 deferred，因为 deferred 破坏 composer 相对坐标"——该断言被 R64 §3 证伪，留着会让文件自相矛盾）。

`.absolute()` / `.bottom(relative(1.0))` / `.mb(COMPOSER_PICKER_TRIGGER_GAP)` / `.right_0()` / `.left_0()` / `.occlude()` 全部保留，`occlude()` 计数与基线相同（render.rs 4 处、utility_bar.rs 1 处）。

**未动** `+` 菜单（R64 R6）：它是流内元素（无 `.absolute()`），把输入行往下推、不覆盖卡片边框，因此无此缺陷。它的形态差异（参考实现是浮层）记在 R64 §7，另立任务。

---

## §4 门禁与证伪（我本人独立执行）

### 门禁

| 检查 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | 退出 0，无输出 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 退出 0 |
| `cargo test -p vega_ui` | **284 passed / 0 failed**（基线 280，+4 为新增 A1 测试） |
| `cargo test --workspace` | **1149 passed / 0 failed**（基线 1145，+4） |

### 证伪：A1 测试不是空跑

A1 断言"几何逐像素不变"。若它连几何变化都抓不到就是空跑。我把 `COMPOSER_PICKER_TRIGGER_GAP` 从 8.0 临时改为 14.0：

```
r64_picker_slider_layer_bounds_match_the_baseline ... FAILED
r64_permission_picker_bounds_match_the_baseline ... FAILED
r64_picker_list_layer_bounds_match_the_baseline ... FAILED
panicked: Expected x=832.5 y=905 w=254.5 h=109, got x=832.5 y=899 w=254.5 h=109
```

**精确抓住了 6px 位移**（正是 8→14 的差）。测试非平凡。随后已还原（`git diff` 为空）。

---

## §5 实机像素验证（修复前后对照）

同一行（物理 y=1482，逻辑 y=741）、同一位置扫描：

| 场景 | 修复前 | 修复后 |
|---|---|---|
| 模型卡片内（x 1700..2300） | `rgb(232,232,232)` 连续 run | `rgb(253,253,253)` → `rgb(255,255,255)`，**无 232** |
| 权限下拉内（x 1100..1250） | `rgb(232,232,232)` 满行 | 卡片边框只从 **x=1571** 起（在下拉右侧），下拉 x 范围 889..1534 **未被覆盖** |
| 项目菜单内（x 867..1078） | `rgb(232,232,232)` 满行 | 无 232 连续 run；菜单自身边框在 y=1418 |

**三个层全部干净。** 视觉上（`/tmp/accept-r64-model-card-fixed-crop.png`）卡片已完整浮在 composer 之上，无横线。

---

## §6 未能验证的部分（明确记录）

**A2（绘制顺序）在生产测试里不可观测。** GPUI 测试平台不带 headless renderer（`render_to_target` 报 "no HeadlessRenderer configured"），`debug_bounds` 是测试能拿到的唯一几何量，且它在 `Div::paint` 里由布局边界填充——**任何测试都无法观测"哪个 quad 最后被画"**。

因此：

- 测试里**没有**、也不应该有假装断言绘制顺序的用例。
- **A1 不能替代 A2**。A1 证明的是"deferred 没有移动/缩放任何东西"。
- A2 的证据是 §5 的实机像素扫描。

这一点在规格 §5 就写明了，实现者也如实报告未声称 A2，没有粉饰。

---

## §7 我的验收遗漏（本轮最该记住的一条）

**R62 验收时，我的截图里已经有这条线，但我没识别出来。**

`/tmp/accept-permission-picker.png` 等 R62 证据图中，卡片中部就有这条 `#E8E8E8` 线。我当时看到卡片与 composer 重叠，把它归结为"阴影让它浮起来了"（那是 R61 用户批准的 B 方案：允许重叠但必须浮起），**没有逐行扫描像素确认层内是否混入了其他层画上来的元素**。

**这是第二次同类错误**：R57 时我读到源码里 `shouldShowFullAccessWarning` 就断定 `Full access` 不可点（用户用截图纠正）；R51 时我把 `showUtilityBarBranchWhen` 当成整条栏的开关。**共同模式是：用推理代替观察，或只做粗粒度观察就下结论。**

**已写入 `AGENTS.md` 的规则**：浮层类改动的验收必须做一次像素行扫描——取浮层中部一行，确认其中没有其他层画上来的元素。

---

## §8 证据文件清单

| 文件 | 内容 |
|---|---|
| `/tmp/L1.png` | **修复前**：模型卡片被卡片边框切穿 |
| `/tmp/L2-line-zoom.png` | **修复前**：线的特写 |
| `/tmp/accept-r64-model-card-fixed.png` | **修复后**：模型卡片完整 |
| `/tmp/accept-r64-model-card-fixed-crop.png` | **修复后**：卡片放大 |
| `/tmp/accept-r64-permission-fixed.png` | **修复后**：权限下拉未被切穿 |
| `/tmp/accept-r64-project-fixed.png` / `-crop.png` | **修复后**：项目菜单未被切穿 |

验收工具：`scripts/native-drive.swift`（原子化 激活-点击-截图）；像素扫描 `/tmp/vscan.swift`（单行/列颜色 run）、`/tmp/vfind.swift`（全图定位某颜色的横/纵 run）。后两者本轮临时编写，未入库。

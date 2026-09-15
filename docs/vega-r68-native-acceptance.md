# R68 实机验收报告（两个下拉弹窗：点外部关闭 + 宽度与内边距）

> 日期：2026-09-15
> 受测构建：`dist/Vega.app` → `/Applications/Vega.app`，`md5 a25d7a5cbbf0a64734da381ee941f0f2`
> 基线：`master @ 1c62455`
> 签名：`codesign --verify --deep --strict` 通过

---

## §1 结论

**用户提出的两条问题都已修复并实机验证通过。**

| # | 用户原话 | 修复前 | 修复后 |
|---|---|---|---|
| 1 | *"它弹出了之后悬浮在那里，我点其他地方它不消失。"* | 点任何地方都不关 | 点外部即关（三个位置实测） |
| 2 | *"它的边距大小，你这也太窄了吧？"* | 卡片 **183px**，项目名全被截断 | 卡片 **350px**，名字**完整显示** |

---

## §2 问题 1：点击外部关闭

### 修复

两个弹窗各加 `on_mouse_down_out`，走各自既有的关闭路径：

- 项目弹窗 → `utility_projects_open = false` + `cx.notify()`
- 分支弹窗 → `BranchSelector::request_close(cx)`（保留其 `BranchSelectorClosed` 事件与 controller 的 pending 清理）

### ⚠️ 必须同时修的陷阱：触发器会关不上

触发器的开关跑在 mouse-**up**，而 `on_mouse_down_out` 跑在 mouse-**down**。只加 out 处理器的话，点触发器是「down 先关 → up 再开」，**看起来完全没反应**。实测复现：

```
1. click chip        -> open=true
2. click chip again  -> open=true   ← 错，应该关掉
```

**修法**：触发器改用 `capture_any_mouse_down(stop_propagation)` 在 **capture 阶段**抢先声明这次手势，弹窗的 out 处理器就看不到它。原先的 `.on_mouse_down(...)` 是 **bubble** 阶段，比 capture 晚，**挡不住**。

**另一条走过的弯路（记录以免重犯）**：把 out 处理器挂到**包含触发器的外层 wrapper** 上也能修好触发器，但会**破坏弹窗内部点击**——弹窗是 `absolute` 定位，落在 wrapper 布局 bounds 之外，于是内部点击被当成「外部」。实测该方案内部点击会关掉弹窗。**不要用。**

### 实机证据

| 操作 | 结果 |
|---|---|
| 点主内容区空白（`1200,300`） | 弹窗**消失** ✔ |
| 点 composer 输入框 | 弹窗**消失** ✔ |
| 再点触发器 | 弹窗**消失**（陷阱已修）✔ |
| 点弹窗内部行 | 弹窗**保持** ✔ |
| 分支弹窗点外部 | 弹窗**消失** ✔ |

---

## §3 问题 2：宽度（"太窄"的真因）

### 我第一版规格搞错了基准，已更正

第一版从参考实现 CSS 取了 `[data-vega-window-type=browser]` 块的 `--menu-item-height: 36` / `--menu-item-padding: 6/10` / `--menu-gutter: 10`，据此写下"参考 20px vs Vega 12px，内边距差一倍，行高也该 32→36"。

**那组 token 是浏览器变体**，而 Codex 桌面截图才是产品真身。实测对比（方法已用 Vega 自身 32px 行高校准为 2×：64px 行距 ÷ 32 = 2.0）：

| | Codex 实测 | Vega 修复前 | 差 |
|---|---|---|---|
| **卡片宽度** | **261** | **183** | **-30%** |
| 卡片边→行填充 | 4.5 | 4 | ≈0 |
| 行填充→图标 | 9.5 | 8 | ≈1.5 |
| 卡片边→图标 | 14 | 12 | **≈2** |
| 行高 | 28.5 | 32 | Vega 反而更高 |

**结论（推翻第一版）**：

- **行内边距只差 2px，不是缺陷**；"20 vs 12"是拿浏览器变体当基准造成的假差距。
- **行高 Vega 已经比参考更高**，改 36 是反向优化 → **R7 撤销，行高保持 32**。
- **真正的缺陷是宽度**：183 vs 参考的**最小** 260。名字被截断就是这个的直接后果。

### 根因

`render_utility_projects_menu` 把弹窗挂在 **chip 的 `relative()` 容器**里并写 `.w(350).max_w_full()`。`max_w_full` 的 containing block 是那个 chip，而 chip 宽度由项目名决定 —— 弹窗被 chip **钳死**。生产测试实测 chip 只有 40px、弹窗被钳到 40px；实机约 183px。

### 修复

- **R13**：宽度改为 `project_menu_width(viewport)` = `min(350, 视口 - chip 左侧 inset - 8)`，不再受 chip 钳制，同时不溢出窗口。
- **R8**：共享行内水平内边距 8 → **10**（落在 `menu_list.rs`，两个弹窗同时生效）。
- **R9**：搜索行**去掉**独立灰底（参考实现就是放大镜 + 占位文字直接坐在卡片面上）。

### 实机证据

- 选中行实测宽 **346.5 逻辑 px**（修复前整个卡片才 183）。
- **所有项目名完整显示**：`r13-alpha-project`、`r11-open-workspace-check`、`r14-folder-two`、`vega-e2e-sandbox` —— 截图里**没有一个**省略号。
- 搜索行**无**灰底（项目与分支两个弹窗都确认）。
- 分支弹窗同样生效（共用 chrome）。

---

## §4 门禁与证伪

| 检查 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | 退出 0 |
| `clippy --workspace --all-targets -- -D warnings` | 退出 0 |
| `./scripts/cargo-lock.sh test -p vega_ui` | **315 passed / 0 failed** |
| `./scripts/cargo-lock.sh test --workspace` | **全绿**（30 个 suite，0 失败） |

### 证伪（我本人独立执行，全部确认）

| 破坏 | 失败测试 | 如预期 |
|---|---|---|
| 恢复 `.max_w_full()` 钳制 | `r68_a5`（宽度）+ `r68_a6` | ✔ |
| 触发器改回 bubble 阶段 | `r68_a4_...project_popup` | ✔ |
| 搜索行恢复 `bg_hover` | `r68_a7` | ✔ |
| `MENU_ROW_HEIGHT = 36` | `r68_a6` | ✔ |

四次证伪后均已还原（`git status` 干净，315 passed 复现）。

---

## §5 规格实现者反馈的三点（未粉饰）

1. **§2 的机制描述不完整**：规格说用 out 处理器**替换**弹窗原有的 `.on_mouse_down(stop_propagation())`。实现者**两个都保留**了——原有的 bubble 阶段声明负责「点弹窗内部不穿透到 composer 自己的外部点击处理器」，与 capture 阶段的 out 处理器**阶段不同、互不冲突**。替换反而会改变行为。**这个判断是对的**，规格该改。

2. **分支弹窗的触发器在测试里几乎不可达**：`menu_below` 把它放在 chip 下方，但 composer 在窗口底部，`anchored` 的 `snap_to_window_with_margin` 会把弹窗**拉回上方盖住 chip**，只剩 chip 顶部约 4px 露出来。测试因此计算「露出的那条带」并断言它存在。这是**既有的**布局性质，不是 R68 引入的。

3. **R14 的「视口收缩」需要一个新常量**：规格没写右侧以什么为界。实现者用 `CONTENT_PADDING + COMPOSER_UTILITY_BAR_INSET + COMPOSER_UTILITY_CHIP_INSET + 8` 作为 chip 左侧 inset。列居中时该值偏保守（不会乐观），列填满窗口时精确。**实机未在极窄窗口下测**——这是一条未覆盖的边界。

---

## §6 证据文件

| 文件 | 内容 |
|---|---|
| `/tmp/r68-b-open.png` | 项目弹窗打开：350px 宽，名字完整，搜索行无灰底 |
| `/tmp/r68-c-outside.png` | 点主内容区后：弹窗消失 |
| `/tmp/r68-e-input.png` | 点 composer 输入框后：弹窗消失 |
| `/tmp/r68-f2.png` | 再点触发器后：弹窗消失（陷阱已修） |
| `/tmp/r68-g-branch.png` | 分支弹窗：同样改进 |
| `/tmp/r68-h-branch-out.png` | 分支弹窗点外部后：消失 |

---

## §7 遗留

- **极窄窗口下的弹窗宽度**未实机验证（§5 第 3 点）。测试 `r68_r14_...` 覆盖了逻辑，但实机像素未测。
- `docs/vega-r66-slider-card-parity.md`（未跟踪，非我创建）仍在 `master` 工作区，去留待用户决定。

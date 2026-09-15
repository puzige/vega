# R67 实机验收报告（标题行高亮改 hover 态 + Off 档标签改 `Off`）

> 日期：2026-09-15
> 受测构建：`dist/Vega.app` → `/Applications/Vega.app`，`md5 75446baa500c8e57d78dbf4b71e13f4a`
> 基线：`master @ 38d3abb`
> 签名：`codesign --verify --deep --strict` 通过

---

## §1 结论

**用户提出的两条修正全部修复并实机验证通过。**

| # | 用户原话 | 修复前 | 修复后 |
|---|---|---|---|
| 1 | *"这里的阴影不是说鼠标不移上去它就展示，而是说鼠标移上去。"* | 灰底**常驻**（卡片一开就有） | 灰底**只在鼠标移到标题块上时**出现 |
| 2 | *"并且这个，null不能改成off吗？"* | `None` | **`Off`** |

两条都由**实机像素**与**生产测试**双重证明（见 §3、§4）。

---

## §2 修复内容

### §2.1 高亮改为 hover 态

`render_title_block`：`.bg(colors.bg_hover)` → `.hover(move |style| style.bg(colors.bg_hover))`。

**同时必须加 `.id("thinking-slider-title-block")`**，否则 hover **永不重绘**。这不是猜测，是本轮实测的框架行为：

GPUI 只在元素拥有 element state 时才注册 hover 翻转的重绘监听（`gpui-pre-0.3.4/src/elements/div.rs:2783`），而 element state 只在 `Element::id()` 返回 `Some` 时存在（`:1848`）。绘制分支（`:3391`）在有 hitbox 时读 `hitbox.is_hovered(window)`，无 hitbox 时退化到 element state——匿名 div 没有 element state。

临时探针实测（已删除）：

```
ANONYMOUS: base        renders=1 bg=BLUE
ANONYMOUS: hover-child renders=1 bg=BLUE   ← 永不变化
ANONYMOUS: hover-block renders=1 bg=BLUE   ← 永不变化
STATEFUL:  base        renders=1 bg=BLUE
STATEFUL:  hover-child renders=2 bg=RED    ← 生效
STATEFUL:  left        renders=3 bg=BLUE   ← 退出还原
```

**只加 `.hover()` 不加 `.id()` 会得到一个永远不亮的灰底，比原来的常驻灰底更糟。** 这是本条最危险的地方，已写进规格 §2 与代码注释。

加 `.id()` 不违反 R66 R5（容器不处理点击）：id 只注册一个 `HitboxBehavior::Normal` 的 hitbox，不注册任何监听器。`r66_r5_the_highlight_container_is_inert` 与 `r66_a7_...` 均**未改动且通过**。

### §2.2 Off 档标签改为 `Off`

`OFF_LABEL`：`"None"` → `"Off"`。

R66 当初选 `None` 的依据是参考实现 `composer.mode.local.reasoning.none.label` = `None`（本轮复核仍如此）。**但那个 `none` 是参考实现的一档 reasoning effort**，它把"关闭推理"表达为梯子最低那一档。Vega 的 Off 是独立的 `ReasoningChoice::Disabled`，经 `disabled_wire` 生效，**永不进入 `efforts`**。所以参考实现里没有与 Vega Off 对应的标签可抄——借用档位标签会把两个概念混为一谈，还让 Off 档与 `none` 档显示同一个词。

**`tier_display_label` 的映射表一行未动**（`none` → `None`、`low` → `Light`、`xhigh` → `Extra High` 全部保持）。这是本条最容易做错的地方：用户说的"`null` 改成 `off`"指的是 Off 档标签，不是映射表里 `"none" => "None"` 那一行。

---

## §3 实机像素证据

用 `scripts/native-drive.swift`（点击序列）与新增的 move-only 驱动（**hover 不能用点击验证**——点标题行会进二级列表）单进程完成「激活 → 移动 → 截图」。

判定：浅灰 `243`（= `bg_hover` `0xF3F3F3`）中性色块，逐行取穿过块中心的**连续**水平游程。

| 状态 | 灰底测量（2× 物理像素 → 逻辑） | 结论 |
|---|---|---|
| 卡片打开，指针在别处 | 标题区**无** 243 色块 | **非常驻** ✔ |
| 指针在**档位名行** | 一个连续块 `y 1402..1487`，**高 86px = 43 逻辑**，宽 198px = 99 逻辑 | 两行一起亮 ✔ |
| 指针在**模型名行** | **同一测量**（`y 1402..1487`，43 逻辑高，99 逻辑宽） | 同一块 ✔ |
| 指针移出块 | 该色块**消失** | 可逆 ✔ |

块高 **43 逻辑 px** 覆盖两行文字（档位名行 + 模型名行），且**不含**下方滑块轨道（轨道在 `y 2360+`，是另一个 `233,232,232` 色块）。宽 99 逻辑 px 与 R66 实测的贴内容宽度一致。

**关键**：无论指针在档位名行还是模型名行，量到的都是**同一个** `y 1402..1487` 连续块——这正是用户 R66 要求的"模型跟 thinking level 一起被选中"，而 R66 的实现把它做成了常驻。

### Off 档与档位名

| 项 | 实测 | 结论 |
|---|---|---|
| Off 档标签 | **`Off ›`** | ✔（不是 `None`，不是 `关闭`） |
| `high` 档 | **`High ›`**，紫色（最强档） | 首字母大写 + 色阶正确 ✔ |
| `low` 档 | **`Light ›`**，蓝色 | `low` → `Light` 映射未被误改 ✔ |

`Light` 这一条是 R11 的**回归证据**：若实现者把用户的"`null` 改 `off`"误读成改映射表，`low` 会显示 `Low` 或 `none` 会显示 `Off`，这里会立刻看出来。

---

## §4 门禁与证伪

| 检查 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | 退出 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 退出 0 |
| `./scripts/cargo-lock.sh test -p vega_ui` | **305 passed / 0 failed** |
| `./scripts/cargo-lock.sh test --workspace` | 全绿（见 §5 说明） |

### 证伪（我本人独立执行，四次全部确认）

| 破坏 | 失败测试 | 是否如预期 |
|---|---|---|
| 去掉 `.id()`，保留 `.hover()` | `r67_a4`、`r67_a5`、`r67_a6` | ✔（`r67_a3` 正确地**仍通过**——"永不点亮"确实满足"非常驻"，说明该测试只证明非常驻，不冒充 hover 生效） |
| 恢复常驻 `.bg()` | `r67_a3`、`r67_a6` | ✔ |
| `OFF_LABEL` 改成别的值 | `r67_a1`、`r66_a3` | ✔ |
| 把映射表 `"none"` 改成 `"Off"` | `r67_a2`、`r66_a1` | ✔ |

四次证伪后均已还原（`git status` 干净，`git diff master --name-only` 只有滑块文件与规格文档）。

### 生产测试能观测绘制了（纠正一条旧注释）

既有注释断言 *"The test platform has no headless renderer, so the grey fill ... [is] not observable here"*（`thinking_slider.rs:1808`、`:2762`）。**只对了一半**：

- `capture_screenshot` → `render_to_image()` 确实需要 `HeadlessRenderer`，未配置时 `bail!`（`gpui-pre-0.3.4/src/platform/test/window.rs:441`）；
- 但 **`Window::painted_quads()`**（`window.rs:2618`）直接读 `rendered_frame.scene.quads`，**不需要渲染器**。

本轮实测确认它在本仓库可用（真实卡片返回 14 个 quad，每个带 `bounds`/`corner_radii`/`background`），**因此 R67 的 hover 行为由生产测试 `r67_a3..a6` 断言**，不再只靠截图。注释已按事实改写。

> 注：`painted_quads` 的 bounds 是**缩放像素**（本机 2×），`debug_bounds` 是逻辑像素，测试里已按 `scale_factor` 换算。

---

## §5 门禁的一次波动（已排除，非 R67 引入）

`test --workspace` 首跑出现 1 个失败：

```
test tests::diff::diff_refresh_intents_keep_content_during_background_and_retry ... FAILED
```

**判定为已知的共享 git 状态 flake，非 R67 引入**，依据三条：

1. **隔离运行通过**：`cargo test -p vega tests::diff::diff_refresh_intents_...` → **1 passed**；
2. **重跑全绿**：紧接着的整轮 `test --workspace` 全部通过；
3. **R67 未触碰该 crate**：`git diff master --name-only` 只有 `crates/vega_ui/src/conversation_stream/thinking_slider.rs` 与规格文档，失败测试在 `crates/vega` 的 diff 模块。

这正是 `scripts/cargo-lock.sh` 头部记录的并发/共享状态 flake 类（失败测试每次不同、隔离即过），与 R64–R66 轮次观察到的是同一类。

---

## §6 证据文件

| 文件 | 内容 |
|---|---|
| `/tmp/r67-card2.png` | 卡片打开、指针在别处：标题区**无**灰底，显示 `Off` |
| `/tmp/r67-hover.png` | 指针在标题块：一个连续灰底覆盖两行 |
| `/tmp/r67-tierrow.png` | 指针在档位名行：同一块 |
| `/tmp/r67-modelrow.png` | 指针在模型名行：同一块 |
| `/tmp/r67-revert.png` | 指针移出：灰底消失 |
| `/tmp/r67-tier-crop.png` | `High ›` 紫色（最强档） |
| `/tmp/r67-low-crop.png` | `Light ›`（`low` 映射未误改的回归证据） |

---

## §7 遗留

`docs/vega-r66-slider-card-parity.md`（未跟踪，2026-09-15 09:50，**不是我创建的**）仍在原处。它主张"两行**各自**有独立灰底"与 `low`→`Low`、`xhigh`→`XHigh`，三处都与像素证据及参考实现原文冲突（R66 报告 §5 已详列）。**本轮未删除也未修改它**——它可能是你或另一会话写的。当前实现依据像素证据，采用"一个整块"与 `Light`/`Extra High`。请确认它的去留。

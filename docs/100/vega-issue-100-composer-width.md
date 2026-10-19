# Issue #100 · Composer 与正文列宽度一致 — 规格（冻结）

> 状态：**SPEC FROZEN**
> 基线：`master @ 79e5b4b`
> 来源：GitHub issue [#100](https://github.com/puzige/vega/issues/100)「composer 和 正文宽度不一致」+ 用户实机截图
> 决策：**B 方案（2026-09-21 用户裁决）** — 正文列与 Composer **都取 Codex 的 768px**，两者统一为**同一个宽度**。

---

## §1 现象与实测

用户截图 2808×1720 设备像素 = 1404×860 逻辑像素（Retina 2×）。在同一坐标系下实测：

| 元素 | 设备像素 | 逻辑像素 | 宽度 |
|---|---|---|---|
| 正文列（`conversation-column`） | x 954..2593 | x 477.0..1296.5 | **819.5 ≈ 820** |
| Composer 卡片（`composer-shell`） | x 1037..2510 | x 518.5..1255.0 | **736.5 ≈ 736** |

两者都 `mx_auto` 居中，所以 Composer 在**每侧**比正文列窄 **42px**。用户截图上的两条红色竖线（x≈445/1335 逻辑）与红色方框正是标注这个错位：正文列左缘 x≈477，Composer 左缘 x≈518.5。

---

## §2 根因

`crates/vega_theme/src/lib.rs` 把正文列与 Composer 的宽度冻结成**两个独立测量的常量**：

```rust
pub const CONTENT_MAX_WIDTH: f32 = 820.0;   // 正文列
pub const COMPOSER_MAX_WIDTH: f32 = 736.0;  // Composer
```

R21（`docs/vega-r21-screenshot-parity.md`）当初从参考截图里**分别量了两块区域**——「Conversation readable column ≈1640 device → 820」与「Composer outer width ≈1476 device → 736」——于是得到两个值。但参考实现的设计意图是**一个值**（见 §3），Vega 因此产生了一个结构性的宽度不一致。

---

## §3 参考实现证据（Codex）

### 3.1 源码：Composer 与正文列共用同一个 token

`vega-design-reference/app/webview/assets/app-initial-*.css`（electron 桌面端 `body`）只声明一个宽度变量：

```css
--thread-content-max-width: 48rem;   /* = 768px */
```

`app-primary-*.js` 里两处引用的是**同一个变量**：

| 元素 | 类名 |
|---|---|
| 正文列 | `mx-auto w-full max-w-(--thread-content-max-width)` |
| Composer 容器 | `relative mx-auto flex w-[min(100%,var(--thread-content-max-width))] min-w-0 justify-center px-panel` |

即 **Codex 中这两者天生等宽**，不存在「Composer 独立宽度」这个概念；宽度差只来自各自的内边距（正文 `px-toolbar`、Composer `px-panel`），**列宽本身是同一个**。`vega-design-reference/DESIGN_TOKENS.md` 亦记录「会话内容宽度：默认 `48rem`（桌面端 body 上）」。

### 3.2 实机截图佐证

参考截图 `Snipaste_2026-09-06_07-38-52.png`（1400×900）同一坐标系下：

| 元素 | 左缘（逻辑 px） |
|---|---|
| Composer 卡片边框 | ≈ **190** |
| 正文文字左缘 | ≈ **193**（3px 为字形 side bearing） |

两者落在**同一条左缘**上——这正是 issue #100 中 Vega 缺失的性质。

### 3.3 取值

`48rem = 48 × 16 = 768px`。

---

## §4 契约

**R1（必须）** `Layout::CONTENT_MAX_WIDTH` = **768.0**，`Layout::COMPOSER_MAX_WIDTH` = **768.0**；二者是同一宽度，来源为参考实现 `--thread-content-max-width: 48rem`。

**R2（必须）** 两个常量必须**恒等**。保留为两个独立字面量（各自仍被冻结测试独立钉住），并新增**编译期断言**：任一方被改动而另一方未同步时，构建必须失败。

**R3（必须）** 任意窗口宽度下，Composer 卡片与正文列**等宽**：两者共用同一个上限且共用同一层 `Layout::CONTENT_PADDING`，因此宽度恒为 `min(768, 可用列宽 − 2×CONTENT_PADDING)`。

**R4（必须）** 窄窗（可用列宽 < 768，例如 1403px 窗口且 Environment rail 展开时为 747px）下，两者**同时被列宽钳制且仍然等宽**。不得为达到 768 而破坏最小水平内边距 16px。

**R5（必须）** 只改宽度。`COMPOSER_RADIUS`(20)、`COMPOSER_MIN_HEIGHT`(100)、`COMPOSER_PADDING_TOP/BOTTOM`(12/16)、`COMPOSER_SEND_SIZE`(28)、R49 utility bar 全部几何（高 37 / inset 19 / radius 12 / chip gap 8 / chip inset 14.5）、R57/R61/R62/R64/R66/R67 的结论**不变**。

**R6（必须）** 不改其它宽度 token：`SETTINGS_CONTENT_MAX_WIDTH`(744)、`MENU_MAX_WIDTH`(350)、`ENVIRONMENT_RAIL_WIDTH`(320)、`ENVIRONMENT_CARD_WIDTH`(304)、`ENVIRONMENT_BREAKPOINT`(1230)。

**R7（必须）** 同步更新冻结测试与文档：`vega_theme` 的 `r21_phase_two_geometry_is_frozen`、`workspace.rs` 中断言 `width == COMPOSER_MAX_WIDTH` 的三处、`vega-design-guidelines.md`、`vega-ui-spec.md`、`vega-r21-screenshot-parity.md`。

---

## §5 非目标

- 不改 Composer 的圆角 / 高度 / 内边距 / 发送按钮尺寸。
- 不改正文列的最小内边距（16px）与居中方式。
- 不引入 `--thread-body-max-width` 那套 browser/electron 分支——Vega 只做原生桌面，单一 768。
- 不新增 `Layout` token，不删既有 token。

---

## §6 验收

| # | 证据 | 判据 |
|---|---|---|
| A1 | 生产测试 | 1403×860（rail 开）、rail 关、960×600 最小窗三种状态下，`composer-shell` 宽 == `conversation-column` 宽（R3/R4） |
| A2 | 冻结测试 | `CONTENT_MAX_WIDTH == 768.0`、`COMPOSER_MAX_WIDTH == 768.0`，且两者恒等（R1/R2） |
| A3 | 编译期 | 两 token 不等则构建失败（R2） |
| A4 | 门禁 | `python3 scripts/verify.py` 受影响包 fmt/clippy/test 全绿 |
| A5 | 原生截图 | 正文列左缘与 Composer 卡片左缘对齐（像素级） |

**既有测试影响**：`crates/vega/src/window/workspace.rs` 的三处 `assert_close(composer.size.width, COMPOSER_MAX_WIDTH, …)`（约 1893 / 3116 / 3134 / 3281 行）在 1403px 窗口下不再成立（768 > 747），必须改为「Composer 宽 == 正文列宽」的断言——这正是 issue #100 的回归判据。

---

## §7 已知未覆盖

- 原生像素验收需在真实 `~/Documents/Vega/Vega.app` 构建上完成；本轮先交付代码与自动化门禁，原生截图按仓库固定安装约定单独执行。
- 正文列与 Composer 的**内边距**差异（`px-toolbar` vs `px-panel` 在 Codex 中的区别）未对齐——Vega 两者都用 `CONTENT_PADDING`(16)，本就一致，不在本轮范围。

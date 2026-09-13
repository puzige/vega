# R50 · 选中填充派生 + 侧栏分区间距

> 状态：**SPEC FROZEN**（主 Agent 诊断 + 冻结，实现交 implementation subagent）
> 基线：`master @ 1c23fc4`
> 权威事实来源：`/Users/puzige/Workspace/vega-design-reference/`
>   - `app/webview/assets/app-initial-5b0a474bff5e.css`（token 层，816KB）
>   - `app/webview/assets/app-primary-7fe7c6486695.css`（组件层，145KB）
>   - `DESIGN_TOKENS.md`（202 行人工摘要）
>   - 实机截图（2x，逻辑 1403×859）：`sess_1320fcf3.../image-9f3849a8405ca811d6a357274c68a3b3.png`
>   - tab strip 特写（2x，逻辑 1092×291）：`sess_1320fcf3.../image-bec6b2d16f01b15dac9af7e7bd8bfcbb.png`
> 前置轮次：R47（`docs/vega-r47-panel-structure-alignment.md`）D5 与 §2.4 明确把这两项列为残差、另轮处理。本轮即该残差。

---

## §0 本轮修正了一个既有错误结论

R47 §2.4 与 delivery 记录写的是：

> `bg_active`(#ededed) 与 Codex tab fill(#f4f4f4) 差异 —— 全局选中态 token，影响面大，另轮处理。

该结论把 `#f4f4f4` 当作一个**独立的候选色值**，因而推导出"全局换值、影响 49 处调用点"的高风险结论。**本轮证伪**：

1. `#f4f4f4` 在参考实现中**不是设计 token**。全仓精确检索：`app-initial-*.css` 中仅出现 2 次，均为同一条 Tailwind 任意值工具类
   `.bg-\[\#F4F4F4\]{background-color:#f4f4f4}`；`app-primary-*.css` 中出现 **0** 次。其余命中全部是 mermaid 主题数据（`chunk-WYO6CB5R-*.js` 的 `cScale1`）与 SVG 渐变（`app-primary-6cd7b8b3f5e3.js` 的 `stopColor`），与 UI 表面无关。
2. 参考实现里选中填充是**一条语义规则**，而非一个字面色值：

   ```
   --color-background-primary-ghost-hover: var(--vscode-list-hoverBackground)
   --vscode-list-hoverBackground:          var(--color-background-button-secondary-hover)
   .electron-light{--color-background-button-secondary-hover: color-mix(in oklab, var(--color-text-foreground) 5%, transparent)}
   .electron-light{--color-text-foreground: #1a1c1f}
   ```

   即 **选中填充 = 5% 前景墨色叠加在当前表面之上**（`rgba(26,28,31,0.05)`）。
3. 该单条规则同时解释两次独立像素测量：

   | 表面 | 合成算式 | 计算值 | 实测值 | 出处 |
   |---|---|---|---|---|
   | 侧栏（`#f9f9f9`） | `0.05×26 + 0.95×249 = 237.85` | `#EEEEEE`→238 | **`#ededed`(237)** | 截图 x=50 列、选中行「优化 Launch Pad 应用排序」 |
   | Tab pill（`#fff`） | `0.05×26 + 0.95×255 = 243.55` | `#F4F4F4`(244) | **`#f4f4f4`(244)** | tab 特写 x=200 列，y 16..72px |

   两处表面不同、实测值不同，却由同一条 5% 规则精确解释——这不是巧合，是**规则被证实**。

**结论**：残留项不是"色值错了"，而是"Vega 把带 alpha 的派生规则压扁成了一个不透明常量 `bg_active = #EDEDEDFF`"。Vega 的侧栏选中态**本来就是对的**（237 = 237）；错的是在白色表面（tab pill、composer 卡片等）上复用了同一个不透明值，得到 237 而非应有的 244。

---

## §1 契约 A · 选中填充随表面派生

### A.1 新增 token（`crates/vega_theme/src/lib.rs`，`ThemeColors`）

```rust
/// 选中/激活填充：5% 前景墨色叠加在当前表面之上。
/// 参考实现语义：--color-background-primary-ghost-hover
///   = color-mix(in oklab, #1a1c1f 5%, transparent)
/// 保留 alpha，由 GPUI 在各自表面上合成，从而在任意表面得到正确结果。
pub bg_active_alpha: Rgba,
```

- 浅色：`rgba(0x1A1C1F1A)`（alpha = 0.05 × 255 ≈ 13 = 0x0D… **实现时取 `0.05*255=12.75`，按 GPUI `Rgba` 的 `a` 字段取 0.05**）
- 深色：参考实现深色走 `#ffffff08`（3% 白）。取 `rgba(0xFFFFFF08)` 等价语义；**深色不得回退到浅色值**。

> 实现注记：GPUI 的 `Rgba { r, g, b, a: f32 }`，`a` 用 `0.05` / `0.03` 直接表达，不要手工乘 255。

### A.2 `bg_active` 保持不变

`bg_active`（浅 `#EDEDEDFF` / 深 `#303030FF`）**不改值**。它是"在不透明表面上已经合成好的结果"，现有 49 处调用点中，绝大多数位于侧栏 / 不透明表面，**语义与取值都仍然正确**。改它会无差别影响全部选中态——这正是 R47 担心的风险，本轮通过新增 token 规避。

### A.3 唯一必须切换的调用点

| 位置 | 当前 | 改为 | 依据 |
|---|---|---|---|
| `crates/vega/src/window/workspace.rs` workspace tab pill 激活态 | `colors.bg_active` | `colors.bg_active_alpha` | 该 pill 位于 `#fff` 表面，实测参考实现为 `#f4f4f4`(244)，不透明 `#ededed`(237) 偏低 7 级 |

**仅此一处**。其余调用点不动。若实现中发现其他调用点位于明确白色表面上（`bg_base` / `bg_surface`），在交付文档中列出并说明，但**默认不动**。

### A.4 冻结测试

1. `bg_active_alpha` 浅色 alpha == 0.05、深色 == 0.03；浅色 RGB == `#1A1C1F`。
2. 合成不变量（纯函数，不依赖渲染）：在 `#fff` 上合成 `bg_active_alpha` 得到 `(244,244,244)`；在 `#f9f9f9` 上合成得到 `(238,238,238)`（±1 容差，覆盖 237/238 取整差）。**该测试是规则的可执行证明**，必须写。
3. `bg_active` 值不变（现有断言保留，不得修改既有期望值）。

---

## §2 契约 B · 侧栏垂直分区间距

### B.1 诊断（根因）

Vega 当前分区间距由**两个叠加的 margin** 产生，而非一个显式节距：

- `crates/vega_ui/src/sidebar/threads_block/organization/render.rs:209` — `body.gap_2()`（8px，作用于**所有** section 之间）
- `crates/vega_ui/src/sidebar/threads_block/organization/render.rs:320` — `render_pinned_pi` 内 `.mb_1()`（4px，**仅** Pinned 之后）
- 叠加 label 行 `h(28)` 内的行盒余量

结果：Pinned→Projects 边界比 Projects→Recents 边界多出 4px，节距**不一致**。这正是"看起来乱"的来源——不是某一边多了 12px，而是同一层级关系有两个不同的值。

### B.2 契约

1. **移除 `render_pinned_pi` 的 `.mb_1()`**（`render.rs:320`）。Pinned 不是特殊分区，不得有特殊间距。
2. **新增显式 token** `Layout::SIDEBAR_SECTION_GAP`，作为**唯一**的 section 节距来源，替代 `body.gap_2()` 对 section 的作用。
3. 节距值按参考实现实测锚定（见 B.3），并在 token 文档注释中写明它是"section 节距"，不是"通用 gap"。
4. 任何情况下**不得**用负 margin、`pt`/`pb` 补偿或坐标特判来凑数。

### B.3 参考实现实测（2x 截图 → 逻辑值）

侧栏文本行带（阈值 215，x 20..520）：

| 逻辑区间 | 高度 | 与前一带间距 | 判定 |
|---|---|---|---|
| 55.5..68.0 | 13.0 | — | 顶部品牌行 |
| 95.0..106.5 | 12.0 | 27.0 | 导航行 |
| 126.0..139.0 | 13.5 | 19.5 | 导航行 |
| 156.5..169.0 | 13.0 | 17.5 | 导航行 |
| 187.5..201.0 | 14.0 | 18.5 | 导航行 |
| 219.5..232.0 | 13.0 | 18.5 | 导航行（末） |
| **263.0..275.5** | 13.0 | **31.0** | **Projects label** |
| 294.0..305.0 | 11.5 | 18.5 | 项目行（首） |
| … | | ≈19 | 项目行 |
| 481.5..491.5 | 10.5 | 19.5 | Show more |
| **534.5..544.0** | 10.0 | **43.0** | **Recents label** |
| 565.5..575.5 | 10.5 | 21.5 | 会话行（首） |

**行内节距 ≈ 19px**（18.5 / 19.0 / 19.5 / 20.0 / 21.0 / 21.5，均值 ≈19.6）。
**section 边界 ≈ 31px**（导航末行 → label）/ **≈ 43px**（列表末行 → label）。

> 实现注记：31 与 43 的差异来自前一个 section 是否含可滚动列表。实现时取**一个** `SIDEBAR_SECTION_GAP`，值须由实现者用同一套像素测量法在 Vega 上回归确认落在 19–21 行距与 31/43 边界之间；若无法用单一值同时满足，**停下来按 `[BLOCKED]` 升级**，不要引入第二个特判 token。

### B.4 参考实现的行几何（供 B.2 对齐，非本轮必改）

`DESIGN_TOKENS.md` §7 + CSS 实证：`.sidebar-navigation{--height-token-nav-row:30px;--padding-row-x:8px;--radius-token-row:10px}`（桌面 `data-vega-window-type=electron`）。即导航行高 **30px**、水平内边距 **8px**、行圆角 **10px**。Vega 侧栏行高 token 为 `Typography::SIDEBAR_LINE_HEIGHT = 32.0`。

**本轮不改行高**（32 → 30 会牵动 R48 冻结的缩进阶梯与全部行内几何，属独立一轮）。仅记录事实，供后续轮次引用。

### B.5 冻结测试

1. `SIDEBAR_SECTION_GAP` 值冻结断言。
2. **结构不变量测试**：渲染 Pinned→Projects→Recents 三 section，断言任意相邻 section 的节距**相等**（消除 4px 差）。该测试是本次修复的核心可执行证明。
3. `render_pinned_pi` 不含额外 `mb_*` 的断言（可用 debug_selector 边界计算，或直接断言 Pinned 末行底 → Projects label 顶 == Projects 末行底 → Recents label 顶）。
4. 既有 R48 缩进阶梯测试（`SIDEBAR_ROW_INSET` / `SIDEBAR_LABEL_INSET`）必须保持全绿——**本轮不得改变水平缩进**。

---

## §3 明确不做

- 不改 `bg_active` 的值。
- 不改侧栏行高（32）、水平内边距、行圆角。
- 不改 `body.gap_1()`（`render.rs:198`，用于加载态）。
- 不动 tab pill 的其他几何（高度/内边距/圆角）——属 R51 tab 重构范围，见 `vega-r51-*`。
- 不改深色模式既有断言。

## §4 验收门禁

| # | 证据等级 | 要求 |
|---|---|---|
| 1 | UNIT-PROPERTY | 合成不变量测试（A.4.2）通过 |
| 2 | INTEGRATION-DELEGATING | 结构不变量测试（B.5.2）通过 |
| 3 | E2E-REAL | 全工作区 `cargo test` **串行单次**运行，0 失败（并行会触发 git fixture 竞争，见 R47/R48/R49 教训） |
| 4 | 原生截图 | tab pill 激活态实测填充 == `#f4f4f4`(244±1)；侧栏选中行仍为 `#ededed`(237±1)；三 section 节距一致 |
| 5 | 真实交互 | 点击 tab 切换、侧栏 Pinned 折叠/展开，行为与 R44/R47/R48 契约一致，无回退 |

## §5 提交约定

- 分支 `feat/r50-sidebar-section-rhythm`，worktree `/Users/puzige/Workspace/vega-r50-sidebar-rhythm`
- **不 push、不创建 MR**
- commit message 用 Conventional Commits，scope 用 `A6-02`（与 R47–R49 一致）
- 交付文档：`docs/vega-r50-selected-fill-and-section-rhythm-delivery.md`

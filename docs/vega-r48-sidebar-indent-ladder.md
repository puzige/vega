# R48 — Sidebar 缩进阶梯对齐 Codex

状态：**冻结** · 前置：R47（ed52b07）· 关联：`vega-r45-shell-controls-composer.md`、`vega-r46-window-anchored-shell-slots.md`、`vega-r47-panel-structure-alignment.md`、`vega-design-guidelines.md`

## §0 冻结依据

真值来源与 R47 同：Codex WebView token 源码 `/Users/puzige/Workspace/vega-design-reference/app/webview/assets/app-initial-*.css` + Codex 实机 AX 控件盒 + 原生 2x 像素测量。
证据：`/tmp/r48-vega-2x.png`（Vega 安装包原生 2x）、`/tmp/vega-r45-analysis/codex-base-2x.png`、对照图 `/tmp/r48-indent-definitive.png`。

## §1 诊断（已量化）

### D1 项目行的图标比 section 标签右偏 7.5px（核心缺陷）

Vega 当前水平基准链（`crates/vega_ui/src/sidebar/`）：

```
内容区左缘 base = SIDEBAR_PADDING(12) + scroll.ml(-4) + scroll.pl_2(8) = 16
section 标签       墨迹 = base                    = 16   (实测 17.0)
项目行 图标        墨迹 = base + row.px_2(8)       = 24   (实测 24.5)
项目行 文本        墨迹 = 图标 + 16 + row.gap_2(8) = 48   (实测 49.0)
子任务 文本        墨迹 = base + NAV_CONTENT_INSET(32) = 48 (实测 48.5)
Show More         墨迹 = base + NAV_CONTENT_INSET(32) = 48 (实测 48.5)
```

**folder 图标列 24.5 比 section 标签列 17.0 右偏 7.5px**，视觉上文件夹图标从层级列里"掉"出来，与标签之间留出一段空隙。

Codex 真值（同一坐标系，逐行核实）：

| 元素 | Vega | Codex |
|---|---|---|
| section 标签 | 17.0 | 17.5 |
| folder 图标 | **24.5** | **16.5** |
| folder 文本 | 49.0 | 41.0 |
| child 文本 | 48.5 | 40.5 |

**Codex 的 folder 图标(16.5) 与 section 标签(17.5) 同列**；folder 文本与 child 文本同列(41.0/40.5)。即 Codex 的层级模型是：**图标列 = 标签列（都贴 base），文本列 = 内容列**。

### D2 项目行多了一层 8px 左内边距

项目行自身 `.px_2()`(8) 把图标从 base(16) 推到 24，而 section 标签没有这层内边距。这 8px 是 D1 空隙的直接来源，也是"图标列与标签列不一致"的唯一成因。

### D3 文本列是巧合对齐，不是共享基准

folder 文本(48) 与 child 文本(48) 恰好同列，但成因不同：folder 靠 `px_2 + 16 + gap_2` 累加得到，child 靠 `NAV_CONTENT_INSET(32)` 得到。两者不是同一容器盒，任一改动都会立刻错位 —— 需改为共享同一阶梯 token。

## §2 契约

### 2.1 新增阶梯 token（`vega_theme::Layout`）

| token | 值 | 含义 |
|---|---|---|
| `SIDEBAR_LABEL_INSET` | **0.0** | section 标签相对 base 的额外内边距（标签贴 base） |
| `SIDEBAR_ROW_INSET` | **24.0** | 行**文本**列相对 base 的内边距；**导出值** = 图标 16 + `gap_2` 8 |

**`SIDEBAR_ROW_INSET` 是导出量，不是独立参数。** 项目行的文本列 = 图标(16) + gap(8) 自然得到 24；
子任务行没有图标，因此直接 `.pl(ROW_INSET)` 落到同一列。冻结测试必须断言
`SIDEBAR_ROW_INSET == 16.0 + 8.0`，否则改图标尺寸或 gap 会让两列重新错位。

推导（base = 16）：

```
标签列   = base + LABEL_INSET(0)        = 16   -> Codex 17.5  ✓
图标列   = base                         = 16   -> Codex 16.5  ✓  (folder 图标与标签同列)
文本列   = base + ROW_INSET(24)         = 40   -> Codex 41.0  ✓
child列  = base + ROW_INSET(24)         = 40   -> Codex 40.5  ✓
```

关键：**folder 图标与 section 标签同列（都贴 base）**，folder 文本与 child 文本同列（都走 ROW_INSET）。这是 Codex 的层级模型。

### 2.2 项目行（`organization/render.rs::render_pi_project`）

- 行盒 `.px_2()` → **`.pr_2()`**（去掉左内边距，只保留右侧 8）。行盒左缘本就在 base(16)，
  hover/selected 药丸边缘不动，只是内容起点从 24 回到 16。
- 图标(16px) 与文本之间的 `.gap_2()`(8) **保持不变**，于是文本自然落到
  16 + 16 + 8 = **40**，与 `SIDEBAR_ROW_INSET` 列一致。
- **验收锚点**：folder 图标左缘 == section 标签左缘（±1px）；folder 文本左缘 == child 文本左缘（±1px）

### 2.3 子任务行（`organization/projections.rs::render_project_thread`）

- `.pl(px(Layout::SIDEBAR_NAV_CONTENT_INSET))` → `.pl(px(Layout::SIDEBAR_ROW_INSET))`
- 保持 `.pr_3()` 与其余语义不变

### 2.4 Show More / progressive control（`render.rs::render_progressive_control`、`render_project_progressive_control`）

- `.pl(px(Layout::SIDEBAR_NAV_CONTENT_INSET))` → `.pl(px(Layout::SIDEBAR_ROW_INSET))`
- 使 Show More 与 child 文本同列

### 2.5 section 标签

- `render.rs` 中 Pinned / Projects 标签、`projections.rs` 中 Recents 标签统一 `.pl(px(Layout::SIDEBAR_LABEL_INSET))`
- 标签列保持 base(16)（现状已正确，改为显式 token 而非隐式继承容器）

### 2.6 不改动项

- `SIDEBAR_PADDING`(12)、`scroll.ml(-4)`、`scroll.pl_2()` 容器链 —— 保持（base=16 是标签列真值）
- 行高 `SIDEBAR_LINE_HEIGHT`(32)、字号 `Typography::SIDEBAR`(13) / `METADATA`(12)
- 项目行的 hover/selected/focus 视觉、actions 区域、拖拽与折叠语义
- `threads_block.rs` 对 `SIDEBAR_NAV_CONTENT_INSET` 的使用（R44/R22 会话行，不在本轮范围）

## §3 验收表（一个状态 · 一个生产测试 · 一张原生截图）

| # | 状态 | 生产测试 | 截图 |
|---|---|---|---|
| t1 | folder 图标列 == section 标签列（±1px） | 断言 `project-folder-<id>-open` 左缘 == `organization-section-label-Projects` 左缘 | 01 |
| t2 | folder 文本列 == child 文本列（±1px） | 断言 `organization-project-<id>` 左缘 == `project-thread-row-<id>` 左缘 | 02 |
| t3 | ROW_INSET 导出关系 | 断言 `SIDEBAR_ROW_INSET == 16.0 + 8.0`（图标 + gap） | 03 |
| t4 | 项目行 hover/selected 不变 | 既有 hover/selected 断言全绿（不改） | 04 |
| t5 | 折叠态图标不变列 | collapsed 与 expanded 的 folder 图标同列 | 05 |
| t6 | 门禁 | fmt / clippy -D warnings / test --workspace / xtask package + installed SHA 一致 | — |

## §4 残差

- `threads_block.rs` 会话行仍用 `SIDEBAR_NAV_CONTENT_INSET`(32)，与 organization 的 40 列不一致；本轮不动（R22/R44 契约），另轮评估。
- Codex 侧栏无 Pinned 分区标题（置顶项直接列在最上）；Vega 保留独立 Pinned 标签（产品差异，不追平）。
- 分区间距（垂直）31.5 vs Codex 19 的问题不在本轮（见 R48 对话记录，属垂直节奏，另轮）。
- `SIDEBAR_PROJECT_METADATA_WIDTH`(85) 未在本次测量覆盖范围内。

# R47 — 面板结构对齐 Codex（token 化 + Terminal 单层化）

状态：**冻结** · 前置：R45（d7cb2d6）、R46（95c5456）· 关联：`vega-r45-shell-controls-composer.md`、`vega-r46-window-anchored-shell-slots.md`、`vega-design-guidelines.md`

## §0 冻结依据（真值来源）

本轮起，面板几何/颜色的**权威真值**从"截图目测"升级为 Codex WebView 层源码 token（人类提供）：
`/Users/puzige/Workspace/vega-design-reference/app/webview/assets/app-initial-*.css`。
关键 token（light 主题实测解析）：

| Codex token | 值 | 本轮用途 |
|---|---|---|
| `--height-toolbar` | 46px | 主工具条（Vega `MAIN_HEADER_HEIGHT=46` 已一致） |
| `--height-toolbar-pane` | 40px | 面板工具条（Vega `WORKSPACE_HEADER_HEIGHT=40` 已一致） |
| `--padding-toolbar` | `calc(var(--spacing)*2)` = **8px** | 工具条尾随内边距 → 槽位带 trailing inset |
| `--color-background-surface` | `var(--gray-0)` = **#ffffff**（light）/ gray-900（dark） | **终端 body 背景 = 会话同款 surface** |
| `--color-vega-terminal-background` | `var(--vscode-terminal-background)` = `--color-background-surface` | 同上（终端不用独立灰底） |
| `--color-border` | `color-mix(foreground 8%)` ≈ #ebebeb | 面板分隔线（实测 #ededed = gray-100） |
| `--padding-row-x` | 8px | 终端文本左缘 ≈16px（8 padding + 8 网格） |
| `--right-panel-composer-overlay-reserve` | 118px | Codex 自己也有"右面板预留"token |

原生 2x 像素测量（Vega 安装包 vs Codex 实机，均 1403 逻辑宽）：
`/tmp/vega-r45-analysis/FINAL-term-compare-2x.png`、`codex-term-*.png`、`vega-term-*.png`。

## §1 诊断（全部已量化）

- **D1 右 pane 顶带 0px 归属间隔**：pane 动作组右缘 = 槽位带左缘（都落在窗口右缘 −108px）。Codex 实测 pane `+` 右缘 → summary 槽左缘 ≈ **32px** 空白。两层归属（面板内容 vs 窗口布局）在视觉上合并成一条 7 图标按钮带。
- **D2 同形不同义**：pane 本地 `Icon::DockBottom`（"移到底部"）与 shell 槽位 `Icon::DockBottom`（"切换终端 ⌘J"）同形，仅隔 1 个按钮；逐像素比对为同一字形。
- **D3 Terminal 面板三层 vs 单层**：
  - Vega：顶边框(585,#e8e8e8) → 42px tab 行（**有下边框** 627）→ 32px 状态行 `zsh · 登录 shell`（**有下边框** 659）→ 灰底 body `#f6f6f6`。三条全宽分隔线。
  - Codex：顶边框(579,#ededed) → ~40px tab 行（**无下边框**）→ 白底 body `#ffffff`。一条分隔线。
- **D4 终端背景用错 token**：Vega 把 `code_bg`（#f6f6f6，为内联代码块设计的 inset 面）当作终端 body 背景；Codex 终端 = `--color-background-surface`（与会话区同面）。结果：Vega 终端读作"灰色凹陷区"，Codex 读作"白色工作面"。
- **D5 tab pill 几何**：Vega 34px 高 / #ededed；Codex 28px 高 / #f4f4f4。
- **D6 尾随内边距**：Vega 12px（`pr_3`）；Codex 8px（`--padding-toolbar`，AX 实证：窗口右缘 1431 − 槽位右缘 1423 = 8）。
- 字体不是问题：advance 7.94 vs 8.12 px/char、cap height 均 9.5px。

## §2 契约

### 2.1 Layout token（`vega_theme::Layout`）

| token | 值 | 说明 |
|---|---|---|
| `TOOLBAR_TRAILING_INSET` | **8.0** | 取代槽位簇的 `pr_3()`(12)；依据 Codex `--padding-toolbar` + AX 实测 |
| `SHELL_SLOT_CLUSTER_RESERVE` | **104.0** | = 3×28 + 2×6 + 8（原 108；随 inset 修正） |
| `SHELL_SLOT_GUTTER` | **32.0** | pane 动作组与槽位带之间的归属间隔（实测 Codex ≈32） |
| 顶带 pane header 尾随预留 | `RESERVE + GUTTER` = **136** | 右 pane / 最大化 bottom pane 的顶带头部 |
| 主头部（main-header）尾随预留 | `RESERVE` = **104** | 主头部右侧无本地动作，无需 gutter |

槽位中心（1403 逻辑宽窗口）六态恒定：**窗口右缘 −22 / −56 / −90**（= 1381 / 1347 / 1313；较 R46 右移 4.5px，Codex 真值背书：inset 12→8 使槽位带右移）。

### 2.2 右 pane 顶带（含最大化 bottom pane 顶带）

- pane header `pr = RESERVE + GUTTER`；实测验收：pane 最后一个动作右缘与槽位带左缘间距 = 32±2px。
- **图标去重**：pane 本地 dock 动作（移到底部/移到右侧）改用新图标 `Icon::DockMove`（面板矩形 + 指入箭头，inline SVG，白名单内自绘），tooltip 文案不变；shell 槽位继续用 `DockBottom`/`DockRight`。同一个动作仍然只有一个入口（R45 §3 不变）。

### 2.3 Terminal 面板（不回退 R44）

1. **body 背景** = `colors.bg_base`（surface，#ffffff），不再用 `code_bg`。ANSI 前景/选区色不变。
2. **状态行只在异常态渲染**：`status != Running`（Starting/Exited/Failed）时才出现 `terminal-toolbar`（含 复制屏幕 / 重启终端 两个按钮）；`Running` 态整行不渲染。理由：Codex 无状态行；Running 态的 copy 功能由既有 `⌘C`→`copy_screen` action 承接，重启诉求由 `+`（新建）与 tab `×`（关闭）覆盖。这是**可见性谓词**（同 R44 rendered-visibility 思路），不是功能删除。
3. **单分隔线**：移除 workspace pane header 的 `border_b_1`（所有 dock 共用）。依据：Codex 底部面板 header 无下边框；右 pane 内容首行（`r12-linked / 0 files` 工具行）自带下边框，分隔职责归内容行。Terminal 面板最终 = 仅顶边框 1 条。
4. **tab pill 高度 34 → 28**（icon 16 + 上下 6），fill 仍用 `bg_active`（残差见 §4）。
5. **文本左缘 ≈16px**：terminal canvas frame 水平 padding 12 → 8（实测 Codex 16.5）。
6. R44 全部语义不变：chevron 隐藏 / `+` 仅创建 / 全局 reveal 保持 Composer 焦点 / 显式激活聚焦 PTY / 关闭后相邻同pane选择 / rendered-visibility 谓词 / 不可用右迁移保 tab+PTY。

### 2.4 明确不做（残差驱动）

- pane header 控件数（chevron/+ /dock/maximize = 4）多于 Codex（+ / ×）——R44 契约冻结，不删功能。
- `bg_active` 全局值（#ededed）与 Codex tab fill（#f4f4f4）的差异——影响所有选中态，不在本轮动。
- 终端字体度量（advance 7.94 vs 8.12）——不同 mono 字体域，不改。

## §3 验收表（一个状态 · 一个生产测试 · 一张原生截图）

| # | 状态 | 生产测试（production 入口） | 截图 |
|---|---|---|---|
| t1 | 槽位六态恒定 | `shell_bounds` 断言 centers = 1306.5/1340.5/1374.5（inset 8） | 01–06 |
| t2 | 右 pane 顶带 gutter | `mounted_bounds`：pane 末动作右缘 → 槽位带左缘 = 32±2 | 07 |
| t3 | 图标去重 | pane dock 按钮 selector/label 不变 + 与槽位2字形宽度差断言 | 08 |
| t4 | Terminal Running | 无 `terminal-toolbar` 元素 + canvas frame bg = bg_base | 09 |
| t5 | Terminal Exited | `terminal-toolbar` 出现且含 copy/restart | 10 |
| t6 | R44 回归组 | 既有 r44 测试全绿（不改断言） | — |
| t7 | 门禁 | fmt / clippy -D warnings / test --workspace / xtask package + installed SHA 一致 | — |

## §4 残差

- `bg_active` #ededed vs Codex tab fill #f4f4f4（全局 token，影响面大，另轮处理）。
- pane header 控件数 4 vs Codex 2（R44 冻结，保留）。
- 终端字体域不同（Vega mono 12.5px）。
- Codex 侧面板是 browser 面板、Vega 是 Review/diff 面板——功能域不同，仅对齐结构模型与 token。

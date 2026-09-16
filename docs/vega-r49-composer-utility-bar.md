# R49 — Composer 上下文栏（utility bar）与两个状态的像素级对齐

状态：**冻结** · 前置：R45（d7cb2d6）、R46（95c5456）、R47（ed52b07）、R48（08602fb）· 关联：`vega-design-guidelines.md` §9 Composer

2026-09-16 局部取代：chip 的高度、水平 padding、胶囊形状、hover/open 底色和两 chip 间距以 [utility chip states R1–R3](vega-utility-chip-states.md) 为准（28px / 8px / 全胶囊 / 静止透明 / 间距 8px）。本文件其他几何与真实数据契约保持不变。以下旧值作为历史冻结记录保留，不用于本次触发器实现。

## §0 冻结依据与人类裁决

真值来源：Codex 实机原生 2x 抓帧（`/tmp/r49-codex-newchat-2x.png` 新建任务页、`/tmp/r49-codex-work-2x.png` 会话页）+ Codex WebView token 源码（`vega-design-reference/app/webview/assets/app-primary-*.css`）。

**人类裁决（本轮）**：
1. utility bar **只做两格**：文件夹 + 分支。Codex 的 `Local`（执行环境）**不移植**（不占位、不置灰）。
2. **移除** Environment 卡片中的分支行（`environment-branch`），分支入口唯一化到 composer —— 符合 R45「同一动作只有一个入口」。

## §1 诊断（已量化）

### 两个 composer 状态

| 状态 | 出现时机 | Codex 特征 |
|---|---|---|
| **新建任务页**（utility-bar variant） | 未发出第一条消息 | 卡片上方多一条 utility bar（文件夹 / 环境 / 分支） |
| **会话页** | 已发出第一条消息 | 仅卡片本体 |

Vega 现状：**两个状态是同一个 composer**，既无 utility bar，也无状态区分。

### 会话 composer 卡片：Vega 数值上已基本对齐

| 项 | Codex 实测 | Vega token | 结论 |
|---|---|---|---|
| 卡片宽 | 736.5 | `COMPOSER_MAX_WIDTH = 736.0` | **已对齐** |
| 卡片高（空态） | 99.0 | `COMPOSER_MIN_HEIGHT = 100.0` | **已对齐**（差 1px） |
| 发送按钮 | 28.0 | `COMPOSER_SEND_SIZE = 28.0` | **已对齐** |
| 卡片边框 | `#ededed` | `border_subtle` | 已对齐 |
| 卡片居中 | 内容区内偏移 0.5px | `mx_auto()` | **已对齐** |

结论：**会话 composer 不需要重做几何**，本轮只做校验性冻结测试。

### utility bar 权威几何（Codex 新建任务页实测）

```
bar  : x 510.0 .. 1219.5   width = 710.0   （稳定区，圆角内收）
card : x 497.0 .. 1232.5   width = 735.5
bar 左右各比 card 内缩 19.0
bar  top    = 707.0
bar  bottom = 744.0   <- 与 card top 重合，bar 底被卡片覆盖（层叠）
card top    = 744.0
card bottom = 843.0
bar  height = 37.0
bar  fill  = #f5f5f5
```

bar 内三个 chip（Codex 含 Local；Vega 取前两格）：

```
chip1（文件夹）: x 524.5 .. 573.5   w=49.5   left inset from bar = 14.5
chip2（环境）  : x 601.5 .. 653.5   w=52.5   gap = 28.0
chip3（分支）  : x 683.0 .. 744.0   w=61.5   gap = 29.5
chip 图标 16px + 文字，无边框、无背景
```

Codex token 佐证：`ComposerHomeUtilityBar` 用 `background-color: var(--color-background-composer-action-bar)` + `padding-inline: var(--spacing)`（4px）。实测 bar fill `#f5f5f5` 即该 action-bar 面。

### 缺口

1. 无 utility bar 层（含其层叠：bar 底被卡片覆盖 0px 重叠，bar 比卡片窄 19px×2）。
2. 无「新建任务页 vs 会话页」的状态区分 —— Vega 现在任何状态都渲染同一种 composer。
3. 文件夹 chip 无 composer 内入口（项目切换目前只在 sidebar）。
4. 分支 chip 需从 Environment 卡片迁移（人类裁决 2）。

## §2 契约

### 2.1 新增 Layout token（`vega_theme::Layout`）

| token | 值 | 依据 |
|---|---|---|
| `COMPOSER_UTILITY_BAR_HEIGHT` | **37.0** | Codex 实测 bar height |
| `COMPOSER_UTILITY_BAR_INSET` | **19.0** | bar 相对卡片左右各内缩 |
| `COMPOSER_UTILITY_BAR_RADIUS` | **12.0** | bar 顶部圆角（实测圆角过渡约 8..12，取 12 与卡片 20 形成层级） |
| `COMPOSER_UTILITY_CHIP_GAP` | **28.0** | chip 间距（实测 28.0 / 29.5，取 28） |
| `COMPOSER_UTILITY_CHIP_INSET` | **14.5** | 首个 chip 距 bar 左缘 |

`COMPOSER_MAX_WIDTH`(736)、`COMPOSER_MIN_HEIGHT`(100)、`COMPOSER_RADIUS`(20)、`COMPOSER_SEND_SIZE`(28) **不变**（已对齐 Codex）。

### 2.2 utility bar 结构（`vega_ui/src/conversation_stream/render.rs`）

- 在 `composer-shell` 外层新增容器（debug_selector `composer-utility-bar`），置于卡片**上方**、与之**零重叠**（bar bottom == card top）。
- 宽度 = 卡片宽 − `2 × COMPOSER_UTILITY_BAR_INSET`，水平居中（`mx_auto`）。
- 高度 `COMPOSER_UTILITY_BAR_HEIGHT`；填充 `bg_sidebar`（对齐 Codex `#f5f5f5` 面）；顶部圆角 `COMPOSER_UTILITY_BAR_RADIUS`，**底部无圆角**（被卡片接续）。
- bar 与卡片**视觉上连续**：bar 的左右内缩 + 卡片完整宽度，形成「标签页压在卡片上」的层叠观感。不使用负 margin；靠父容器 `flex_col` 顺序布局实现。
- bar 内从左到右两个 chip，`.gap(px(COMPOSER_UTILITY_CHIP_GAP))`，首个 chip 左内边距 `COMPOSER_UTILITY_CHIP_INSET`。

### 2.3 chip 样式（两格共用）

- 结构：`icon(16px, text_secondary)` + `label(Typography::SIDEBAR, text_primary)`，`.gap_2()`。
- **无边框、无背景、无圆角药丸**（与 Codex 一致；区别于现有 `branch-selector-trigger` 的带框药丸）。
- hover：`bg_hover` + `rounded_md`（Codex chip 有 hover 反馈）；cursor pointer。
- 各自带 tooltip：文件夹 chip `切换项目`、分支 chip `切换分支`。

### 2.4 状态区分（新建任务页 vs 会话页）

- **utility bar 仅在「新建任务页」渲染**：谓词 = 当前路由有项目上下文（`shell_project_id` 可解析）**且**当前会话无任何消息（`ConversationStream.entries.is_empty()`）。
- 会话页（已有消息）不渲染 utility bar —— 与 Codex 行为一致。
- 谓词必须是**渲染可见性谓词**（真实是否渲染），不得用 `hidden()` 或零高度占位。

### 2.5 文件夹 chip 行为

- 点击打开项目选择器（下拉），列出 sidebar 已有项目；选中后写 `SelectedProject` 全局并 `refresh_windows`。
- 复用既有项目数据源（`sidebar.project_label` / 项目列表），**不新建数据管道**。
- 无项目上下文时 chip 不渲染（整条 bar 不渲染，见 2.4）。

### 2.6 分支 chip 行为

- **迁移**：把现有 `BranchSelector` 实例的渲染入口从 Environment 卡片（`workspace.rs::render_environment` 的 `environment-branch` 节点）移到 composer utility bar。
- **移除** Environment 卡片的分支行（人类裁决 2）：`environment-branch` 节点删除，其分支状态/动作不再在 Environment 暴露。
- `BranchSelector` 本体（`vega_ui/src/branch_selector.rs`）**不改语义**：open/close/切换/pending/错误码/focus/滚动全部保持；`set_menu_below(true)` 保持（composer trigger 弹层向上/向下由既有逻辑决定）。
- **样式例外**：`BranchSelector` 的 trigger 当前是带边框药丸（`.border_1().rounded_md()`），被 Environment 卡片与 composer 共用。迁移后 trigger 需按 §2.3 的无框样式渲染。**做法**：给 `BranchSelector` 增加一个 `set_chrome(ComposerChip)` 之类的显式开关（或等价的最小改动），使 trigger 样式由挂载点决定，而不是硬改默认样式。若实现者认为需要更简单的方案，可 [BLOCKED] 上报。
- 分支标签仍来自 `current_head`（真实 git 状态），非 git 项目按既有 `NonGit` 语义隐藏。

### 2.7 明确不做

- **不移植** Codex 的 `Local`（执行环境）chip —— 不占位、不置灰（人类裁决 1）。
- 不改会话 composer 的几何（已对齐）。
- 不改 `vega_conversation` 的任何类型。
- 不新增依赖。

## §3 验收表（一个状态 · 一个生产测试 · 一张原生截图）

| # | 状态 | 生产测试 | 截图 |
|---|---|---|---|
| t1 | 新建任务页渲染 utility bar | 断言 `composer-utility-bar` 存在且高度 == 37 | 01 |
| t2 | 会话页不渲染 utility bar | 发一条消息后断言 `composer-utility-bar` absent | 02 |
| t3 | bar 宽 = 卡片宽 − 2×19，水平居中 | 断言 bar 左右内缩各 == 19±1，中心对齐卡片中心 | 03 |
| t4 | 两 chip 顺序与间距 | 断言文件夹 chip 左缘 == bar 左缘 + 14.5±1；chip 间距 == 28±1 | 04 |
| t5 | 文件夹 chip 切换项目 | 打开下拉选中另一项目 → `SelectedProject` 更新 + 路由刷新 | 05 |
| t6 | 分支 chip 迁移后可用 | 点击打开分支列表、切换生效（复用既有 branch 测试路径） | 06 |
| t7 | Environment 卡片无分支行 | 断言 `environment-branch` absent；Environment 其余行不变 | 07 |
| t8 | 会话 composer 几何不回退 | 断言卡片宽 736 / 高 ≥100 / 发送 28（既有断言，不改） | 08 |
| t9 | 门禁 | fmt / clippy -D warnings / test --workspace / xtask package + installed SHA 一致 | — |

## §4 残差（预期）

- `Local` chip 缺失导致 Vega 的 bar 只有两格，与 Codex 三格不同构 —— 人类裁决 1 已接受。
- Codex bar 的 `padding-inline: 4px` 与实测 chip inset 14.5 的差异：本规格取实测值，token 来源以实机为准。
- 项目选择器若在 composer 内复用 sidebar 的下拉组件，其弹层定位可能需按 composer 上下文调整 —— 实现时若遇到，按最小改动处理并记录。

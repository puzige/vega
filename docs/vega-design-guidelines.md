# Vega 设计守则

**版本** v1.1 · 2026-09-10

**状态** 当前视觉语言与设计 token 的规范入口

**适用范围** Vega macOS 原生客户端（Rust + GPUI）

本文件是从 Vega 自有代码、规格、品牌资产与用户可见界面证据中独立整理的原生设计守则。用户提供的外部参考只触发了“建立设计守则”这一需求；其中来自第三方应用解包的 token、文件结构与实现细节没有作为本文输入，也不得进入 Vega 的实现。

## 1. 权威边界

### 1.1 可采用与禁止采用的来源

- 可采用：Vega 仓库内自主编写的规格、代码、测试与品牌资产，以及经用户确认的产品决策。
- 可采用：第三方产品正常运行时用户可见的截图、交互与人工测量，以及平台方公开文档；引用时只记录可观察结果。
- 禁止采用：从第三方安装包、归档、bundle 或可执行文件中解包、反编译或提取的 CSS、JavaScript、内部 token 名、组件树、私有图标和其他资产。
- 禁止采用：仅因外部实现中存在就把某个数值、命名或 API 搬入 Vega。相似决策必须能由 Vega 自己的产品目标、规格和验证证据独立解释。

历史逆向研究文档只作为审计记录保留，不是设计输入，也不能作为实现或验收结论的引用来源。

### 1.2 规范优先级

Vega 的设计与实现按以下顺序判定：

1. 产品行为、安全、数据真实性与平台约束，以 PRD、技术规格和执行宪法为准。
2. 已冻结任务规格对其范围内的精确几何和行为拥有优先级，例如 [R18 品牌 UI](vega-r18-brand-ui.md) 与 [R19 主窗口壳层](vega-r19-codex-parity.md)。
3. 本文件统一跨任务的视觉语言、token 使用方式和新界面的默认决策。
4. [`vega_theme`](../crates/vega_theme/src/lib.rs) 中的 `ThemeColors`、`Typography` 与 `Layout` 是“已经实现”的代码真值。文档新增值在进入类型化 token 并通过测试前，不得宣称已落地。

[UI 验收规格](vega-ui-spec.md)继续承载可测行为与组件验收；若其中的历史数值与本文件或后续冻结规格冲突，使用更新、更具体的规范，并在实施任务中同步修正文档。

## 2. 设计原则

### 2.1 安静的原生工作台

- 工作区以中性表面、清晰层级和充足留白为主，不用大面积品牌色、饱和渐变或装饰性玻璃效果争夺内容注意力。
- 使用 macOS 原生窗口、标题栏、字体和交互预期；不把 Web 布局模型或兼容层搬进 GPUI。
- 边界优先依靠表面层级、1px 细分隔线和空间关系表达，避免重边框与层层卡片。

### 2.2 品牌来自形态，不来自铺色

Vega Logo 的连续终端折角、单颗细长星与轻微笑意，是产品的几何基调。功能图标仍应保持熟悉、直接的语义；星形只用于 Agent、AI 或 Thinking 等品牌相关动作。品牌蓝只承担主操作、选中态与品牌重点。

### 2.3 状态必须真实

界面只展示 Vega 确实拥有的数据、权限和操作。加载、成功、失败、禁用、选中与危险状态必须来自真实 controller/state，不用静态数字、虚构 branch、伪进度或装饰性状态冒充功能。

### 2.4 四像素节奏

布局以 4px 为基本节奏，常用间距为 `4 / 8 / 12 / 16 / 20 / 24 / 32`。允许 1px 分隔线、5px 拖拽分隔器、12.5px 代码字号等有明确渲染或交互理由的例外；不要随意新增 3px、7px、11px 等一次性数值。

### 2.5 语义先于色板

组件表达“背景、边框、正文、危险、品牌”等语义，不直接选择某档灰色或某个十六进制值。浅色与深色模式必须由同一语义映射切换，而不是在组件中分支维护两份样式。

## 3. Token 架构

```text
设计意图
  ├─ 颜色语义 ──> ThemeColors ──> theme(cx).colors.<token>
  ├─ 字体层级 ──> Typography  ──> Typography::<TOKEN>
  └─ 几何尺度 ──> Layout      ──> Layout::<TOKEN>
                                      │
                                      └─ GPUI 组件
```

规则：

- 组件禁止新增十六进制颜色、appearance-specific 颜色或重复的布局常量。
- 新 token 先在本文件说明意图，再进入 `vega_theme` 的类型化 API 与测试，最后由组件消费。
- 组件通过 `theme(cx)` 读取当前主题；不得引入 CSS 变量、Tailwind 名称、`--vscode-*` 兼容变量或 Web `data-*` 状态协议。
- ANSI 终端颜色是外部内容协议，不属于产品 chrome；它们继续封装在 `vega_theme::terminal_indexed_color` 中。
- 旧 sidebar group 色数组仅用于兼容历史数据，不是新产品界面的推荐色板。

## 4. 颜色

### 4.1 当前语义颜色

| Token | Light | Dark | 用途 |
|---|---:|---:|---|
| `bg_base` | `#FFFFFF` | `#202020` | 主内容背景 |
| `bg_sidebar` | `#F3F3F3` | `#191919` | 侧栏背景 |
| `bg_elevated` | `#FFFFFF` | `#2A2A2A` | 卡片、Composer、浮起表面 |
| `bg_hover` | `#ECECEC` | `#323232` | 中性 hover |
| `bg_active` | `#EAF2FC` | `#203247` | 当前项目、任务与选中控件 |
| `border_subtle` | `#E8E8E8` | `#383838` | 1px 分隔线与细边框 |
| `text_primary` | `#202020` | `#EDEDED` | 正文与主要标签 |
| `text_secondary` | `#676767` | `#ABABAB` | 辅助信息、时间与次级标签 |
| `text_tertiary` | `#8A8A8A` | `#828282` | 占位符与弱提示 |
| `accent` / `brand_primary` | `#3478D8` | `#8FC7FF` | 主操作、选中图标、品牌重点 |
| `brand_primary_strong` | `#245AAF` | `#609DE1` | 高对比品牌强调与 hover |
| `brand_soft` | `#EAF2FC` | `#203247` | 低对比品牌洗色 |
| `brand_on_accent` | `#FFFFFF` | `#13233A` | 品牌主色表面上的内容 |
| `success` | `#1A7F37` | `#3FB950` | 成功、diff 新增 |
| `danger` | `#CF222E` | `#F85149` | 错误、危险操作、diff 删除 |
| `warning` | `#9A6700` | `#D29922` | 权限确认、预算告警 |
| `code_bg` | `#F6F6F6` | `#262626` | 代码块与终端外的等宽表面 |

### 4.2 使用规则

- 普通 hover 使用 `bg_hover`；选中态使用 `brand_soft`/`bg_active`，文字或图标使用 `brand_primary`。
- 成功、危险和警告保留各自语义，不能为了“统一品牌”全部改成蓝色。
- 文字层级优先用 `text_primary / secondary / tertiary`；不要靠随意透明度制造第四、第五层灰色。
- 新增语义时先证明它不能由现有 token 表达。不要把完整原始灰阶暴露给组件。
- 浅色和深色都要单独验收。深色不是浅色值机械反相，也不依赖浅色阴影维持边界。

## 5. 排版

Vega 默认使用平台系统无衬线字体；代码和终端使用平台等宽字体。CJK 必须具备可读 fallback，不得出现豆腐块或中英文基线明显断裂。

| 角色 | 当前值 | 规则 |
|---|---:|---|
| 元数据 | 12px | 时间、状态、说明；不得承载主要操作 |
| 正文 / 控件 / 侧栏 | 13px | 正文行高 1.55；侧栏行高 32px |
| 代码 | 12.5px | 等宽；保留空白与对齐 |
| 区块标题 | 14px / 600 | 一个页面内的主要分组 |
| 会话消息 | 15px | 行高 1.65，优先阅读舒适度 |
| 页面标题 | 16px / 600 | 页面或路由一级标题 |
| Settings 标题 | 24px | 现有 Settings 层级 |
| 空态标题 | 28px / 600 | 一个视图最多一个；不扩散到普通卡片 |

标题只使用“页面 / 区块 / 卡片”三级层级；卡片标题为 13px / 500。正文不靠大字号制造层级，优先使用间距、字重与语义颜色。

## 6. 几何与布局

### 6.1 当前共享几何

| 区域 | Token / 目标值 | 说明 |
|---|---:|---|
| 原生标题栏前导留白 | 96px | 为 macOS traffic lights 保留 |
| Sidebar | 260px | 内边距 12px；行高 32px |
| 主内容外间隙 | 4px | Sidebar、主面板与窗口边缘之间 |
| 主 Header | 46px | 底部 1px 分隔线 |
| 可读内容列 | max 820px | 居中；最小水平内边距 16px |
| Composer | max 736px / min-height 100px | 圆角 20px，内容可因多行或错误增长 |
| 普通 Panel / Card | radius 12px | 默认容器圆角 |
| Environment rail | 292px | ≥1180px 时可持久显示 |
| Environment card | inset 16px / radius 18px | 轻边框，必要时使用克制小阴影 |
| Workspace header | 40px | 右侧或底部工作区共享 |
| 底部 Workspace | default 272px | 保留现有最小值和拖拽行为 |
| 右侧 Workspace | 43% / min 270px | 用户 resize 结果持久化 |

主窗口设计与截图验收使用 1400×900；应用最小窗口为 960×600。响应式验收必须覆盖 1179px 与 1180px 两侧，确保 Environment 的 overlay/rail 切换不会丢失用户折叠状态或真实操作。

### 6.2 圆角与表面

- 普通卡片默认 12px；Composer 20px；Environment 18px。小型按钮使用共享组件既有尺度，不为单个页面新造圆角档位。
- GPUI 当前使用普通原生圆角，不模拟 CSS `corner-shape` / superellipse。未来若增加 squircle，必须形成共享 renderer 和视觉回归，不做组件级渐进增强。
- 主内容与 Sidebar 依靠中性色差和细分隔表达层级。浮层、菜单、临时 Environment 卡片可以使用轻阴影；常驻卡片默认不叠加阴影阶梯。
- 不使用 blur/半透明玻璃作为可读性的必要条件。任何材质效果都必须在无 blur 时仍有清晰边界。

## 7. 图标与品牌形态

- 所有共享功能图标使用固定 16×16 容器，通过 `vega_ui::icons` 映射 GPUI Kit / Lucide 风格 SVG。
- 图标采用统一 outline 语言：24×24 viewBox 的 2px 源描边、round cap、round join，在 16px UI 尺寸下保持一致光学重量。
- 禁止用 Unicode 字符、emoji、文本加号或手写 `PathBuilder` 代替交互图标。批准的 App Logo 是唯一默认允许的自定义品牌几何。
- Folder、Plus、Chevron、More、Settings 等保持熟悉的功能轮廓。不要给每个图标加笑脸、星星或品牌装饰。
- 图标颜色由调用方传入语义 token；普通态为次级文字色，hover 提升对比，选中态使用品牌色，危险操作使用 `danger`。

## 8. 组件状态

| 状态 | 视觉与行为契约 |
|---|---|
| Rest | 保持中性表面与明确标签，不依赖 hover 才能理解主要功能 |
| Hover | 使用 `bg_hover` 或提高图标对比；hitbox、文字位置和行高不发生跳动 |
| Selected | 使用 `brand_soft` / `bg_active`，关键文字或图标使用 `brand_primary`；不增加重色左边条 |
| Pressed | 在同一语义色族内短暂提高对比；操作仍只触发一次 |
| Focus visible | 键盘焦点必须清晰可见，推荐 2px `accent` 焦点环；不得只靠 hover 表达焦点 |
| Disabled | 降低强调并阻止操作；保持足够可读性，必要时说明禁用原因 |
| Loading | 展示与真实任务绑定的进度或占位；不伪造百分比，不用无限动画掩盖失败 |
| Error / Warning | 使用对应语义色并提供可恢复信息；不能只用颜色传达含义 |

完整的 hover、pressed、focus、disabled、loading、error 和键盘状态是组件交付的一部分。R19 尚未宣称完整视觉 parity 的状态，不得仅凭本文件把它们标记为已完成。

## 9. Composer

- Composer 是单个主表面：增长输入区在上，一行真实操作在下；不要再套多层卡片或装饰性工具栏。
- 当前壳层使用 max-width 736px、min-height 100px、radius 20px、底部 inset 16px。
- Context、mode、permission、model、thinking 与 send/stop 必须连接已有 controller 和 guard。状态缺失时隐藏或禁用，不造假。
- 不显示装饰性的 token/cost 仪表。成本信息只在有真实账单/计数来源的产品位置展示。
- 默认使用实色语义表面。玻璃、blur、Web `data-composer-*` 变体和单行 44px 胶囊不属于当前 GPUI 契约；若产品确需新增，必须单独 spec。

## 10. App Shell

```text
┌────────────────────────────────────────────────────────────┐
│ native titlebar / main header 46                           │
├──────────────┬──────────────────────────────┬──────────────┤
│ Sidebar 260  │ Center · readable max 820    │ Environment  │
│              │ Composer max 736             │ 292 / overlay│
├──────────────┴──────────────────────────────┴──────────────┤
│ optional bottom workspace · header 40 · default 272        │
└────────────────────────────────────────────────────────────┘
```

- Environment 只在有真实 project authority 时出现；standalone task 不显示 project-only rail。
- 持久右侧 workspace 打开后替代 Environment rail；底部 workspace 横跨 center 与 right。
- 用户手动折叠和宽度触发的自动隐藏是两个独立状态，resize 不得覆盖用户选择。
- Settings 当前不属于 R19 主壳层 parity 范围；沿用已实现布局，后续另行冻结。

## 11. 动效与可访问性

- 动效只解释状态变化，不装饰等待。默认交互过渡以 150ms 为上限；复杂布局动画必须单独说明价值并可中断。
- 遵循系统 Reduce Motion。开启后关闭启动扫光、shimmer、循环 pulse 和非必要位移动画，保留即时状态切换。
- 启动页首帧必须不依赖异步运行时才能显示 Logo；若未来加入品牌扫光，必须提供静态 reduced-motion 路径，并以独立任务验收。
- 所有主要操作具备键盘路径和可见焦点；图标按钮有可访问名称与稳定 hitbox。
- 文本、图标与状态在 Light/Dark 下都必须可读；不能仅靠色相区分 success、warning、danger 或 selected。

Reduced Motion、全量焦点环与 loading shimmer 当前仍需专项审计。没有自动化和真实 macOS 证据前，状态写“待验收”，不能写“已实现”。

## 12. UI 变更检查表

提交任何视觉或交互变更前，逐项确认：

- [ ] 有对应 spec；没有在组件里写死颜色、字体、圆角或共享几何。
- [ ] 同时检查 Light 与 Dark，不依赖浅色阴影修复深色边界。
- [ ] 在 1400×900、1200×760、960×600 验证布局，并覆盖 1179/1180 响应式边界。
- [ ] hover、pressed、selected、focus-visible、disabled、loading 与 error 状态符合真实 controller。
- [ ] 图标来自共享 16×16 SVG API，没有 Unicode 交互符号或未经批准的品牌装饰。
- [ ] 键盘操作、焦点可见性、Reduce Motion 与图标可访问名称已检查。
- [ ] 没有复制第三方 CSS、内部 token、图标、Logo、JavaScript 或解包资产。
- [ ] 自动化门禁通过；最终视觉结论有真实 macOS 启动、交互与截图证据。

## 13. 当前落地状态

| 范围 | 状态 |
|---|---|
| Light/Dark 语义颜色、品牌蓝、状态色 | 已进入 `ThemeColors` 并有 token 测试 |
| 正文、消息、代码、标题、Sidebar 字体层级 | 已进入 `Typography` |
| R19 壳层、Composer、Environment、Workspace 几何 | 已进入 `Layout` 并有冻结测试 |
| 共享 16×16 GPUI Kit/Lucide 图标规则 | R18 已落地 |
| 真实主壳层与 1180px Environment 响应式 | R19 已落地并完成真实 macOS 验收 |
| 完整 focus/pressed/disabled 状态视觉审计 | 待专项验收 |
| Reduce Motion 全应用审计 | 待专项验收 |
| Settings 视觉统一 | R19 明确留待后续阶段 |

## 14. 变更方式

设计守则的变化必须与实现保持可追踪：

1. 在本文件修改设计意图、语义和验收边界。
2. 若涉及共享数值，同一任务更新 `vega_theme` 类型化 token 与对应测试。
3. 更新受影响的冻结任务规格或说明其优先级，不留下两个都自称权威的冲突值。
4. 在真实 macOS 上验收关键状态和响应式边界，再写入交付记录。

本文件不以“搬运更多 token”为目标；只有被 Vega 产品需要、能形成语义、可由实现和测试约束的值，才进入设计系统。

## 15. 变更记录

- v1.0 (2026-09-09)：建立 Vega 原生视觉语言、语义 token、组件状态与验收检查表。
- v1.1 (2026-09-10)：明确 clean-room 来源边界，并与 R19/当前主题实现同步校正 UI 验收规格。

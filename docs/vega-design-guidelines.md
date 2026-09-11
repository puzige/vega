# Vega 设计守则

**版本** v1.20 · 2026-09-11

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
2. 已冻结任务规格对其范围内的精确几何和行为拥有优先级，例如 [R18 品牌 UI](vega-r18-brand-ui.md)、[R19 主窗口壳层](vega-r19-codex-parity.md) 与更新的 [R21 当前截图对标](vega-r21-screenshot-parity.md)。
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
| `bg_sidebar` | `#FAF9F9` | `#191919` | 侧栏背景 |
| `bg_elevated` | `#FFFFFF` | `#2A2A2A` | 卡片、Composer、浮起表面 |
| `bg_hover` | `#ECECEC` | `#323232` | 中性 hover |
| `bg_active` | `#EAF2FC` | `#203247` | 当前项目、任务与选中控件 |
| `border_subtle` | `#E8E8E8` | `#383838` | 1px 分隔线与细边框 |
| `text_primary` | `#191C1F` | `#EDEDED` | 正文与主要标签 |
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
| 元数据 | 12px | 时间、状态、说明及 Sidebar 辅助信息；不得承载主要操作 |
| 正文 / 控件 / Sidebar 主条目 | 13px | 正文行高 1.55；Sidebar 保持 32px 行高 |
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
| Sidebar | default 304px / 240–365px | 内边距 12px；行高 32px；导航行圆角 8px；拖拽宽度持久化 |
| 主内容外间隙 | 0px | R21 平直分栏；Sidebar 与主面板用 1px 分隔 |
| 主 Header | 46px | 底部 1px 分隔线 |
| 可读内容列 | max 820px | 居中；最小水平内边距 16px |
| Composer | max 736px / min-height 100px | 圆角 20px，内容可因多行或错误增长 |
| 普通 Panel / Card | radius 12px | 默认容器圆角 |
| Environment rail | 320px | 304px card + 16px right inset；≥1230px 时可持久显示 |
| Environment card | 304px / inset 16px / radius 18px | 轻边框，必要时使用克制小阴影 |
| Settings 内容列 | max 744px | 与剩余主区域水平居中 |
| Settings boolean switch | 32×20px | 只绑定真实布尔设置，不为对标截图造状态 |
| 大型浮层 | max 350px / radius 18px | 小型菜单按内容收窄，不强制拉伸 |
| Workspace header | 40px | 右侧或底部工作区共享 |
| Terminal toolbar | 32px | 状态靠左，终端操作组成右侧紧凑按钮组 |
| 底部 Workspace | default 272px | 保留现有最小值和拖拽行为 |
| 右侧 Workspace | 43% / min 270px | 用户 resize 结果持久化 |

主窗口设计与截图验收使用 1403×860，并回归 1400×900；应用最小窗口为 960×600。响应式验收必须覆盖 1229px 与 1230px 两侧，确保 Environment 的 overlay/rail 切换不会丢失用户折叠状态或真实操作。

### 6.2 圆角与表面

- 普通卡片默认 12px；Composer 20px；Environment 18px。小型按钮使用共享组件既有尺度，不为单个页面新造圆角档位。
- GPUI 当前使用普通原生圆角，不模拟 CSS `corner-shape` / superellipse。未来若增加 squircle，必须形成共享 renderer 和视觉回归，不做组件级渐进增强。
- 主内容与 Sidebar 依靠中性色差和细分隔表达层级。浮层、菜单、临时 Environment 卡片可以使用轻阴影；常驻卡片默认不叠加阴影阶梯。
- 不使用 blur/半透明玻璃作为可读性的必要条件。任何材质效果都必须在无 blur 时仍有清晰边界。

## 7. 图标与品牌形态

- 所有共享功能图标使用固定 16×16 容器，通过 `vega_ui::icons` 映射 GPUI Kit / Lucide 风格 SVG。
- 图标采用统一 outline 语言：24×24 viewBox 的 2px 源描边、round cap、round join，在 16px UI 尺寸下保持一致光学重量。
- 禁止用 Unicode 字符、emoji、文本加号或手写 `PathBuilder` 代替交互图标。批准的 App Logo 是唯一默认允许的自定义品牌几何。
- Folder、Plus、Chevron、More、Settings 等保持熟悉的功能轮廓。项目展开状态由打开/闭合 Folder 自身表达，不再叠加一个独立 Chevron。不要给每个图标加笑脸、星星或品牌装饰。
- 图标颜色由调用方传入语义 token；普通导航图标和选中导航图标均保持中性，hover 提升对比，危险操作使用 `danger`。品牌色只用于主操作、焦点和明确的品牌/Agent 语义。

## 8. 组件状态

| 状态 | 视觉与行为契约 |
|---|---|
| Rest | 保持中性表面与明确标签，不依赖 hover 才能理解主要功能 |
| Hover | 使用 `bg_hover` 或提高图标对比；hitbox、文字位置和行高不发生跳动 |
| Selected | 普通导航使用中性 `bg_active` + `text_primary`，不染品牌色且不增加重色左边条；只有明确的主操作或品牌/Agent 状态才使用 `brand_soft` / `brand_primary` |
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
│ Sidebar 304* │ Center · readable max 820    │ Environment  │
│ 240–365      │ Composer max 736             │ 320 / overlay│
├──────────────┴──────────────────────────────┴──────────────┤
│ optional bottom workspace · header 40 · default 272        │
└────────────────────────────────────────────────────────────┘
```

- Environment 只在有真实 project authority 时出现；standalone task 不显示 project-only rail。
- 持久右侧 workspace 打开后替代 Environment rail；底部 workspace 横跨 center 与 right。
- Workspace header 只承载标签与 pane 级操作；内容级工具栏单独成行，并把相关操作收进同一尾部按钮组，禁止用三个同级 `space-between` 元素把中间操作推到面板中央。
- Terminal 内容在状态栏下使用一致内边距；PTY 行列数以扣除 chrome 和内边距后的真实 canvas bounds 为准。
- Sidebar 宽度、用户手动折叠和窗口触发的自动隐藏是三个独立状态，resize 不得覆盖另外两者。
- Sidebar 的常驻 Settings action 使用完整 32px 导航行高，位于 12px 左右与底部 inset 内，并使用与其他导航行一致的 8px 圆角。它是侧栏内容栅格中的普通导航行，不是贴住窗口边缘的 footer 条带。
- Sidebar 的任务信息架构按 `Pinned / Projects / Recents` 排列，区块标题使用普通首字母大写而非全大写；任务在三个投影位置中只能出现一次。区块标题和项目行的辅助操作默认保持安静，仅在所属标题/行 hover、键盘聚焦或菜单打开时显现，且显隐不得造成布局跳动。
- `Pinned` 标题本身已表达置顶语义，区内任务行不重复渲染 Pin 图标、所属项目或空占位。无前导图标的 Pinned 与顶层 Recents 任务标题和各自区块标题左对齐；项目 Folder 与名称保持 8px 间距，项目子任务继续落在距行左侧 32px 的内容列以表达层级。
- Sidebar 区块按内容自然高度连续排列，不以 Projects 撑满剩余空间。Projects 默认显示 5 个顶层项目、Recents 默认显示 10 个任务；有更多内容时使用对齐内容列的 `Show More / Show Less` 在当前会话内渐进展开，外层 Sidebar 统一承担溢出滚动。
- Sidebar 对话行静止时只展示标题，不展示相对时间；Pinned 行也不重复展示所属项目。尾部菜单触发器仅在 hover、键盘聚焦或菜单打开时显现，并保持固定命中区避免布局跳动。每个展开项目默认显示 5 条非置顶任务，超出时使用独立的 `Show More / Show Less` 渐进展开，控件与 32px 项目子任务内容列对齐。
- 全局搜索入口使用窗口顶部共享控制组中的放大镜按钮，紧邻 Sidebar 显隐按钮并位于 Back / Forward 之前；Sidebar 展开或隐藏时位置和数量都保持稳定。按钮带“搜索 (⌘K)”可访问名称与提示，点击或键盘激活打开现有搜索面板；保留 Command-K，Sidebar 内容区不再重复渲染搜索入口。此规则取代 R39 的 Sidebar 局部位置，精确规格见 [R41 Titlebar Search adjacency](vega-r41-titlebar-search.md)。
- 窗口顶部 `Sidebar / Search / Back / Forward` 四个控制统一使用 28×28px 方形交互面、16px 居中图标和 4px 相邻间隔；禁用的历史按钮也保留同尺寸槽位，保证四个图标中心恒定相隔 32px。精确规格见 [R43 Titlebar control spacing](vega-r43-titlebar-control-spacing.md)。
- 全局搜索面板使用独立于紧凑菜单的 520px 宽度与 480px 最大高度 token；宽度在窄窗口保留左右各 16px，短窗口按既有顶部和底部安全区收缩。输入、范围切换与键盘提示保持可见，长结果只在结果区内部滚动，禁止让面板随结果延伸成接近整窗的窄长列。精确规格见 [R40 Search palette geometry](vega-r40-search-palette-geometry.md)。
- `Pinned` 存在时，其最后一条任务与后续 `Projects` 标题之间使用 12px 分组间距（基础 8px 区块间距加一个 4px 节奏单位）；这一补偿只属于 Pinned→Projects，不改变其他区块距离或任务行选中背景。
- Pinned 任务标题继续与 `Pinned` 区块标题和顶层 Recents 标题落在同一内容列；其选中、hover 与焦点表面向该内容列左侧扩展 8px，并在表面内部保留等量的 8px 前导 padding，避免文字贴住圆角背景且不移动标题列。
- Pinned 的内层与 Sidebar 外层滚动视口必须包含上述完整表面，不能裁掉前导 padding 或左侧圆角；验收须查看真实绘制结果，不能只依赖行布局坐标。精确规格见 [R37 Pinned clip](vega-r37-pinned-clip.md)。
- Sidebar 的 `Pinned / Projects / Recents` 组织内容列与应用边缘保留 8px 逻辑内缩；三组标题、任务与项目行共用同一前导列，内容列右边缘固定，避免选中表面贴住窗口边缘。精确规格见 [R38 Sidebar content inset](vega-r38-sidebar-content-inset.md)。
- Sidebar 持久选中态只标识最具体的活动目标：打开选中项目内的任务时，仅任务行使用中性 active 背景，外层项目行保持静止并以 Folder open 图标表达展开；只有项目被选中且没有打开该项目任务时，项目行才保留 active 背景。Pinned 中的项目任务遵循同一优先级。精确规格见 [R42 Single selection highlight](vega-r42-single-selection-highlight.md)。
- Settings 采用与当前 Sidebar 同宽的导航 rail 和 744px 最大内容列；只展示 Vega 已有的真实设置页。

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
- [ ] 在 1403×860、1200×760、960×600 验证布局，并覆盖 1229/1230 响应式边界。
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
| R19 壳层、Composer、Environment、Workspace 几何 | 已进入 `Layout` 并有冻结测试；R21 数值以其实现提交为准 |
| 共享 16×16 GPUI Kit/Lucide 图标规则 | R18 已落地 |
| 真实主壳层与 Environment 响应式 | R21 的 1230px 基线已实现并通过自动化；待真实 macOS 视觉验收 |
| 完整 focus/pressed/disabled 状态视觉审计 | 待专项验收 |
| Reduce Motion 全应用审计 | 待专项验收 |
| Settings 视觉统一 | R21 五页壳层与真实 Sidebar switch 已实现并通过自动化；待真实 macOS 视觉验收 |

## 14. 变更方式

设计守则的变化必须与实现保持可追踪：

1. 在本文件修改设计意图、语义和验收边界。
2. 若涉及共享数值，同一任务更新 `vega_theme` 类型化 token 与对应测试。
3. 更新受影响的冻结任务规格或说明其优先级，不留下两个都自称权威的冲突值。
4. 在真实 macOS 上验收关键状态和响应式边界，再写入交付记录。

本文件不以“搬运更多 token”为目标；只有被 Vega 产品需要、能形成语义、可由实现和测试约束的值，才进入设计系统。

## 15. 变更记录

- v1.24 (2026-09-11)：统一 Sidebar、Search、Back、Forward 的 28px 方形命中框与 4px 间距，消除混用按钮内边距造成的视觉不等距。
- v1.23 (2026-09-11)：项目与其活动任务不再同时高亮；任务选中态优先于外层项目选中态。
- v1.22 (2026-09-11)：搜索按钮进入共享窗口控制组，固定紧邻 Sidebar 显隐按钮并位于 Back / Forward 之前。
- v1.21 (2026-09-11)：全局搜索面板改用 520px 独立宽度和 480px 最大高度，长结果在面板内部滚动。
- v1.20 (2026-09-11)：搜索改为 Sidebar 顶部工具栏的放大镜按钮，移除整行搜索入口并保留原有搜索动作与快捷键。

- v1.0 (2026-09-09)：建立 Vega 原生视觉语言、语义 token、组件状态与验收检查表。
- v1.1 (2026-09-10)：明确 clean-room 来源边界，并与 R19/当前主题实现同步校正 UI 验收规格。
- v1.2 (2026-09-10)：纳入 R21 用户可见截图测量所得的 Sidebar、Environment、Settings 与菜单几何，并冻结 1229/1230px 响应式边界。
- v1.3 (2026-09-10)：冻结 R22 Workspace/Terminal Panel 的职责分层、32px 内容工具栏、尾部操作分组与终端内容留白。
- v1.4 (2026-09-10)：冻结 R23 Sidebar 常驻 footer action 的 32px 行高、内容列满宽与单层 12px 水平内边距。
- v1.5 (2026-09-10)：按用户复验纠正 R23 的“占满”解释：Settings footer 改为 rail 左右与底部 edge-to-edge，12px inset 仅约束普通 Sidebar 内容。
- v1.6 (2026-09-10)：再次按用户原生复验纠正 R24：Settings 回归 12px 内容栅格；侧栏导航采用中性灰选中态与 8px 圆角；项目移除独立 Chevron，由 Folder open/closed 图标表达展开状态。精确规格见 [R25 Sidebar navigation](vega-r25-sidebar-navigation.md)。
- v1.7 (2026-09-10)：侧栏信息架构改为 `PINNED / PROJECTS / RECENTS`，并冻结 section/project/task 辅助操作的 hover/focus/menu-open 显现规则。精确规格见 [R26 Sidebar sections](vega-r26-sidebar-sections-hover.md)。
- v1.8 (2026-09-11)：移除 Pinned 行内重复的 Pin 图标，统一任务标题内容列；项目 Folder 与名称间距改为 8px，并固定 Pinned 项目元数据列。精确规格见 [R27 Sidebar row alignment](vega-r27-sidebar-row-alignment.md)。
- v1.9 (2026-09-11)：Sidebar 区块标题统一为 `Pinned / Projects / Recents` 普通首字母大写，保留既有弱化层级、顺序与交互。精确规格见 [R28 Sidebar heading case](vega-r28-sidebar-heading-case.md)。
- v1.10 (2026-09-11)：Sidebar 区块改为内容自然排列；Projects 默认 5 个、Recents 默认 10 个，并以 `Show More / Show Less` 渐进展开，消除 Projects 与 Recents 之间的弹性空白。精确规格见 [R29 Sidebar progressive lists](vega-r29-sidebar-progressive-lists.md)。
- v1.11 (2026-09-11)：Sidebar 导航建立独立字号层级：主条目 15px、分组标题与辅助信息 13px，保留 32px 行高且不放大全局正文或菜单。精确规格见 [R30 Sidebar typography](vega-r30-sidebar-typography.md)。
- v1.12 (2026-09-11)：按用户原生复验回退 R30 字号，Sidebar 恢复主条目 13px、分组与辅助信息 12px；Pinned 行移除遗留的 32px 空缩进并与分组标题左对齐。精确规格见 [R31 Sidebar rollback and Pinned leading edge](vega-r31-sidebar-typography-revert-pinned-leading.md)。
- v1.13 (2026-09-11)：把无图标顶层任务的零前导缩进扩展到 Recents，使 Pinned 与 Recents 标题分别和区块标题左对齐；项目子任务保留 32px 层级缩进。精确规格见 [R32 Sidebar Recents leading edge](vega-r32-sidebar-recents-leading.md)。
- v1.14 (2026-09-11)：Sidebar 对话行移除静止时间，Pinned 同时移除项目归属；每个展开项目默认显示 5 条任务，并提供独立的 `Show More / Show Less`。精确规格见 [R33 quiet thread rows](vega-r33-sidebar-quiet-thread-rows.md)。
- v1.15 (2026-09-11)：Sidebar 顶部“新建任务”和“搜索”共享同一行宽与尾部快捷键列，移除搜索行额外外边距。精确规格见 [R34 shortcut column](vega-r34-sidebar-shortcut-column.md)。

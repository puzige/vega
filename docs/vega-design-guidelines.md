# Vega 设计守则

**版本** v1.32 · 2026-09-13

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

Issue #103：展开的工具详情与 thinking 正文使用 240px 最大高度
（`Layout::DISCLOSURE_CONTENT_MAX_HEIGHT`），工具组子列表使用 320px 最大高度
（`Layout::TOOL_GROUP_MAX_HEIGHT`）。短内容自然收缩，标题保留在滚动区域外；
长内容在内部滚动且不得压缩子行。完整交互与验收见
[Issue #103](vega-issue-103-scroll-window.md)。两个 token 已实现并通过 GPUI 几何回归验证；原生验收待完成。

| 区域 | Token / 目标值 | 说明 |
|---|---:|---|
| 原生标题栏前导留白 | 96px | 为 macOS traffic lights 保留 |
| Sidebar | default 304px / 240–365px | 内边距 12px；行高 32px；导航行圆角 8px；拖拽宽度持久化 |
| 主内容外间隙 | 0px | R21 平直分栏；Sidebar 与主面板用 1px 分隔 |
| 主 Header | 46px | 底部 1px 分隔线 |
| Sidebar 顶部工具栏 | `MAIN_HEADER_HEIGHT` 46px / `SIDEBAR_TOOLBAR_CONTENT_GAP` 18px | Issue #129：工具栏从窗口 y=0 起且不收缩；后接 18px 间距，使新建任务仍从 y=64 起 |
| 可读内容列 | max 768px | 居中；最小水平内边距 16px；与 Composer **同宽**（issue #100） |
| Composer | max 768px / min-height 100px | **与正文列同宽**（issue #100；参考实现两者共用 `--thread-content-max-width: 48rem`）；圆角 20px，内容可因多行或错误增长；包裹列上下 padding 12/16px（`Layout::COMPOSER_PADDING_TOP/BOTTOM`，值不变，R45 起 token 化） |
| Composer utility bar | h 37px / inset 19px / radius 12px（顶部） | 仅新建任务页；宽 = 卡片宽 − 2×19，`mx_auto` 共用卡片中轴；bar 底 == 卡片顶（零重叠）；chip gap 8px、首 chip inset 14.5px；chip 高 28px、水平 padding 8px |
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

- 普通卡片默认 12px；Composer 20px；Environment 18px；Composer utility bar 顶部 12px 且底部无圆角（被卡片接续）。小型按钮使用共享组件既有尺度，不为单个页面新造圆角档位。
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

### 8.1 用户消息气泡（Issue #78）

用户文字右对齐于可读消息列；短消息随内容收缩，最大宽度为列宽的 80%（`Layout::USER_MESSAGE_MAX_WIDTH_RATIO`）。气泡采用 `brand_soft` 背景与 `text_primary` 正文，圆角 16px（`Layout::USER_MESSAGE_RADIUS`），水平/垂直内边距为 12/8px，不显示「你」标签。内部空行、CJK/Latin 混排与无空格长文本必须完整换行。用户图片同样沿列右对齐；助手、工具和错误呈现保持既有布局。最大列宽 768px 时，气泡宽不超过 614.4px，正文宽不超过 590.4px。详见 [Issue #78](vega-issue-78-message-bubbles.md)。

### 8.2 消息复制（Issue #78 follow-up，当前禁用）

2026-09-23 用户复验：消息下方常驻的动作行读起来很丑、且与布局耦合，先禁用该交互。开关 `conversation_stream::MESSAGE_COPY_ACTIONS_ENABLED = false` 时 `message_with_copy` 原样返回消息体，不挂载也不预留任何动作行，几何回到 #144 之前的基线；`MessageCopy` 缓冲、共享 Copy 图标与渲染分支全部保留，待重新设计放置方式后只需翻转该开关即可恢复。

启用状态下的冻结契约如下（保留供恢复时对照）：用户文字与助手正文在消息下方各保留一行共享 24px 图标按钮位置，用户侧靠右、助手侧靠左。动作默认透明，消息与动作组成连续 hover 区域；hover 或按钮获得键盘焦点时显现，显隐不改变高度。复制图标复用共享 SVG，tooltip 与可访问名称为「复制消息」，颜色采用 `text_secondary`、`bg_hover`、`bg_active`。只复制该项完整源文，保留 Markdown 与尾换行；流式读取最新正文，不包含工具/思考/错误提示，空正文与纯图片不提供复制动作。详见 [冻结契约](vega-issue-78-hover-copy.md)。

## 9. Composer

- Composer 是单个主表面：增长输入区在上，一行真实操作在下；不要再套多层卡片或装饰性工具栏。
- 当前壳层使用 max-width 768px（与正文列同宽，issue #100）、min-height 100px、radius 20px、底部 inset 16px。
- utility bar（R49；无项目草稿由 A8-01 取代）：**新建任务草稿始终渲染** —— 即使未选择项目，也显示可选项目的文件夹 chip；有真实 Git 项目时再显示分支 chip。已发出第一条消息的会话页只渲染卡片本体，谓词必须是真实渲染可见性，不得用 `hidden()` 或零高度占位。最多两格（文件夹 / 分支），Codex 的 `Local`（执行环境）不移植、不占位、不置灰。
- utility bar 几何：保留 R49 的 bar 高 37px、相对卡片左右各内缩 19px 并共用卡片中轴、顶部圆角 12px 而底部无圆角；bar 底边与卡片顶边**零重叠**（`bar bottom == card top`），靠父列顺序堆叠形成「标签页压在卡片上」的层叠观感，不使用负 margin。2026-09-16 用户要求收紧两个 chip：边界间距由 28px 改为 8px，首个 chip 距 bar 左缘仍为 14.5px。
- utility bar chip 样式（取代 R49 对触发器的旧定义）：两个触发器均高 28px、水平 padding 8px、全胶囊；保留 `icon(16px, text_secondary)` + `label(Typography::SIDEBAR, text_primary)`、内部 `gap_2`。静止无边框且底色透明；hover 或对应菜单打开时显示专用主题覆盖色，文字和图标不参与透明度变化；关闭并移走指针后恢复透明。浅色覆盖色为 Vega 自主选定的 `#DBDBDB × 0.6`，叠在 `#FAF9F9` 上约为 `#E7E7E7`；深色为白色 × 0.10，叠在 `#191919` 上约为 `#303030`。分支保留 GitBranch 图标。文件夹 chip tooltip「切换项目」、分支 chip tooltip「切换分支」。这些为产品目标，不代表第三方内部实现。详见 [utility chip states](vega-utility-chip-states.md)；非 composer 的 R19 带框触发器不变。
- 文件夹 chip 打开项目下拉（列出 sidebar 已有项目）；无项目草稿显示「选择项目」。选中、切换或解除项目时，草稿绑定及 Composer/分支上下文必须与共享 `SelectedProject` 同步，不得只换全局选择而留下旧任务绑定；复用既有项目数据源，不新建数据管道。分支 chip 复用既有 `BranchSelector`（open/close/切换/pending/错误码/focus/滚动语义不变），仅由挂载点切换 trigger chrome（`set_chip_chrome`），非 git 项目按既有 `NonGit` 语义隐藏。详见 [A8-01](vega-a8-composer-project-entry.md)。
- 已有消息的 Git 项目会话继续不渲染 utility bar；其 BranchSelector 通过 Composer 卡片底部操作行中的分支 chip 保持可达。该入口复用同一 selector，不能恢复 Environment 分支行或改变 R49 utility bar 几何。详见 [Issue #191](vega-issue-191-branch-entry.md)。
- Context、mode、permission、model、thinking 与 send/stop 必须连接已有 controller 和 guard。状态缺失时隐藏或禁用，不造假。
- Issue #58：权限选择扩展为只读、确认、自动、完全访问四项；自动保持沙箱，完全访问显式关闭 bash 的 OS 沙箱且仍保留危险确认。完全访问使用共享 Warning 图标及 warning token，复用既有菜单布局与键盘路径；详见 [Full access 规格](vega-issue-58-full-access.md)。
- 不显示装饰性的 token/cost 仪表。成本信息只在有真实账单/计数来源的产品位置展示。
- 默认使用实色语义表面。玻璃、blur、Web `data-composer-*` 变体和单行 44px 胶囊不属于当前 GPUI 契约；若产品确需新增，必须单独 spec。

## 10. App Shell

```text
┌────────────────────────────────────────────────────────────┐
│ native titlebar / main header 46                           │
├──────────────┬──────────────────────────────┬──────────────┤
│ Sidebar 304* │ Center · readable max 768    │ Environment  │
│ 240–365      │ Composer max 768 (= 正文列)   │ 320 / overlay│
├──────────────┴──────────────────────────────┴──────────────┤
│ optional bottom workspace · header 40 · default 272        │
└────────────────────────────────────────────────────────────┘
```

- Environment 只在有真实 project authority 时出现；standalone task 不显示 project-only rail。
- Issue #141：新窗口与重启后 Environment 默认折叠，由用户点击头部按钮展开；未手动展开前，项目/任务切换与窗口 resize 不自动展开。不新增持久化设置，详见 [Environment 默认折叠](vega-issue-141-environment-default-collapsed.md)。
- Environment 卡片不再承载分支入口（R49 人类裁决）：`environment-branch` 行已删除，分支入口唯一化到 composer utility bar，符合「同一动作只有一个入口」。卡片其余行（标题、项目行、Changes / Review、Local terminal）保持不变。
- 持久右侧 workspace 打开后替代 Environment rail；底部 workspace 横跨 center 与 right。
- Workspace header 只承载标签与 pane 级操作；内容级工具栏单独成行，并把相关操作收进同一尾部按钮组，禁止用三个同级 `space-between` 元素把中间操作推到面板中央。
- Terminal 内容在状态栏下使用一致内边距；PTY 行列数以扣除 chrome 和内边距后的真实 canvas bounds 为准。
- Workspace header 的前导 Chevron 是唯一的 pane 隐藏入口；尾部 Plus 只打开创建菜单，菜单不重复列出已有标签、预览恢复或“关闭全部终端”。尾部不再重复渲染 Minimize。全局 Terminal 入口显示或隐藏当前项目终端，但显示动作不自动夺取任务 Composer 焦点；只有显式选择终端标签、终端画布、“新建终端”或 Maximize/Restore 当前 Workspace 才把键盘输入交给当前可见内容。Dock 仅改变布局并继续保留 Composer 焦点；Maximize/Restore 必须聚焦所选 Workspace 内容，不能把焦点留在已卸载的 Composer。隐藏终端不再同时获得通用 Restore 与 Terminal 两个恢复入口。精确规格见 [R44 Terminal panel interaction model](vega-r44-terminal-panel-logic.md)。
- Workspace 的可见性必须以真实响应式挂载为准，不能把 `hidden == false` 当成已经可见。终端停留在因窄窗口而不再渲染的 right pane 时，第一次全局 Terminal toggle 或 Environment reveal 必须迁移同一终端实体和 PTY 到 bottom 并立即显示，同时保留 Composer 焦点；正常宽度下既有 toggle 与 Dock 语义不变。
- Sidebar 宽度、用户手动折叠和窗口触发的自动隐藏是三个独立状态，resize 不得覆盖另外两者。
- Sidebar 的常驻 Settings action 使用完整 32px 导航行高，位于 12px 左右与底部 inset 内，并使用与其他导航行一致的 8px 圆角。它是侧栏内容栅格中的普通导航行，不是贴住窗口边缘的 footer 条带。
- Sidebar 的任务信息架构按 `Pinned / Projects / Recents` 排列，区块标题使用普通首字母大写而非全大写；任务在三个投影位置中只能出现一次。区块标题和项目行的辅助操作默认保持安静，仅在所属标题/行 hover、键盘聚焦或菜单打开时显现，且显隐不得造成布局跳动。
- `Pinned` 标题本身已表达置顶语义，区内任务行不重复渲染 Pin 图标、所属项目或空占位。无前导图标的 Pinned 与顶层 Recents 任务标题和各自区块标题左对齐；项目 Folder 与名称保持 8px 间距，项目 Folder 图标与区块标题同列，项目子任务落在 `base + SIDEBAR_ROW_INSET(24)` 的共享文本列以表达层级（R48 起取代旧的 32px 内容列）。
- Sidebar 区块按内容自然高度连续排列，不以 Projects 撑满剩余空间。Projects 默认显示 5 个顶层项目、Recents 默认显示 10 个任务；有更多内容时使用对齐内容列的 `Show More / Show Less` 在当前会话内渐进展开，外层 Sidebar 统一承担溢出滚动。
- Sidebar 对话行静止时只展示标题，不展示相对时间；Pinned 行也不重复展示所属项目。尾部菜单触发器仅在 hover、键盘聚焦或菜单打开时显现，并保持固定命中区避免布局跳动。每个展开项目默认显示 5 条非置顶任务，超出时使用独立的 `Show More / Show Less` 渐进展开，控件与项目子任务共享同一文本列（`base + SIDEBAR_ROW_INSET`）。
- Sidebar 的项目操作菜单只显示 `移除项目`，不显示上下移动命令；未来的鼠标拖动排序另行设计。精简标签不改变保留本地文件与任务历史的移除语义，详见 [A8-02](vega-a8-project-removal-integrity.md)。分组菜单的移动命令不受影响。
- 全局搜索入口使用窗口顶部共享控制组中的放大镜按钮，紧邻 Sidebar 显隐按钮并位于 Back / Forward 之前；Sidebar 展开或隐藏时位置和数量都保持稳定。按钮带“搜索 (⌘K)”可访问名称与提示，点击或键盘激活打开现有搜索面板；保留 Command-K，Sidebar 内容区不再重复渲染搜索入口。此规则取代 R39 的 Sidebar 局部位置，精确规格见 [R41 Titlebar Search adjacency](vega-r41-titlebar-search.md)。
- 窗口顶部 `Sidebar / Search / Back / Forward` 四个控制统一使用 28×28px 方形交互面、16px 居中图标和 4px 相邻间隔；禁用的历史按钮也保留同尺寸槽位，保证四个图标中心恒定相隔 32px。精确规格见 [R43 Titlebar control spacing](vega-r43-titlebar-control-spacing.md)。
- 主头部尾部是恰好三个永久槽位（切换环境 / 切换终端 ⌘J / 切换右侧面板），统一 28×28 交互面、16px 居中图标与 6px 相邻间隔；每槽只表达一个全局布局表面，选中态由该表面真实 rendered 可见性驱动（`hidden == false` 不算可见），禁用时保留占位、图标降为 tertiary、不回流；R43 的 4px 间隔规则仍只适用于前导 Sidebar/Search/Back/Forward 组。三个槽位窗口锚定在窗口右上角（不属于会话列、rail 或任何 pane），任何面板开合都不得改变其坐标；凡占据窗口顶部条带的面板头部（右面板、任一面板最大化）都预留同一尾部槽位带，让面板自身尾部动作排在槽位左侧，底仓头部不预留。Environment rail 与 overlay 卡片从 header 行下方开始，不覆盖控件。Environment overlay（<1230px）支持 Esc 与点击卡片外部关闭并回焦 Composer，rail 模式无 Esc 契约。精确规格见 [R45 Header shell controls and Composer alignment](vega-r45-shell-controls-composer.md)，几何归属与顶部条带归属由 [R46 Window-anchored shell slots](vega-r46-window-anchored-shell-slots.md) 取代。
- 工具条尾随 inset 是 8px（Codex `--padding-toolbar`），不是通用 12px padding：窗口锚定槽位带以 8px 贴住窗口右缘，主头部尾随预留为 104px（3×28 槽位 + 2×6 间隔 + 8px inset），槽位中心恒定于窗口右缘 −22/−56/−90。占据顶部条带的 pane 头部在槽位带之外再预留 32px 归属 gutter（预留合计 136px），让 pane 动作组与窗口槽位带隔出一段空白归属间隔，两层控件不得读成同一条按钮带；pane 本地 dock 动作使用带指入箭头的 DockMove 图形，与槽位自身的 DockBottom/DockRight 同形去重。终端 body 使用会话同款 surface（light = #ffffff）而非内联代码块的 code_bg 灰面，文本左缘 ≈16px；终端状态行仅在异常态（Starting/Exited/Failed）渲染，Running 态整行不存在，复制由既有 ⌘C action 承接；workspace pane header 无下边框，分隔职责归各内容行，终端面板最终只保留顶边框一条分隔线。以上几何与颜色真值来源为 Codex WebView token 源码（`app-initial-*.css`）与原生 2x 像素/AX 实测。精确规格见 [R47 Panel structure alignment](vega-r47-panel-structure-alignment.md)。
- 全局搜索面板使用独立于紧凑菜单的 520px 宽度与 480px 最大高度 token；宽度在窄窗口保留左右各 16px，短窗口按既有顶部和底部安全区收缩。输入、范围切换与键盘提示保持可见，长结果只在结果区内部滚动，禁止让面板随结果延伸成接近整窗的窄长列。精确规格见 [R40 Search palette geometry](vega-r40-search-palette-geometry.md)。
- `Pinned` 存在时，其最后一条任务与后续 `Projects` 标题之间使用 12px 分组间距（基础 8px 区块间距加一个 4px 节奏单位）；这一补偿只属于 Pinned→Projects，不改变其他区块距离或任务行选中背景。
- Pinned 任务标题继续与 `Pinned` 区块标题和顶层 Recents 标题落在同一内容列；其选中、hover 与焦点表面向该内容列左侧扩展 8px，并在表面内部保留等量的 8px 前导 padding，避免文字贴住圆角背景且不移动标题列。
- Pinned 的内层与 Sidebar 外层滚动视口必须包含上述完整表面，不能裁掉前导 padding 或左侧圆角；验收须查看真实绘制结果，不能只依赖行布局坐标。精确规格见 [R37 Pinned clip](vega-r37-pinned-clip.md)。
- Sidebar 的 `Pinned / Projects / Recents` 组织内容列与应用边缘保留 8px 逻辑内缩；三组标题、任务与项目行共用同一前导列，内容列右边缘固定，避免选中表面贴住窗口边缘。精确规格见 [R38 Sidebar content inset](vega-r38-sidebar-content-inset.md)。
- Sidebar 缩进阶梯（R48）：以导航内容原点 base 为基准，`base` = 标签列 = folder 图标列，`base + SIDEBAR_ROW_INSET(24)` = 项目名称列 = 项目子任务文本列 = `Show More / Show Less` 列。`SIDEBAR_ROW_INSET` 是导出量而非独立参数，等于 folder 图标 16px 加项目行 `gap_2` 8px；项目行自身不设前导内边距，只靠图标 + 间距自然落到文本列，无图标的行直接消费该 token，因此改动图标尺寸或行间距必须同步更新 token，否则两列重新错位。section 标签通过 `SIDEBAR_LABEL_INSET`(0) 显式表达其贴 base 的列，不再隐式继承容器。真值来源为 Codex 实机 AX 控件盒与原生 2x 像素测量（标签 17.5 / folder 图标 16.5 / folder 文本 41.0 / child 文本 40.5，Vega 收敛为 base 16 与文本列 40）。精确规格见 [R48 Sidebar indent ladder](vega-r48-sidebar-indent-ladder.md)。
- Sidebar 持久选中态只标识最具体的活动目标：打开选中项目内的任务时，仅任务行使用中性 active 背景，外层项目行保持静止并以 Folder open 图标表达展开；只有项目被选中且没有打开该项目任务时，项目行才保留 active 背景。Pinned 中的项目任务遵循同一优先级。精确规格见 [R42 Single selection highlight](vega-r42-single-selection-highlight.md)。
- Settings 采用与当前 Sidebar 同宽的导航 rail 和 744px 最大内容列；只展示 Vega 已有的真实设置页。

## 11. 动效与可访问性

- 动效只解释状态变化，不装饰等待。默认交互过渡以 150ms 为上限；复杂布局动画必须单独说明价值并可中断。
- 遵循系统 Reduce Motion。开启后关闭启动扫光、shimmer、循环 pulse 和非必要位移动画，保留即时状态切换。
- 启动页首帧必须不依赖异步运行时才能显示 Logo；若未来加入品牌扫光，必须提供静态 reduced-motion 路径，并以独立任务验收。
- 所有主要操作具备键盘路径和可见焦点；图标按钮有可访问名称与稳定 hitbox。
- Workspace 的全局 Review 显示操作保留当前焦点；用户显式激活 Diff tab 才把焦点交给 Review 内容。两条路径必须走独立焦点意图，详见 [Issue #192](vega-issue-192-review-focus.md)。
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

- Issue #78 follow-up (2026-09-23)：消息悬停复制动作**当前禁用**。用户复验认为消息下方常驻的动作行很丑、且与布局耦合，要求先禁用。开关 `conversation_stream::MESSAGE_COPY_ACTIONS_ENABLED = false` 时 `message_with_copy` 原样返回消息体，不挂载也不预留动作行，几何回到 #144 之前基线；缓冲、图标与渲染分支保留，翻转开关即可恢复。详见 [禁用记录](vega-issue-78-disable-copy-delivery.md) 与 [冻结契约](vega-issue-78-hover-copy.md)。

- Issue #100 (2026-09-21)：Composer 与正文列**统一为 768px**。参考实现用**同一个** `--thread-content-max-width: 48rem`（=768px）同时驱动正文列与 Composer 容器，两者本应共用一条边；Vega 此前从截图分别量出 820 / 736，Composer 每侧窄 42px。`Layout::CONTENT_MAX_WIDTH` 与 `Layout::COMPOSER_MAX_WIDTH` 均改为 768，并由冻结测试与编译期断言保证恒等；Composer 的圆角/高度/内边距/发送按钮与 utility bar 几何全部不变。详见 [Issue #100 composer width](vega-issue-100-composer-width.md)。

- A8-02 (2026-09-18)：项目操作菜单只保留 `移除项目`，移除上下移动命令及标签中的解释性后缀；保留文件与任务历史的安全语义不变。
- A8-01 (2026-09-17)：新建草稿在无项目时也显示 Composer 项目 chip；项目切换必须同步草稿和控制器，移除单独的「添加项目文件夹以开始…」首页引导。详见 [Composer project entry](vega-a8-composer-project-entry.md)。

- v1.33 (2026-09-15)：R69 首页常驻真实 Composer：无打开任务时不再渲染静态占位卡片「新建任务并开始输入…」，改为直接渲染真实 `ConversationStream`（含 R49 utility bar 与 R57 底行），其任务为**未持久化的惰性草稿**，首次提交才落库；侧栏「新建任务」与 ⌘N 改为导航到该草稿路由，不再急切 INSERT（消除 `未命名任务` 堆积）。Composer 几何（736/100/28/20）、utility bar 几何、底行结构与配色全部不变。详见 [R69 Home lazy draft composer](vega-r69-home-lazy-draft-composer.md)。

- R51 (2026-09-13)：Workspace 保留多 Tab；Tab 高 28px、圆角 10px、标签 13px、前置类型图标 16px，水平 inset/gap 均为 8px。非激活底色透明，关闭命中区维持 24px；非激活关闭在所属 Tab hover、父 Tab focus 或关闭按钮键盘 focus 时显现且不改变几何。外层 header 和全局槽位不变，激活填充由 R50 独立承接。该尺寸是 Vega 依据用户可见截图选定的实现目标，不代表第三方内部实现。详见 [R51 Tab controls](vega-r51-tab-controls.md)。

- v1.32 (2026-09-13)：R49 Composer utility bar 对齐 Codex：§9 明确 utility bar 仅新建任务页渲染（有项目上下文且会话无消息，真实渲染可见性谓词），只做文件夹/分支两格，几何 bar 高 37、左右各内缩 19、顶部圆角 12 而底部无圆角、chip 间距 28、首 chip inset 14.5，bar 底与卡片顶零重叠形成层叠；chip 为无框 16px 图标 + 文字，hover 才出 `bg_hover` + `rounded_md`；文件夹 chip 复用 sidebar 项目数据源打开项目下拉，分支 chip 复用既有 `BranchSelector` 并由挂载点切换 trigger chrome。§10 记录 Environment 卡片不再承载分支入口（`environment-branch` 删除），分支入口唯一在 composer。新增 `COMPOSER_UTILITY_BAR_HEIGHT/INSET/RADIUS`、`COMPOSER_UTILITY_CHIP_GAP/INSET` 五个 token 并有冻结测试；会话 composer 几何（736/100/28/20）不变。真值来源为 Codex 实机原生 2x 抓帧（bar 510.0..1219.5、card 497.0..1232.5、chip 524.5/601.5/683.0）。精确规格见 [R49 Composer utility bar](vega-r49-composer-utility-bar.md)。

- v1.31 (2026-09-12)：R48 Sidebar 缩进阶梯对齐 Codex：§7 侧边栏明确 `base(16) = 标签列 = folder 图标列`，`base + SIDEBAR_ROW_INSET(24) = 文本列 = 项目子任务文本列 = Show More 列`；新增 `SIDEBAR_LABEL_INSET`(0) 与 `SIDEBAR_ROW_INSET`(24) 两个 token，后者是 folder 图标 16 + `gap_2` 8 的导出量（冻结测试断言该恒等式），项目行去掉多余的前导 8px 内边距，子任务行与渐进控件改用共享文本列 token。真值来源为 Codex 实机 AX 控件盒与原生 2x 像素测量（标签 17.5 / folder 图标 16.5 / folder 文本 41.0 / child 文本 40.5）。精确规格见 [R48 Sidebar indent ladder](vega-r48-sidebar-indent-ladder.md)。
- v1.30 (2026-09-12)：R47 面板结构对齐 Codex：工具条尾随 inset 收敛为 8px（Codex `--padding-toolbar`，取代 12px），槽位中心恒定于窗口右缘 −22/−56/−90，主头部尾随预留改为 104px；顶部条带 pane 头部在槽位带外增加 32px 归属 gutter（预留合计 136px），pane 本地 dock 动作改用 DockMove 图标与槽位图形去重；终端 body 改用 surface（`bg_base`，light = #ffffff）而非 `code_bg`，画布文本左缘 ≈16px；终端状态行仅异常态渲染（Running 态不渲染，复制走 ⌘C action）；workspace pane header 移除下边框，终端面板只保留顶边框一条分隔线。真值来源为 Codex WebView token 源码与原生 2x/AX 实测。精确规格见 [R47 Panel structure alignment](vega-r47-panel-structure-alignment.md)。
- v1.29 (2026-09-12)：主头部三个壳层槽位改为窗口锚定（窗口右上角绝对定位，垂直居中于 46px header band），任何面板开合不再改变槽位坐标；占据窗口顶部条带的面板头部（右面板、任一面板最大化）预留同一尾部槽位带，面板自身尾部动作排在槽位左侧，底仓头部不预留；Environment rail 与 overlay 卡片从 header 行下方开始，不再覆盖控件。R45 的三槽位语义、禁用保位、选中态规则与 Composer 几何全部不变。精确规格见 [R46 Window-anchored shell slots](vega-r46-window-anchored-shell-slots.md)。
- v1.28 (2026-09-12)：主头部尾部收敛为三个永久 28×28 槽位（环境 / 终端 ⌘J / 右侧），6px 间隔、真实 rendered 可见性驱动选中态、禁用保位；Environment overlay 增加 Esc 与点击外部关闭并回焦 Composer；Composer 包裹列 12/16px padding token 化（值不变）。精确规格见 [R45 Header shell controls and Composer alignment](vega-r45-shell-controls-composer.md)。
- v1.27 (2026-09-11)：冻结窄窗口下不可见 right terminal 的恢复规则：全局入口首次操作即迁移同一 tab/PTY 到 bottom，不能先隐藏幽灵 pane 或新建终端。
- v1.26 (2026-09-11)：区分 Workspace 布局动作的焦点语义：Dock 保留 Composer 焦点，Maximize/Restore 激活当前所选内容，避免最大化后输入落入已卸载 Composer。
- v1.25 (2026-09-11)：收敛 Workspace/Terminal 状态：Chevron 单独负责隐藏，Plus 菜单只负责创建；全局显示终端不抢占 Composer 焦点，并移除隐藏终端的重复 Restore 入口。精确规格见 [R44 Terminal panel interaction model](vega-r44-terminal-panel-logic.md)。
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

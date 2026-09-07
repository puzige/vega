# R4 · Vega 客户端 UI 翻新

版本 v0.2 · 2026-09-05 · Owner Codex · Executor 原生 Luna / max

用户授权：参考 Codex / ChatGPT 客户端翻新当前 UI；后续实现不使用 pi。
任务卡：`vega-review-tasks.md` R4。本卡是 ui-spec v0.4 对视觉部分的明确修订；安全、controller、性能冻结条款继续有效。

## 1. 目标与依据

采用 Codex 的项目 / 任务工作区结构，以更舒展的对话排版和简洁输入区提高阅读质量。完成原生 GPUI 界面，不将 HTML 原型冒充应用实现。

当前源码可复核的问题：composer 首排同时放模式、三个权限选项、分支、模型和思考档位；用户消息逐行带边框；新建任务为满宽黑色按钮；侧栏操作拥挤；header 含 S3 / 演示注入 / 跟随状态；空态含无功能模板。这些与实际操作的重要程度不一致。

本机 Codex 客户端静态样式版本 26.901.31953 提供了参照：4px 基准间距、侧栏默认约 275px、弱分隔线、20px composer 圆角、底部轻量操作行。只参考设计原则与标量，不复制客户端代码、字体、图标或资源。系统字体与 Vega 原创图标继续属于 Vega。

电脑使用权限在本轮早期曾返回 `Computer Use permissions are not granted`，因此当时没有把代码分析、HTML 原型或构建成功当作实际窗口视觉验收。重启 Codex 后权限已恢复，现有 Vega 窗口的 AX、截图与菜单交互已由主 Agent 复核；新本地构建仍须启动后重新完成真实窗口视觉、键盘、中文输入与窄窗走查。

## 2. 统一视觉规范

| token | Light | Dark |
|---|---|---|
| bg_base | #FFFFFF | #202020 |
| bg_sidebar | #F7F7F7 | #191919 |
| bg_elevated | #F7F7F7 | #2A2A2A |
| bg_hover | #ECECEC | #323232 |
| bg_active | #E8E8E8 | #303030 |
| border_subtle | #E8E8E8 | #383838 |
| text_primary | #202020 | #EDEDED |
| text_secondary | #676767 | #ABABAB |
| text_tertiary | #8A8A8A | #828282 |
| accent | #202020 | #EDEDED |
| code_bg | #F6F6F6 | #262626 |

success / danger / warning 保持原值及语义。必要的新语义 token 须在本规范与 ThemeColors 一起声明。颜色、字号只由 vega_theme 导出，不能在其他 crate 硬编码。既有 palette 字面值测试允许按本表更新，其余安全断言不变。

- 字体：系统 sans，代码沿用当前 mono；不捆绑 OpenAI 字体。
- 正文 13px / 1.55；对话正文 15px / 1.65；代码 12.5px；侧栏 13px、行高 34px；页面标题 16px / semibold；区块 14px / semibold；空态主标题 26px / medium；说明 / 元数据 12px。
- 间距使用 4 / 8 / 12 / 16 / 24 / 32。侧栏 260px 保留，内边距 12px，条目圆角 8px。会话和输入区 max 820px 居中，边距至少 24px。
- 普通面板圆角 12px，composer 20px，轻边框；不加阴影堆叠、背景渐变或装饰动画。popover 可保留必要阴影与强于背景的轮廓。
- 960×600 与 1280×800 都必须保持正文、发送、模型、权限可达；小于现有阈值按原规则收起侧栏。保持 macOS 原生 titlebar 和窗口按钮。

## 3. 各视图要求

### 3.1 侧栏与空态

侧栏顶部有清楚但轻量的 Vega 标识和新建任务入口；新建按钮改为 ghost row，保留无项目禁用及真实错误信息。项目、置顶、任务列表有清楚层级；选中任务使用柔和背景与主色标题，取消重色左边条。长标题截断；时间与操作避让，hover 不造成布局跳动。现有重命名 / 置顶 / 归档 / 删除 / 项目管理仍可达且复用原 handler。允许用一个可键盘访问的菜单或精简操作带收纳低频动作，禁止隐藏功能且无替代入口。

设置入口放在底部稳定位置，复用现有 Settings action（如果目前只在菜单，增加侧栏入口）。未实现的自动化占位不在普通界面展示。空态用“想在这个项目里完成什么？”与一条真实操作说明；保留实际项目选择 / 添加 / 新建入口，删除三个无功能模板按钮。无项目与有项目但未开任务的提示要区分。不要新增虚构搜索、通知、账号、附件按钮。

### 3.2 任务标题区

标题为视觉主体；Diff / Commit 可用简洁的“更改” / “提交”或图标加文字，并保留受信动作的 disabled / busy 行为。去掉普通 header 的 S3、演示注入、跟随中等工程标签。演示与 benchmark 的 public 入口保留供原 harness 使用，不放在普通产品流。失去底部跟随时，已有回到底部操作仍明确可达。

### 3.3 对话与工具

用户消息使用一块轻底色、圆角、无逐行分隔边框的连续区域。助手正文直接排在底色上，段落留白清楚；代码、引用、列表、表格沿用 renderer，不重写解析和虚拟列表。不能让用户消息裁切、折行错位或打乱现有变高 item 缓存和锚点。

工具行降低轮廓重量，清楚区分进行中 / 完成 / 失败，细节默认折叠且展开仍可用。权限、计划、错误与批准后未开始状态保留显著语义，不能为了简洁弱化危险提示。Artifact / summary 可适配统一间距，但投影与动作不变。

### 3.4 Composer

外部不再横跨全宽画重分隔线。输入内容在上方，底部控件有明确分组，避免所有 selector 挤在同一行：

1. 主输入区，placeholder“描述任务”；保留 1–8 行、中文 IME、既有 @file 组件、历史召回和快捷键。原生验收已确认 FileIndexRequested 未接入 app handler，完整文件引用列入 R5；本卡不在提示文案中宣称已可用。
2. 第一操作行：左侧模式 Ask / Plan / Execute 轻分段、模型和 thinking；右侧主发送动作。模型很长时必须截断且不挤掉发送。空间不足可换行。
3. 第二行轻量上下文：分支、权限、token / 成本计数。权限当前值必须一直可见；此卡可保留三个选项但加中文短标签“只读 / 确认 / 自动”，也可用可键盘操作的菜单承载同样三种原 action。自动权限采用已有 warning token。

模式、模型、thinking 保留真实选择与 keyboard handler；不假装尚未接入请求的 thinking 已修好。R1 的 durable model ack / pending 禁发 / 路由围栏必须在集成时保留。发送默认仍 Cmd+Enter，Enter 换行。无输入 / pending / running / approved plan 等 guard 不能变弱。

### 3.5 设置与 Diff

统一标题、间距、按钮、弱分隔线。设置按 Provider、默认设置、定价维持清楚分组；精简过度卡片嵌套，不改保存语义和 Keychain。Diff 保留真实完整审阅区域、文件导航和安全动作；使用同样的 chrome、字体和 spacing，不新增右分栏架构或改 controller。

## 4. 实现范围

- `vega_theme` tokens / Typography / 必要布局常量。
- `vega_ui` 的 sidebar、conversation render、tool / permission / plan / summary / artifact cards、settings 与 diff / commit 的视觉层。
- `vega/src/window/render.rs` 空态，`main.rs` / UI init 必要的原生图标资源注册。
- 原生验收发现的同线程标题刷新、composer 多行高度重排可在既有 UI 投影 / `text_input` 视口失效路径作最小修正；不改消息文本、IME 协议、存储或发送语义。更新标题须只拥有标题字段，保留 R1 pending / model authority；高度增长不应等待下一次键入才显示最后一行。
- 允许必要的 UI 局部菜单 / focus 状态；不改 store、conversation、runtime、provider、migration、Cargo.lock。零外部依赖。
- 允许新建共享 UI 控件 / 原创 SVG 图标模块，必须可随原生 app 正常打包。不要用 emoji 作为主要操作图标。
- `docs/vega-ui-spec.md` 同步本卡视觉修订；交付报告记录 raw logs / 状态。每个 Rust 文件 ≤1000 行。
- 独立 HTML 预览包含会话 / 空态 / 设置 / Diff、Light / Dark、窄窗口，并能复制当前设计参数。仅示例数据，不接触用户内容，放在 review 目录；不捆绑商业客户端资源。

## 5. 验收与交付

执行 fmt、clippy all-targets -D warnings、workspace test --no-fail-fast、workspace build，记录首次失败。优先跑既有真实 handler E2E / 变高虚拟列表 / composer / permissions 回归；只有新增可交互状态机才补最少必要行为测试，不为颜色与纯布局造镜像测试。

已有 R0：本机 /usr/bin/git 2.39.5 不支持 check-attr --source，12 个分支相关测试在基线失败；全量还出现过一个 artifact 超时/错误分类差异，孤立重跑通过。不得借 UI 修复绕过 Git 校验；区分旧失败与新增回归，门禁未全绿则不宣称可集成。

运行 Cargo 前与主 Agent 协调，R1 与本卡不得同时占用共享 target。交付 ≤3 个本地 commit；不得 push / PR / merge / 替换已安装应用。主 Agent 将 R1 与 UI 在独立集成分支合并并复核冲突。电脑使用权限现已恢复，但新本地构建的真实窗口截图、键盘 / 中文输入 / 窄窗走查仍须在构建启动后执行；在此之前对应项必须标 NOT RUN。

## 变更记录

- v0.1：用户授权客户端 UI 翻新；主 Agent 冻结设计、范围、与 R1 / R0 的依赖和验收边界。
- v0.2：Computer Use 恢复后已运行本地 R4 原生构建；主 Agent 发现重命名后标题区停留旧值，以及多行粘贴后最后一行需再键入才显示，明确将两项最小 UI 投影 / 视口重排修正纳入本卡。菜单位置与键盘行为继续按原契约实窗复核。
- v0.2 补充：原生 @ 输入无候选，源码确认 app 没有订阅 FileIndexRequested，apply_file_index 也无 production 调用。R5 单独补全文件引用，R4 仅将输入提示调整为当前已具备的任务输入能力。

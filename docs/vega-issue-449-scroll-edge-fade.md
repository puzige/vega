# Issue #449：全局滚动边缘渐隐

版本 v1 · 2026-10-18 · 来源 [#449](https://github.com/puzige/vega/issues/449)。用户先批准方案，再明确要求按本规格实施。基线 `75da678a69fa809159f922615e1137bb48b528ba`；任务分支 `feat/449-scroll-edge-fade`。关联 A2-04 / A2-11。

沿用 [设计守则](vega-design-guidelines.md)、[执行总纲](vega-exec-guide.md)、[角色工作流](vega-issue-workflow.md) 和 [#403 Composer 前景契约](vega-issue-403-conversation-underlay.md)。本卡明确覆盖“正文默认不加渐变”的默认视觉规则，仅增加滚动视口边缘的装饰；源文、消息状态与业务契约不变。

## 目标与范围

所有 Vega 自有滚动视口共享背景色到透明的边界过渡。普通视口按该方向真实存在的隐藏内容显隐；保留 Composer 既有固定前景过渡。内容、交互和滚动状态优先，不新增产品说明、设置开关或依赖。

覆盖普通 ScrollHandle、UniformList、变高 ListState，以及自绘 TextInput 与 Terminal。滚动内容之外的固定标题、搜索、操作栏、必要错误/权限提示、滚动条不参与渐隐。既有图片应用菜单纳入；没有滚动的图片适配画布不凭空增加滚动或渐隐。

## 实现前验收矩阵

下表在生产代码修改前冻结。自动用例只在 Actions 执行，真实原生视觉与系统集成由用户手测；表中状态不代表已实现或通过。

| ID | 前置与操作 | 可观察结果 | 自动证据与人工检查 | 初始状态 |
|---|---|---|---|---|
| A1 | 普通纵向视口：空、短、恰好放下、仅溢出少量、长内容；滚至顶部/中间/底部 | 不溢出无渐隐；起点只有结束边，中间两边，终点只有起始边；近边界连续减弱，无1px残影 | 生产共享组件 GPUI bounds / painted_quads 与边界计算；浅深主题手测 | 待实现 / CI / 手测 |
| A2 | 横向标签/代码与双轴文件预览，依次到四个边界/角落 | 左右同一规则；四角不重复加深；不染相邻容器、边框、固定操作和滚动条 | 生产普通及双轴组件渲染；工作区/代码入口接入回归 | 待实现 / CI / 手测 |
| A3 | 两类虚拟列表；有变高消息、空高过程项、前后历史水合、流式追加、用户回看 | 依据真实列表状态，不用条数乘平均高度猜边界；不强开跟随、不抢位置、不遍历所有消息；未加载的历史不冒充可滚动内容 | 实际 UniformList / ListState 的生产渲染与既有 long_session / follow 回归 | 待实现 / CI |
| A4 | 运行时 resize、筛选、切设置页、折叠工具、内容增减与主题切换 | 当帧布局后重算边界；短内容恢复无渐隐，无上帧残留；每个视口使用所属表面颜色 | 普通/虚拟组件 resize 与生产页面入口测试；浅深窄窗口手测 | 待实现 / CI / 手测 |
| A5 | 渐隐处点击、拖选复制、Tab/方向键导航、拖滚动条；嵌套滚动到边界 | 装饰不命中、不获焦点、不拦事件；现有内部消费/边界冒泡不变；首尾标签和动作可完整露出；原文复制不变，键盘焦点可见 | 生产命中/焦点/scroll-wheel 回归，系统始终显示滚动条手测 | 待实现 / CI / 手测 |
| A6 | 原生/Codex/子 Agent 会话中，真实运行/完成吸顶标题滚过渐隐区与 Composer；打开菜单/应用遮罩 | #403 的标题 < 渐隐/实色前景 < 合法上层顺序保持；遮住消息不误触；底部只绘既有48px过渡 | 保留 issue403 / sticky 真实生产场景；重叠 quad order 和遮挡点击断言 | 待实现 / CI / 手测 |
| A7 | 多行输入、附件、待发送、必要错误/隐私控制增减；贴底或回看时 resize | Composer 动态前景高度/末条安全余量、锚点与回到底部不变；末条可读可操作，不加业务占位消息 | 原有 issue403 / issue98 / message-anchor / follow 回归 | 待实现 / CI / 手测 |
| A8 | TextInput 软换行和换行溢出；滚上下端；键盘编辑、选区与 marked text 位于边缘 | 用实际视觉行计算，不按原文换行数量猜；光标、活动选区/marked text 所在边退让；IME 几何、光标跟随及滚轮路径保留 | TextInput 生产 paint / selection / cursor 几何回归；真实输入法由用户检查 | 待实现 / CI / 手测 |
| A9 | 进程内 vt100 产生无历史/短历史/有界长历史，滚历史首中尾，resize、清屏、alternate screen、输入回尾 | 真实最大历史随快照更新；起止边准确，容量不冒充历史；度量不改变滚动位置或多发命令；活动光标行不淡化 | vt100/TerminalSnapshot 进程内回归 + TerminalView 生产绘制，不启动 PTY/shell | 待实现 / CI / 手测 |
| A10 | 设置页、侧栏、工具详情、选择器、工作区/预览逐项走查；Reduce Motion | 所有实际滚动入口纳入，同一视口只包一次；无溢出不装饰；无循环/入场/计时动画；必要状态仍清楚可恢复 | delivery 全局接入清单、代表性真实页面生产根测试；人工主题与键盘走查 | 待实现 / CI / 手测 |

本卡不改存储与权限，数据迁移、持久化故障注入不适用；恢复后的视口使用同一渲染路径。保留既有业务、安全、选择复制和滚动断言，不通过删测试、ignore、放宽条件或自动重试换绿。不新增真实外部进程/网络/应用 E2E。

## 行为契约

### 1. 普通滚动边界

1. 每一轴独立判断：无实际溢出时不绘边缘；位于开始仅绘结束边；中间两边；位于结束仅绘开始边。浮点位置、惯性或 overscroll 按合法滚动范围 clamp。
2. 靠近任一端时按该端隐藏距离连续减弱；这是空间透明度过渡，不是按时间淡入/淡出。流式节点不增加逐条 opacity 或入场动画。
3. 使用所在视口真实主题表面：主阅读 `bg_base`、侧栏 `bg_sidebar`、浮层 `bg_elevated`、工具/代码详情 `code_bg`，其他表面由调用者传入实际现有颜色。背景不统一写死白色/黑色，不改变文字或 ANSI 颜色数据。
4. 渐隐只覆盖相应滚动内容的边条；固定标题、搜索/操作栏、必要提示、滚动条 gutter 及装饰边框在外或维持清楚的绘制顺序。双轴角落不得叠成两倍强度。不得依赖 GPUI 父级 rounded 自动裁剪子项。
5. 渐隐不创建 hitbox/焦点，不注册滚轮/键盘处理，不 stop_propagation、不加 occlude。已有滚轮消费、嵌套边界冒泡、点击、选区复制与键盘操作不变。首尾文本和动作在对应端点必须完整露出；焦点描边保持可见。活动输入行按 §4 退让。
6. 主题、内容与尺寸变化使用当帧完成布局后的状态；不得仅在 wheel 回调更新，否则程序滚动、筛选、软换行与 resize 会残留。未完成布局或零尺寸时不画，不安排持续重绘任务等待。

### 2. 参数与验算

新增共享 Layout token，建议命名 `SCROLL_EDGE_FADE_COMPACT = 16`、`SCROLL_EDGE_FADE_READING = 24`、`SCROLL_EDGE_FADE_EPSILON = 1`。保留 `CONVERSATION_BOTTOM_FADE_HEIGHT = 48`。前两项遵守4px间距梯度；epsilon 是浮点边界容差，不是间距。

对普通视口的一轴，`V` 为轴长，`D` 为经过 clamp 的实际隐藏距离：

```text
F = min(场景 token, V / 4)
strength = clamp((D - 1) / F, 0, 1)
```

`F <= 0` 或未完成测量时不绘。每条边的边界为所属表面色乘 strength，向内部在 F 内线性过渡到透明。不以计时动画平滑状态。特殊编辑视口可关闭受保护行所在边；不更改用户滚动或选择来腾出渐隐空间。

| 输入 | 验算 | 结果 |
|---|---|---|
| token=16，V>=64，D=0/1/9/17 | clamp((D-1)/16) | 0 / 0 / 0.5 / 1；上下左右同式 |
| token=16，V=32 | F=min(16,32/4)=8；2F=16 | 中间至少保留16逻辑px，不被两边吞满 |
| token=24，V=240 | F=min(24,240/4)=24；V-2F=192 | 中间保留192逻辑px |
| Composer 原有过渡 | 48=4×12 | 固定前景行为保留，不套普通滚动末端强度规则 |

紧凑列表、菜单、标签与横向代码用16；主要页面/文件阅读用24。同一实际滚动视口只接一次。调用者不得任意散落新数值。

### 3. Composer 与虚拟化

1. #403 的 `render_conversation_foreground`、48px渐隐、实色前景、动态高度与底部安全余量保留。主会话只补顶部边缘；底部不追加通用渐隐，不因列表贴底关闭既有前景过渡。
2. 底部安全余量不得伪装业务消息或触发消息水合；末条内容仍能完整进入未遮挡区域。顶部边缘基于列表实际可读区域，不遮锚点轨道、预览、Composer 控件或状态。
3. 两类虚拟列表复用原句柄与测量结果，真实首尾判断优先；禁止按条数乘平均行高、全量实例化或重新测量全部消息。已存在 `is_following_tail`、history cursors、splice/锚点/阅读保护语义不变。
4. StickyDisclosure 保持本地标题 prepaint/paint，不重引全窗口 defer 绘制标题。不将整个渐隐/Composer 提升到全局 deferred，避免盖住应用遮罩。合法菜单保持原 deferred 优先级与裁剪作用域。绘制测试比较实际相交区域的 `order`，不强求无交集 quad 数组全序。

### 4. 自绘输入框与终端

TextInput 使用生产 prepaint 的 `total_visual_rows`、`first_visible_row`、实际 `visible_rows` 和行高求隐藏距离，单行横向裁剪如确有隐藏范围也纳入。不能用原文字节/换行数代替软换行。文字/选区按原规则绘制，光标、marked text 与活动选区所在边相交时临时关闭该边；不为装饰主动重定位光标、修改选区或请求额外跟随。IME bounds 与提交/快捷键行为保持。

TerminalSnapshot 目前仅有当前 `scrollback`，新增实际最大历史的只读投影（例如 `max_scrollback`，实际命名由执行者保持清楚）。定义在既有 `vega_conversation::types::TerminalSnapshot`；UI 不读 parser 或启动额外服务，不用固定缓存容量当已存在历史。

锁定 vt100 的 `set_scrollback` 对实际历史长度 O(1) clamp。可在 worker 的一次 generation 更新、同一锁作用域内保存当前 offset，临时设置到最大值、读取真实上限并恢复 offset，只缓存投影。不能在 UI snapshot/paint 中反复探测、复制全部历史、发送 PTY 命令或额外递增 generation。清屏、resize、alternate screen 与历史淘汰时保持真实结果。

终端隐藏距离按真实行高换算，顶部隐藏为 `max_scrollback - scrollback`，底部隐藏为 `scrollback`；输入回到 scrollback=0 后底部渐隐消失。光标所在边退让，原 PTY/输入、终端复制与会话身份不变。所有新增自动测试在进程内构造 parser/snapshot，不启动 shell 或 PTY。

## 实现计划与文件归属

主 agent 独占本 spec、任务记录、审查、PR/CI 与集成；一个专职实现子 agent 独占本卡生产/测试与 delivery。大范围只表示接入已存在视口，不授权无关重构。执行者先补验收断言，再实现；API 与规格冲突先报告 Codex，不静默缩小范围。

### 共享基础

- 新增 `crates/vega_ui/src/scroll_edge_fade.rs`（可配独立 tests 模块），并在 `lib.rs` 注册；共享 token 放 `crates/vega_theme/src/lib.rs`。
- 使用锁定 `gpui-kit=0.6.1` 的 `base::ScrollbarHandle` 只读方法适配 `ScrollHandle / UniformListScrollHandle / ListState`，自绘视口提供小型内部只读度量。不升级或补丁上游依赖，不新增运行时配置/持久化。
- 局部 Element/装饰器透传原树的布局与 prepaint；完成子树测量后读取状态，paint 内容后绘边缘，滚动条/固定控件按既有层级保持清楚。保留原 id、句柄、列表状态和 min/max/flex 布局约束，不给每个页面另造滚动容器。
- 每个可见视口最多常数条渐变；状态获取不遍历所有消息、不引入每 token 全量渲染/parse、离屏图像、模糊、持续计时器或循环重绘。

### 全局接入清单

执行者在 delivery 列出实际每条调用路径与状态，包含默认、运行、完成、失败、禁用及键盘适用状态。普通嵌套内容和自绘视口都要检查，不能只 rg overflow 后声称全覆盖。

| 区域 | 文件/入口范围 | 规则 |
|---|---|---|
| 主/只读/子 Agent 会话 | `conversation_stream/render.rs`、`conversation_stream/subagents.rs`、`vega/src/window/subagent_panel.rs` | 主消息顶部24，底部原48；其他独立列表按所属视口 |
| 思考与工具详情/分组 | `thinking_body.rs`、`tool_card.rs`、`run_activity.rs`、`tool_activity_group.rs` | 局部背景16；标题外置，保留滚动条与嵌套消费 |
| 侧栏及任务组织 | `sidebar/mod.rs`、`sidebar/threads_block.rs`、`sidebar/threads_block/organization/{menu,render}.rs` | 背景跟随真实表面，独立列表各接一次 |
| 设置与编辑表单 | `settings/{render_impl,provider_management,mcp,skills,updater}.rs`、`automation_editor.rs` | 页面阅读24，小型子列表16，固定提示/动作在外 |
| 选择器及弹层 | `branch_selector.rs`、`file_selector.rs`、`menu_list.rs`、`command_palette.rs`、`conversation_stream/render.rs` 模型列表、`image_preview.rs` 应用菜单 | 仅可滚动行16，保留 deferred/焦点/关闭语义 |
| 待发送/权限相关详情 | `conversation_stream/{pending_messages,privacy}.rs`、`user_question_panel.rs` | 局部滚动16，必要状态/按钮保持可见 |
| Diff/提交/工作区预览 | `diff_view/render.rs`、`commit_panel/panel.rs`、`vega/src/window/workspace.rs` | 阅读24；横向标签16；虚拟句柄不替换 |
| 代码/表格/文件 | `conversation_stream/render_rows.rs`、`thinking_body.rs`、`file_preview.rs`、工作区文件预览 | 横向16/纵向阅读24；双轴去重复角落 |
| 输入框/终端 | `text_input/{element,state,mod}.rs` 按需、`terminal.rs`；`vega_conversation/{terminal.rs,types/terminal.rs}` | 自绘适配16，保护活动输入，终端只读范围投影 |

上述目录中的直接相关状态字段、模块注册、进程内回归及既有 fixture 更新允许修改；发现清单外实际滚动入口须回报 Codex 补充本 spec 后再改。非滚动 truncate/overflow_hidden 不加渐隐。不得添加任何代码注释（仅仓规的 unsafe SAFETY 例外），不改无关旧注释。

### 顺序与交付

1. 共享度量/绘制与 A1–A5 回归，先接思考与工具详情验证嵌套和主题。
2. 普通与虚拟视口逐项接入；主会话顶部与 #403 保全；补真实生产入口回归，记录完整清单。
3. 自绘 TextInput / Terminal，A8–A9；静态审查全局收口与必要 UI 状态，提交实现与 delivery。
4. Codex 审查 spec/diff/验收覆盖后开 PR，云端门禁通过才 squash 合入 master，进入 In review 等用户纯手动验收。

本地仅运行低 CPU 快速静态检查，例如 `git diff --check` 和 `cargo fmt --all -- --check`。不得本地运行 Vega、编译、构建、打包、Clippy、test/nextest 或 benchmark。禁止共享构建缓存脚本或本地启动应用绕过限制。

Actions 使用现有 PR Check：

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
```

本卡新增用例采用 `issue449_` 前缀便于日志定位；完整 gate 继续覆盖 #403、sticky、scroll_follow、选择复制与输入回归，不新建真实 E2E 门禁。CI 启动后每5分钟查询；完成/失败通知或用户即时要求可立即查询。仅最新 head 的 required check 成功才合并。

delivery 路径 `docs/vega-issue-449-scroll-edge-fade-delivery.md`，记录真实文件与接入清单、提交、静态检查退出码、待CI项/云端原始结果、耗时与偏离（应为无）。回滚为回退本卡集成提交，无数据迁移。用户验收前保留 Issue、分支与 worktree，不提前 Done/关闭/清理。

## 用户手测步骤

1. 浅深主题分别检查长/短侧栏、设置、模型/文件/分支菜单与工作区标签；滚起始/中间/末尾，resize 与筛选后无错误渐隐。
2. 展开长思考、工具详情与横向代码/双轴文件，验证内部滚轮和边界交外层、复制、滚动条及首尾内容。
3. Native/Codex/子 Agent 长会话中滚过真实运行/完成吸顶标题，打开 Composer 菜单和应用级遮罩；检查前景层级、多行输入动态高度、末条与回到底部。
4. 长输入软换行、光标与中文输入法在首末可见行，确认输入与候选位置、选区不被淡化；终端有/无历史、滚上下端与输入回尾，核对光标和显示。

变更记录：2026-10-18，v1 根据用户批准的方案建立实现前规格和 A1–A10；当前尚无实现或运行验收结果。

# Composer 模型与权限胶囊按钮

> 本文保留上一轮 v1.1 历史契约。用户手测指出常驻底色错误，当前默认／hover／disabled 及交付要求改按 [v2.0 状态纠正规格](vega-issue-546-capsule-state-correction.md)；旧 GREEN 不代表新验收通过。

## 版本与授权

- 任务：[#546](https://github.com/puzige/vega/issues/546)。规格 v1.1，用户选择「模型选择 + 权限切换」。
- 基线：`83fd51d79940516029cdc1eaf8e81566870c877b`；任务分支：`feat/546-composer-capsules`。
- 用户提供的可见截图仅作胶囊轮廓与低强调表面的参考，不转载原始会话或第三方资产。
- 本地只做静态检查；不运行 Vega、编译、构建、打包或测试。遵守 `AGENTS.md`、`docs/vega-exec-guide.md`、`docs/vega-rust-test-delivery-lifecycle.md`。

## 当前事实与范围

`ConversationStream::render_composer` 在 Native 底栏调用 `render_permission_status` 与 `render_model_controls` → `render_model_selector`。两个文字触发器目前使用 `Layout::TITLEBAR_CONTROL_SIZE`（28）、`Layout::DISCLOSURE_RADIUS` 小圆角、`px_2`，通过 `InteractionExt::interactive` 得到透明静止表面。

上方 utility bar 的项目、分支、后端入口已经使用胶囊外形。`DISCLOSURE_RADIUS` 和 `InteractionExt` 同时被侧栏、菜单、设置页、问答/权限卡等使用，不能全局改动。此次仅改 `composer-model` 与 `composer-permission-status` 的外观，不修改其他消费者。

## 验收（先于实现）

### A. 可见形态

1. Native Composer 的模型选择和权限切换均有常驻、低强调的胶囊表面；四个角完全对称。默认浅色使用 `colors.bg_hover`；深色使用相同语义 token 的深色值，不引入硬编码色值。
2. 高度保持 28 逻辑像素；半径 14；左右内距各 12；最小宽度至少 28。数值验算：`28 / 2 = 14`，`2 × 14 = 28`。因此宽度至少高度时左右为半圆，四角半径都为 14；不依赖父元素圆角裁剪。
3. hover、pressed 和菜单打开使用 `colors.bg_active`；禁用保留静止底色，不产生可操作的 hover/pressed 反馈。所有状态保持高度、半径和内距；内容不变时外观状态不改变边界位置。模型名称与“选择强度”切换继续使用既有预留宽度、保持边界。pending 替换为“保存中…”时保留既有内容驱动宽度，不新增预留或修复算法；每个 disabled 状态进入后，hover/down/up/exit 的边界保持稳定。trusted-action busy 不替换内容，仍须与 enabled 边界相同。
4. 标签、图标、字体、现有文字色不改。完全访问等原有权限警示色继续显示。模型名称/思考标签切换的现有预留宽度、截断规则与 ChevronDown 保留。
5. 不增加说明文案、边框、阴影或动画。沿用现有交互动画和焦点样式。

### B. 行为与布局回归

1. 模型按钮仍按现有模型能力打开 Slider/List；关闭及选择路径、焦点/键盘动作与可访问名称保持。pending、save-busy、trusted-action-busy 禁用语义不变，不能因新样式恢复交互。
2. 权限按钮仍打开权限菜单，保留四种模式、警示图标/文字、危险操作确认与授权事件；pending 等既有 guard 不变，不修改安全策略。权限触发器保留鼠标入口、输入框回焦与 Escape，不新增其本不存在的 Tab/Enter 入口。Full Access 模式切换不新增确认弹窗；危险确认指运行时危险操作仍不能自动批准。
3. 正常 768 宽和窄 480 宽 Composer 内容区均不挤掉加号、权限或发送入口；长模型名仍截断并保留模型切换入口。这里 480 是内容区测试宽度，不是新增窗口最小宽度。
4. 发送/停止、加号、上下文环、Codex 状态、utility bar、菜单项及其他页面外观不变。真实系统焦点/菜单集成与最终视觉交由用户手测，进程内测试不冒充原生验收。

## 先行测试与观察路线

在生产样式未改的基线上写齐本轮测试，经非实现者独立审查后通过现有 draft PR 的 Actions 获取有效 RED，再冻结测试实现。禁止源码字符串断言、只测常量、复制算法、先实现后补测或放宽失败断言。

使用真实 `ConversationStream` 渲染或直接调用真实触发器渲染路径的 GPUI 进程内窗口；`gpui-kit = 0.6.1` 提供 test-support，当前 `gpui-pre = 0.3.4` 的 `Window::painted_quads()` 可观察实际绘制 `bounds`、四角 `corner_radii`、`background.as_solid()`，不需截图渲染器。`VisualTestContext` 支持鼠标/键盘注入；`debug_bounds` 或 Kit 的生产元素观察接线可定位触发器。绘制坐标按 scale_factor 换算，结合 quad/child 的有效 content mask 与 viewport 检查完整可见性；不能把按钮内图标 quad 当成按钮背景。默认 NoopTextSystem 是进程内逻辑几何，不作为原生字体测量。文本观察读取真实 TextLayout，图标 glyph/ink 输入观察只证明生产选择参数，不证明 SVG 像素绘制；真实图标外观按用户手测核对。

冻结矩阵：

- 浅深主题 × 两个真实触发器：默认填充、四角半径与实际高度；应在旧基线因透明表面/小圆角缺失而 RED。
- hover、pressed、open 与适用的 disabled 状态：表面颜色/几何稳定，不改事件结果；鼠标移动而非点击验证 hover。
- 模型鼠标打开、既有 Tab 到达与键盘激活/关闭、Slider 标题进入 List、pending 禁用；权限菜单入口、四种权限标签/警示呈现、打开/选择/Escape 输入框回焦；通过真实完整 stream 根层验证外部点击关闭。模型短名称及“保存中…”保留，按 A.3 的内容边界区分稳定性。
- `vega_runtime::permission::decide_execute_permission` 的最小既有策略回归：Full Access 普通操作可批准、危险操作仍要求确认；只调用纯生产决策 API，不执行危险命令、不 mock 决策、不改生产策略。
- 普通与长模型名、正常/窄内容宽：控件不重叠、不溢出主操作，保留模型截断与原有预留宽度。

实现前测试若发现范围外既有缺陷，记录事实并交主 Agent 判断，不擅自修复，也不写无法在既定生产路径达成的回归断言。测试接线错误与产品行为缺失分别记录。

云端使用现有 `pr-check`：fmt、全目标 Clippy、三组 Python 检查、nextest list/run。有效 RED → 相同冻结测试 GREEN 后在 PR 固定完整 SHA、源码/patch 与原始输出；独立删测提交删除 Rust 测试及独占支持，最终空集完整门禁后转正式 PR、审查合并。CI 每 5 分钟查询，不短轮询。

## 实现计划（仅有效 RED 后）

1. 在 `vega_theme::Layout` 增加语义明确的 Composer 文字触发器几何 token（28 高、14 半径、12 横向内距），不改全局 disclosure/titlebar token。
2. 仅在 `conversation_stream/render.rs` 的两个触发器应用胶囊几何与低强调交互表面。使用现有 `interaction_surface` 指定静止/hover/pressed 色，因为 `interactive` 会将静止背景重设为透明；如提取助手，限定于 Composer，不改全局交互实现。
3. 保留事件、focus、aria、pending/disabled、文字/图标及弹层路径。不得重构选择器业务或修改菜单结构。
4. 更新设计守则 Composer 段落引用本规格。其余使用位置仅在交付记录列出，不改代码。

允许生产写入：`crates/vega_theme/src/lib.rs`、`crates/vega_ui/src/conversation_stream/render.rs`；必要的测试文件、临时 test-support 接线及 `vega_runtime` 独立策略回归测试单列并在 GREEN 后删除。runtime 生产代码禁止修改。主 Agent 负责文档、Git/PR/看板，代码写入由专职子 Agent 单一负责。

## v1 → v1.1 先行测试阶段修订

独立审查确认 v1 的无条件 enabled→pending 等宽要求超出只改样式/保留既有 label_width 的范围。生产外观尚未改、尚未取得 RED/GREEN 时重新明确验收：允许 pending 文字更换沿用原有内容尺寸行为，其他几何、交互、短名、禁用 guard 和模型/思考预留断言保留。不是删测求 GREEN；旧部分 artifact 与审查记录留在交付记录，本轮按 v1.1 重新完成先行测试→独立审查→Actions RED。

原来的宽度变化是静态可能性，不是已测失败；NoopTextSystem 的等宽 BMP 字符下 `alpha` 反而比“保存中…”宽，不能宣称已有复现。补齐原矩阵的危险策略、裁剪可见性、键盘/回焦/外点关闭观察后再冻结，不更改生产行为。

## 手测与回滚

合并后用户核对浅深主题、长模型名/窄窗、hover/按下/展开、键盘模型切换、权限危险确认，以及发送/停止仍正常。不自动重启/安装应用，不宣称已完成原生手测。回滚本卡生产差异即可恢复旧触发器外观，不涉及数据迁移或配置。

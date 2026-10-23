# Composer 胶囊按钮状态纠正

## 版本、来源与基线

- 任务：[#546](https://github.com/puzige/vega/issues/546)，用户原生手测反馈后的第二轮，规格 v2.0。
- 用户指出默认状态被做成了选中态，只有点击或选中时才应出现强调背景。上一轮 v1.1 的“常驻低强调底色”是 Agent 的需求理解错误，不是用户要求；本规格取代 v1.1 的默认、hover 和 disabled 表面规则。旧规格、测试及 RED/GREEN 记录保留为历史，不用旧 GREEN 证明新验收。
- 本轮起始生产基线：`4c0d225daf47e66e559752ee78ba067520bea080`，已含上轮合并 `6615748f91273bfb8ee424011b3b5a09b3777d2b`。分支 `feat/546-capsule-states`，独立 worktree。
- 原始反馈截图不公开转载；只记录经核对的产品事实。本地不运行 Vega、编译、构建、打包或测试。
- 本次用户追加明确要求：“本次不用红绿 和 测试 直接修复”。仅对本轮免除测试编写和 RED/GREEN 流程，不修改仓库通用规则或 CI，不伪造测试通过；采用最小修复、静态检查和独立代码审查。

## 根因与范围

`ConversationStream::render_composer` → `render_permission_status` / `render_model_controls` → `render_model_selector`。`render.rs` 两处 `interaction_surface` 把关闭时的 rest 设为 `colors.bg_hover`，hover 设为 `colors.bg_active`，从而默认和仅悬停时都显示填充。没有真实的选中状态错误，也不是阴影或权限模式改变；责任在上一轮表面状态规格和实现。

本轮只纠正两个 Composer 触发器的背景生命周期。保留专属几何 token、文字／图标／警示色、点击／键盘／焦点／菜单行为、模型宽度预留及全部请求 guard；不改全局 `InteractionExt`，不改 utility bar、发送／停止、加号、菜单项、runtime 权限或其他页面。

## 验收（先于本轮实现）

| 状态／操作 | 可观察结果 |
|---|---|
| 两个按钮默认关闭，指针在外 | 背景透明，不显示灰色胶囊填充、边框或投影 |
| 只将指针移入，尚未按下 | 仍透明；保留既有 cursor／tooltip，不能当成选中态 |
| 可用按钮内鼠标左键按下并保持 | 显示 `colors.bg_active` 胶囊填充；此时菜单仍未由 mouse-up 打开 |
| 按下后拖出／外部松开，菜单未打开 | 不残留按下背景，回到透明；不新增选中状态 |
| 点击完成／既有键盘激活打开 Slider、List 或权限菜单 | 对应触发器保持 `colors.bg_active`；指针移出仍保持 |
| 再次关闭、Escape 或外部点击关闭 | 立即恢复透明，即使指针仍在触发器上；选定模型／权限值本身不等于触发器持续选中 |
| 模型处于 pending／save-busy／trusted-action-busy 且菜单关闭 | 保持透明，不因 hover／按下／松开产生可操作背景；鼠标和键盘不能新增请求 |
| 浅色与深色 | 使用同一语义规则，不硬编码颜色；真实字体、SVG 和系统集成仍由用户手测 |

### 必须保留的回归

1. 高度 28、四角半径 14、左右内距各 12、最小宽度 28；透明时仍保留真实命中边界，填充出现时四角完全对称。各指针／打开状态不改变同内容边界。
2. 标签、图标、字号、已有前景色与权限警示色不变；pending 使用“保存中…”并保留原有内容驱动宽度，不扩展本轮为尺寸修复。模型与“选择强度”仍使用既有预留宽度。
3. 模型继续按能力进入 Slider／List，保留鼠标和 Tab／Enter／Space／上下箭头／Escape、Slider 标题到 List、请求和 acknowledgement、回焦及外部关闭。
4. 权限保留四种模式、请求 guard、鼠标入口、输入框回焦、Escape；不新增独立 Tab／Enter 入口或 FullAccess 模式确认弹窗。普通 FullAccess 批准、危险操作要求确认的 runtime 决策不得修改，本次不运行测试。
5. 768／480 内容宽及长模型名保持可见入口、截断和主操作不重叠。其他控件不改，不新增界面文案、阴影、动画或依赖。

## 本轮验证方式（用户明确豁免测试）

测试准备执行在收到新要求后已取消并确认终止；Agent 产生的未提交测试及独占观察接线移除，不进入交付。它们从未在本地或云端执行，不称 RED/GREEN，也不沿用上一轮测试证明新状态。

本轮只做 `cargo fmt --all -- --check`、`git diff --check` 和静态状态／作用域审查。保留既有 CI，不添加或手动运行测试、不改 workflow 以跳过原有 required check；如 PR 触发已有自动门禁，将其实际结果单独记录，不能冒充本轮行为验证。原生外观与系统集成待用户手测。

## 最小实现计划（直接修复）

1. 仅在两个触发器所在 renderer 中计算当前表面色：关闭为主题色 alpha 0，菜单打开为 `colors.bg_active`；同一个当前表面用于 rest 和 hover，pressed 使用 `colors.bg_active`。可用既有 `Rgba` 更新语法，不引入新公开 API／token／依赖。
2. 沿用 `interaction_surface` 的 enabled gating 与现有按下事件；不改全局交互助手，不增加按下事件或业务状态。
3. 专职写入者仅修改 `render.rs` 两处状态色，并清除本次取消的测试准备。`Cargo.toml`、`thinking_slider.rs` 及独占接线回到本轮基线；生产 runtime／theme／其他 UI 禁止修改。主 Agent 只写文档并负责 Git／PR／看板。

## 交付门禁与手测

直接最小修复 → 格式／差异／作用域静态检查 → 独立审查 → PR 与现有 required check → squash merge。本轮无测试先行、RED/GREEN 或行为测试通过声明，不恢复已取消测试或增加删测门禁。

CI 每 5 分钟查询，完成通知／用户明确即时请求除外。Issue 在 Agent 阶段保持 OPEN／In progress；合并后 In review，等用户确认。不得自动安装／重启应用。用户重点核对默认无底色、仅悬停无底色、按下／菜单打开有胶囊、关闭后透明，以及原有模型／权限／发送停止行为。回滚本轮两处状态色修改即可，无数据迁移。

## 本轮证据（进行中）

专职写入者已直接完成两处状态色修改并停写，已清除全部取消的测试及独占接线。主 Agent 复核的生产差异仅 `render.rs`，格式／差异静态检查退出 0；内容 SHA256 `c7dfb386c211ee604cbc52e566091f61810dc9020eac35cb0441d1cec7659849`。独立代码审查及 PR 门禁结果尚待取得；本轮无测试执行或 RED/GREEN 声明，旧轮 9/9 不作为本轮通过证据。

# Composer 胶囊按钮状态纠正

## 版本、来源与基线

- 任务：[#546](https://github.com/puzige/vega/issues/546)，当前规格 v2.1，修复用户反馈的 hover 缺失。
- 当前验收：默认透明、hover 使用 `bg_hover`、按下或菜单展开使用 `bg_active`。v1.1 的常驻底色和 v2.0 的 hover 透明均为 Agent 对状态边界的错误理解；旧记录保留为历史，不用旧 GREEN 证明当前验收。
- 本轮起始生产基线：`d4a65a381d95b6bb8e4ff8d2858a11a1042ffa3a`，已含上轮修复合并 `a39badd7cb79dd535157c886a485fbbcad22ca6d`。分支 `feat/546-capsule-hover`，独立 worktree。
- 原始反馈截图不公开转载；只记录经核对的产品事实。本地不运行 Vega、编译、构建、打包或测试。
- 延续本次状态纠正的直接修复要求，不新增或手动运行测试。仓库当前快速交付政策也已移除强制 TDD；采用最小修复、静态检查、实现独立审查和既有 required CI，不伪造行为测试通过。

## 根因与范围

`ConversationStream::render_composer` → `render_permission_status` / `render_model_controls` → `render_model_selector`。v2.0 在 `render.rs` 两处把关闭时的 rest 和 hover 都传为 alpha 0 的 `surface`，所以消除了默认底色，也误删了 hover 反馈。现有 `InteractionExt::interactive` 已提供透明默认、`bg_hover` 悬停、`bg_active` 按下／选中和 enabled gating；本轮复用该助手，保留胶囊几何，不改全局交互实现。用户所说“阴影”按实际背景填充处理，不新增投影。

本轮只纠正两个 Composer 触发器的背景生命周期。保留专属几何 token、文字／图标／警示色、点击／键盘／焦点／菜单行为、模型宽度预留及全部请求 guard；不改全局 `InteractionExt`，不改 utility bar、发送／停止、加号、菜单项、runtime 权限或其他页面。

## 验收（先于本轮实现）

| 状态／操作 | 可观察结果 |
|---|---|
| 两个按钮默认关闭，指针在外 | 背景透明，不显示灰色胶囊填充、边框或投影 |
| 可用按钮只将指针移入，尚未按下 | 显示 `colors.bg_hover` 浅胶囊底色；保留 cursor／tooltip，与 `bg_active` 展开态区分 |
| 可用按钮内鼠标左键按下并保持 | 显示 `colors.bg_active` 胶囊填充；此时菜单仍未由 mouse-up 打开 |
| 按下后拖出／外部松开，菜单未打开 | 不残留按下背景，回到透明；不新增选中状态 |
| 点击完成／既有键盘激活打开 Slider、List 或权限菜单 | 对应触发器保持 `colors.bg_active`；指针移出仍保持 |
| 再次关闭、Escape 或外部点击关闭 | 清除展开态；指针仍在可用触发器上则回到 `bg_hover`，指针移出则恢复透明。选定模型／权限值不等于触发器持续选中 |
| 模型处于 pending／save-busy／trusted-action-busy 且菜单关闭 | 保持透明，不因 hover／按下／松开产生可操作背景；鼠标和键盘不能新增请求 |
| 浅色与深色 | 使用同一语义规则，不硬编码颜色；真实字体、SVG 和系统集成仍由用户手测 |

### 必须保留的回归

1. 高度 28、四角半径 14、左右内距各 12、最小宽度 28；透明时仍保留真实命中边界，填充出现时四角完全对称。各指针／打开状态不改变同内容边界。
2. 标签、图标、字号、已有前景色与权限警示色不变；pending 使用“保存中…”并保留原有内容驱动宽度，不扩展本轮为尺寸修复。模型与“选择强度”仍使用既有预留宽度。
3. 模型继续按能力进入 Slider／List，保留鼠标和 Tab／Enter／Space／上下箭头／Escape、Slider 标题到 List、请求和 acknowledgement、回焦及外部关闭。
4. 权限保留四种模式、请求 guard、鼠标入口、输入框回焦、Escape；不新增独立 Tab／Enter 入口或 FullAccess 模式确认弹窗。普通 FullAccess 批准、危险操作要求确认的 runtime 决策不得修改，本次不运行测试。
5. 768／480 内容宽及长模型名保持可见入口、截断和主操作不重叠。其他控件不改，不新增界面文案、阴影、动画或依赖。

## 本轮验证方式（直接修复）

本轮不新增测试或观察接线，不恢复上一轮已取消且从未执行的测试。状态依据现有交互助手的真实实现做静态审查；不将静态审查或历史测试称为本轮行为验证。

本轮只做 `cargo fmt --all -- --check`、`git diff --check` 和静态状态／作用域审查。保留既有 CI，不添加或手动运行测试、不改 workflow 以跳过原有 required check；如 PR 触发已有自动门禁，将其实际结果单独记录，不能冒充本轮行为验证。原生外观与系统集成待用户手测。

## 最小实现计划（直接修复）

1. 删除两个 renderer 内重复的 `surface` 变量；权限触发器恢复 `.interactive(colors, true, self.permission_picker_open)`，模型触发器恢复 `.interactive(colors, enabled, self.model_picker_level.is_open())`。
2. 沿用现有助手的 rest／hover／pressed／selected 色与 enabled gating；保留 120ms hover／Reduce Motion、按下事件和业务状态，不改助手实现。
3. 专职写入者只改 `render.rs`；生产 runtime／theme／其他 UI、Cargo、CI 和测试接线禁止修改。主 Agent 只写文档并负责 Git／PR／看板。

## 交付门禁与手测

直接最小修复 → 格式／差异／作用域静态检查 → 独立审查 → PR 与现有 required check → squash merge。本轮无测试先行、RED/GREEN 或行为测试通过声明，不恢复已取消测试或增加删测门禁。

CI 每 5 分钟查询，完成通知／用户明确即时请求除外。Issue 在 Agent 阶段保持 OPEN／In progress；合并后 In review，等用户确认。不得自动安装／重启应用。用户重点核对默认透明、hover 浅底、按下／菜单打开加深、关闭后按指针位置回到 hover 或透明，以及原有模型／权限／发送停止行为。回滚本轮两处状态色修改即可，无数据迁移。

## 版本记录与当前证据

- v2.0：按 Agent 的错误解释将默认及 hover 都设透明；[PR #568](https://github.com/puzige/vega/pull/568) 合并为 `a39badd7cb79dd535157c886a485fbbcad22ca6d`。任务 head `b2e1db872efce82a4b53b2bf2b31bdfc29e1b9c5`，独立静态审查与既有 CI 成功，未新增或执行本卡行为测试；用户随后指出 hover 缺失。
- v2.1：在生产修改前明确恢复 hover、保留默认透明。专职写入者已完成并停写；生产差异仅 `render.rs` 两个 helper 调用与两个重复变量删除（2 行新增、18 行删除），其余 renderer 内容和生产文件未变。主 Agent 执行格式与差异静态检查，均退出 0；render SHA256 `e15997c81edf6ef0cf0455ec72d4038e590566b8d03ec0ea83f7b2cc5302d2a9`。独立审查与本轮 CI 待取得；最终精确 SHA、run／job 与合并状态在 PR／Issue 记录，不提前声称通过。

# #307 · 移除原生透明外壳与拖拽结束恢复跟随

## 用户反馈与本次裁决

2026-10-23，用户在两段流程合并后提供两张截图：准备页和底部引导外有额外的透明圆角窗口边缘；底部引导拖拽后不再跟随系统设置。用户已明确发生在“底部拖拽引导”“拖拽之后才停止”，并选择“两项都修”，随后要求继续。本次属于原生外观缺陷修复及跟随验收修正，不将上一轮 CI 或自审当作本轮通过证据。私有截图不提交公开仓库。

本规格优先替代[两段权限流程规格](vega-issue-307-permission-flow-parity.md) P6/P11 及历史紧凑/原生规格中“开始拖拽即永久冻结本实例”的条款；其他两段流程、状态真实性、共享检测、真实载荷、原任务和关闭清理边界继续有效。准备总览仍不跟随系统设置，本轮不新增绑定 Vega 主窗口的行为。

基线为已合并提交 `5bc062b245d5f1c7cee3b333df896ef31810fc11`，分支 `feat/307-panel-chrome-drag-follow`，独立 worktree。主 checkout 和旧任务分支的任何改动均不移动、不清理。

## 后续用户裁决（本次实施范围）

用户进一步明确：权限框不可由鼠标手动移动，只有 Vega 应用行可拖动；底部引导继续跟随系统设置。确认当前 Vega 在对应列表中后，应结束引导展示（回总览），不等同于权限已开启。公开授权查询只返回授权布尔值，无法区分未添加和已添加但关闭；用户选择“保留原目标，先修其他两项”。因此本次交付原生外壳、禁止手动移动和既定跟随恢复；列表成员检测及据此自动收起明确待解决，不以松手、拖拽结束或 Granted 冒充列表事实，不新增 TCC 数据库读取、AX/抓屏授权或私有 API。

本次委派再次返回 capacity_exceeded，未启动实现写入者；沿用本卡用户允许当前 Agent 直接实现的方式，仅记录自审，不声称独立审查。

## 原始证据与责任层

- `card/view.rs` 给真实表面四周预留每边 24pt 透明阴影区域，并自行绘制圆角、边框和阴影。
- `WindowOptions` 使用 PopUp / Transparent。锁定 `gpui-pre-macos 0.3.4` 的 `MacWindow::open` 无论 titlebar 是否为 None 都加入 Titled 样式，PopUp 再加入 NonactivatingPanel；隐藏标题并不能移除系统窗口装饰。原 `show_panel` 保留整个 styleMask，未关闭系统阴影。因此外层系统装饰与内层 GPUI 表面叠加；这是已核实的配置遗漏，最终像素仍须用户验证。
- `CardController::begin_drag` 设置 following=false，`drag_ended` 为空，卡片未接拖拽结束恢复。因此用户所述拖后停止符合旧实现，但已不符合本次明确要求。
- 锁定 GPUI 的 `App::has_active_drag` 在外部拖拽移交平台时变为 false，不能据此判断结束。`AnyDrag` 持有拖拽预览视图；平台移交把整个 AnyDrag 移入 `PlatformOwnedDragState::Suspended`，回入源窗口再恢复，原生 `draggingSession:endedAtPoint:operation:` 发出 `FileDropEvent::Ended` 后清除。源窗口提交也会在鼠标释放时消费 active_drag。可使用仅由本次预览持有的生命周期令牌，在现有轮询中确认其释放后恢复；不使用光标离窗、假定延时、无 active_drag 或 drop 接收结果来冒充结束/授权成功。

## 验收用例

| ID | 前置状态 / 操作 | 必须观察到的结果 / 验证层 |
|---|---|---|
| F1 | 打开准备页或任一权限的底部引导，Light/Dark | 只有一层实际卡片圆角/边框/阴影；24pt 区域不出现额外原生边框、玻璃外壳或原生阴影。代码/配置审查 + 用户手测 |
| F2 | 打开隐藏的原生面板 | 只对精确且唯一的自有 NSPanel 配置无边框、非激活、透明背景、无系统阴影；显示前读回验证，失败关闭并走原恢复。保留 floating level 3、跨应用行为、失活不隐藏、worksWhenModal=false 和一次非激活显示；不得操作其他窗口或将 Vega 主窗激活。静态 ABI/调用链 + CI + 用户手测 |
| F3 | 尚未拖拽，移动/隐藏/恢复系统设置、跨屏 | 引导保留既有 150ms 更新、1pt 门槛、18pt 表面内缩和可见区 clamp。准备页不随系统设置移动。静态几何不变 + 用户手测 |
| F4 | 从真实应用行拖拽，在源窗口、离窗到系统、回入源窗口 | 当前有效引导开始拖拽即暂停跟随；原生移交不会因普通 active_drag 清空而提前恢复；同一手势尚有效时不移动引导。当前进程真实 .app directory=true 载荷、路径复查、阶段身份校验保留。静态生命周期审查 + 用户手测 |
| F5 | 结束投递、取消外部拖拽、源窗口内释放、重复拖拽 | 仅本次有效拖拽生命周期释放后恢复当前引导的跟随；在下一可用跟随更新重新定位设置。无须关闭/重开卡片；重复手势各自暂停/恢复。结束/取消只影响跟随，不推断投递成功、权限状态、抓屏成功或任务完成。静态调用链 + 用户手测 |
| F6 | 拖拽期间切换阶段、返回后再进入、关闭重开、原任务失效 | 生命周期令牌不跨实例；旧拖拽释放不得改变新实例或伪造权限。阶段切换不能绕过仍有效的拖拽暂停；结束时若仍在 Overview 则不启动设置跟随；关闭仍终止更新/释放 owner。原任务、阶段、路径和取消围栏不变。静态所有权审查 + 用户手测 |
| F7 | 检测失败、几何/移动失败、等待设置窗口、resize 未完成 | 不因拖拽结束清除真实故障、绕过尺寸确认或错误恢复；返回/关闭/Finder/诊断仍可访问；仍不修改权限/driver/签名或触发 prompt/capture。静态错误路径 + 用户手测 |
| F8 | 键盘与两段入口回归 | 普通准备不打开设置；显式动作打开对应精确 URL；返回仅回总览；Tab/Shift-Tab/Enter/Space/Escape 和可见焦点不退化。静态 UI 差异审查 + 用户手测 |

## 最小实施计划

1. `native/macos.rs::show_panel` 在隐藏、已核实的自有 NSPanel 上移除 Titled/Closable/Minimizable/Resizable/FullSizeContentView 装饰位，保留创建时已有的 NonactivatingPanel 位，关闭系统阴影及 movable/movableByWindowBackground 并读回确认，保持非 opaque / 清晰透明背景。复用现有 Objective-C ABI helper，必要的对象 setter 只用于自有面板和 NSColor clearColor；不新增依赖、不改 GPUI 注册表源码。显示前读回并核实相关状态。
2. `controller.rs::drag_ended` 在 open 时允许跟随恢复，closed 时保持关闭。`card.rs` 与 `card/view.rs` 用一次手势专属弱生命周期跟踪预览持有的强令牌，在既有 150ms tick 中发现释放后结束暂停；不为此新建 daemon、自动探测链或公共跨层类型。恢复前核对当前阶段/placement_pending/panel_fault/tracking，不覆盖真实错误。
3. 保留 view 表面、阴影余量和纯几何算法，不靠把 padding 改为0掩盖原生外壳。准备表面600×440、外框648×488；引导表面560×120、外框608×168（均为表面各边加24：宽/高+48）。设置右/下缘表面内缩18的算术沿用原算法，本轮不改数值和主题 token。

允许产品写入仅 `crates/vega_ui/src/cua_onboarding/{card.rs,card/view.rs,controller.rs,native/macos.rs}`；确需其他路径先报告证据。主 Agent 维护本规格和旧规格的替代链接，独占 Git/index/commit/push。单一实现写入者，默认委派实现并独立审查；上一轮直接实现/自审的例外不自动标成本轮独立审查。

非目标：重做两段布局、准备总览跟随 Vega 主窗、权限/driver/签名/任务逻辑、配置/SQLite、依赖/CI/打包、#542、系统权限操作、应用安装/启动。不得新增生产注释（unsafe相邻SAFETY除外）、unwrap/expect、硬编码颜色/字号。

## 验证与交付

- 本地只做低 CPU 格式、diff、静态调用链/ABI/生命周期/保护范围和无 Rust 测试/独占支持清点；不运行 Vega、编译、Clippy、Rust/Python 测试、打包、安装/启动、CUA 或系统权限操作。
- 按本次风险选用静态审查、现有完整 required Actions 和用户手测；不恢复已清除 Rust 测试或为流程临时造测试。现有 fmt、全目标 warnings-denied Clippy、三组 Python、Rust build/discovery 与最终 no Rust tests（0发现/0执行）均须基于最新完整 head，空集不称行为测试通过。
- 用户真实桌面视觉、原生拖拽生命周期/跟随、跨显示器、管理员对话框和焦点验收不由静态/CI代替。最新 head 审查与 required CI 无阻断后 squash merge，Issue OPEN / In review 等用户手测；不因合并关闭 Issue 或置 Done。
- 回滚为单独 revert 本轮 squash，不回滚已合并的两段布局或 #582 原生显示修正。

## 状态

规格 v1.1：已直接实现并自审四份产品源码：精确自有隐藏 NSPanel 移除装饰，关闭原生阴影、opaque 与两种手动移动，设置 clearColor；保留非激活位及原层级/collection/失活/模态配置并读回。DragPreview 独占手势强令牌，卡片仅存弱令牌，既有 tick 确认释放后恢复 open 控制器的 following，保持阶段/尺寸/故障守卫；不触发权限检测或完成任务。当前源窗口回入后 GPUI 消费预览的时序仍须用户真实拖拽验证，不能把源码生命周期审查称作系统验收。

实际本地 `cargo fmt --all -- --check`、`git diff --check`、`cargo metadata --no-deps --offline --locked --format-version 1` 成功；Rust 测试属性/cfg(test)/test-support/dev-dependency 扫描无匹配。未修改依赖、CI、脚本、runtime、共享检测或权限判定；无本地运行、编译或测试。委派受容量限制，本轮仅自审，不声称独立审查。最新完整 head 的 required CI 待结果；原生外观、不可手动移动、拖拽/取消/跟随、跨屏和焦点仍待用户手测。列表成员检测与自动收起保留为未完成目标，不纳入本次已实现声明。

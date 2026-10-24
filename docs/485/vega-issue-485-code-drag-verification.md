# 代码拖选自动横向滚动验证记录

关联 [#485](https://github.com/puzige/vega/issues/485)、[PR #511](https://github.com/puzige/vega/pull/511)、[独立周期规格](vega-issue-485-code-drag-autoscroll.md)。此记录只针对用户追加的代码拖选交互，不改写[引用折行周期](vega-issue-485-verification.md)结论。

## 接手验证记录（2026-10-24，当前阶段）

本节覆盖下文历史阶段状态，不改写旧证据。当前采用[规格接手修订](vega-issue-485-code-drag-autoscroll.md#接手修订2026-10-24当前有效)的快速交付与分层验证，保留 D1–D9。

- 接手前最后一次 [run 39409511774](https://github.com/puzige/vega/actions/runs/39409511774)，head `4aa28ac1dcfc9401e7caea6c69205e5cb732e537`，实际停在测试编译：unused `gpui_kit as gpui` 与缺失 `Half` trait。未取得行为执行结果，不作为有效行为 RED。
- 合入 master `a100f7307c73e51fefcc56012c117b7b832e292e` 的 merge 为 `c3880c66dddf828da3864121797f34fe37c20c11`；接手规格为 `6abbca2dbacc261b069529daa2265a15343098d2`。旧 Quote 特判冲突按新生产结构处理，保留自然折行。
- 初版 `10bd8f127b736b7c4100280061c292bdcc7c76d0` 独立静态审查 NO-GO：上述测试编译错误、Thinking 块序号复用后持久 controller 未重绑 handle、未排除 active drag。没有推送该版运行 CI；修正后再验证。
- 修正版 `bb73a0bbd12421b5ae5a4d1b9d089b59e42ff45e`，合入 master `4db4992df27e9b54e62927209a7fcde420b601f6`。独立静态实现复审 GO，无 P1/P2，不代表行为通过。
- [run 40085512761](https://github.com/puzige/vega/actions/runs/40085512761)：Format 成功；Python 分组 5 / 15 / 40 项成功；Nextest 21 发现 / 21 执行 / 20 通过 / 1 失败 / 0 跳过。失败为 `thinking_code_vertical_edges_and_corners_use_the_inner_y_viewport`，corner=false/true 的原始逻辑选区扩展断言失败，Y 推进、X 隔离与外层不动未报告失败。Clippy 因 `code_drag.rs` 的 `collapsible_if` 失败，完整 required check 失败，不能合并。
- 本轮失败分类：Thinking 纵向拖选刷新接线为产品/集成缺口；Clippy 为代码结构问题。源码表明内部 Y 消费 wheel 后停止传播，而原生选择仅在 bubble 刷新 cursor；新几何不会自动触发非 self-scroll 参与者刷新。修复只补新布局后的正常选择刷新，不新增 Y 驱动；原始拖选点与原生 wheel 的裁剪命中点分开记录，避免纵向 corner 误横滚。保留全部失败断言和其他回归，没有调整 expected-prefix。
- 修复完整 head `fb8415e4d88840164798486cd550f92cf8a5f68a` 的独立静态 delta 复审 GO，无 P1/P2。[run 40086661542](https://github.com/puzige/vega/actions/runs/40086661542) / [Nextest job 120850893747](https://github.com/puzige/vega/actions/runs/40086661542/job/120850893747)：21 发现 / 21 执行 / 21 通过 / 0 失败 / 0 跳过；Format、全目标 Clippy、Python 5 / 15 / 40 项与 required check 全部成功。
- [原始 19 项事件测试源码](https://github.com/puzige/vega/blob/fb8415e4d88840164798486cd550f92cf8a5f68a/crates/vega_ui/src/conversation_stream/code_drag_tests.rs)，SHA-256 `5a0ce201542abcdc7919363622c136ce358fd9f5717f29f1c7d0ad21078da774`，GitHub API 已读回 blob `b40601ae6fde0c5126b50b5726fab169a22b503c`；[2 项 epoch 单元测试与 pending-refresh 分层接线](https://github.com/puzige/vega/blob/fb8415e4d88840164798486cd550f92cf8a5f68a/crates/vega_ui/src/conversation_stream/code_drag.rs)，整文件 SHA-256 `547bd111f476a5e688f6f8d2980e42bfcb7c741588e640bd3236f1a72e4fab41`。fixture/Cargo/所有独占接线均可在同一完整提交恢复。[PR 验证留痕](https://github.com/puzige/vega/pull/511#issuecomment-6383251863)。
- pending-refresh 用例执行生产 controller/scheduler 的过期回调决策，与实际事件停止回归分层验证；不宣称已在同步 GPUI flush 内精确注入真实 MouseUp，也不冒充真实桌面验收。
- 本地仅 rustfmt / diff / 静态检查；没有运行、编译、构建或测试 Vega，没有 CUA、安装、重启和运行数据修改。原始历史 `.patch` 的上下文空行保留原始补丁格式，不为全历史 diff whitespace 提示改写证据；本轮实现增量的 `git diff --check` 通过。

清理提交为 `11e79b54f94f270b5f8c47f6be29ce71987fa18e`：删除 19 项事件测试、2 项 epoch 测试、测试 helper/观测/模块接线和 test-support dev dependency；Cargo manifests 与锁文件逐字节恢复为生产 master `4db4992df27e9b54e62927209a7fcde420b601f6`，没有依赖升级。静态清点无 Rust 测试或独占支持残留；455 个 Rust 文件的固定 Rust 1.98.0 rustfmt 检查与本轮 diff 检查通过。

清理生产投影：从已验证 `fb8415e4` 排除独占测试文件，按语法单元移除 11 处 `cfg(test)`，对 7 个受影响文件作同版 rustfmt 规范化，其余按字节比较。455 个生产 Rust 路径的 path-NUL-byte_length-NUL-content 聚合 SHA-256 两侧均为 `91306281f9d0985e0664efb3e3a1793d3f74d662360d8454d9fe3d0ea5c68e79`。最终完整 head 还须独立复核与 required CI 确认 `no Rust tests`（0 发现 / 0 执行）；空集不是开发测试通过。当前尚未合并，用户手测未完成。

## 历史起点与证据边界

- 起始生产基线：`99e270b13c0f4def33492c432ba25bd81244c2a3`；测试冻结前同步后的未修复生产基线：`db4e27851bdf83d2e49710d0f8707998f9c6b089`。
- 远端 master 起始为 `3a2ef8e9795d609f022b7b81011360db102e3de6`；再次 fetch 后为 `9baca7519d97995267de31eb9c2e213f0c171e67`（#512 侧栏排序）。保护测试草稿后同步上游并合并原已发布分支，保留引用周期全部原始提交为祖先，不强推；UI、Cargo 和工作流生产文件与起始基线一致。原主仓库用户未提交文件保留。
- 分支：`fix/485-markdown-quote-wrap`。PR 在新周期开始时恢复 draft。
- 用户明确按住左键拖选文字到左右边缘，非悬停或普通横向滚轮；明确禁止 CUA。
- 引用正文应折行、围栏代码长行不软折行，是不同契约。本轮保留已通过云端验证的引用容器改动。
- 先前代码保护只看布局/完整内容，没有验证拖选或 scroll offset，本轮不沿用旧 GREEN 作为交互通过证据。

## 必需观测

回归经过生产 Markdown/Thinking 渲染、真实 GPUI 测试平台鼠标/滚轮事件、分步时钟与重复绘制。只读观察真实滚动句柄和文字布局；不得直接赋 offset 或 selected_text。验证至少两个无新指针移动的时间步持续滚动、左右方向/有界范围、起始字符锚点和精确 UTF-8 剪贴板，以及停止、隔离、失活/取消和原纵向选择行为。

GPUI 无界面合成字体与平台事件用于进程内回归；不是 CUA、原生鼠标/字体/系统集成验收。原生手测由用户完成；不得声称应用已安装或问题已经在用户环境验证。

## 阶段

| 阶段 | 证据 | 当前结果 |
|---|---|---|
| 验收与方案 | 独立规格 D1–D9；专职实现方与独立架构审查 | 已记录；生产滚动未改 |
| 测试先行 | 当前草稿源码 SHA-256 `cba06fd039ec907965a1fa8bfdc446de821fa4f95ad7ff79f437f2a791b4706e` | 14 项事件测试草稿完成；纵向 handoff、双窗口、非左键新增项获独立静态审查认可。待重绘松键仍存在时序接入缺口，尚未冻结或执行 |
| 有效 RED | 待现有 Actions run/job/原始失败 | 未取得；接线、依赖、编译失败不算行为 RED |
| 最小实现 | 仅有效 RED 后委派 | 尚未开始 |
| 同一冻结测试 GREEN | 待实现 head 与完整 gate | 未取得 |
| 独立删测 | GREEN 与恢复证据固定后 | 尚未执行 |
| 最终 latest head 空集 gate | fmt / Clippy / Python / 发现构建；0 发现/0 执行 | 未取得；不把空集称测试通过 |

## 冻结前审查与时序缺口

初稿使用真实生产正文与展开 Thinking、正常 down/move/up/wheel、分步时钟和正常 `CopySelectedText` 剪贴板入口；只读钩子均由 `cfg(test)` 限定。审查要求补齐已列入规格的三项边界，生产未修改时已增加以下四个测试：

- 正文、Thinking 各一个多行代码纵向边缘与角落拖选，检查连续两步 Y 推进、选区扩展和代码 X 不被误消耗；Thinking 读取其真实内部 ScrollHandle，并要求外层不动。
- 两条路径的非左键完整手势，检查不横滚、不启动选择。
- 两个实际 Thinking 窗口使用相同代码块 ID；观测按 `WindowId` 保存，不重置其他窗口记录，检查只有来源窗口移动。

上述新增项获得独立静态 GO，不是整个补丁已冻结或云端通过。主会话另行执行 `git diff --check`、固定 Rust 1.98.0 对变更源文件的 `rustfmt --check`、`cargo metadata --offline --locked --no-deps --format-version 1`，均退出码 0。锁文件仅新增既有 GPUI 测试支持的 10 个传递包，既有包身份和 checksum 不变；引用渲染源文件与上一周期相同。没有本地编译、测试或应用运行。

剩余要求是 **真实 timer 改变 offset 后、其 post-layout held-cursor refresh 前松键**，不能以完全 settle 后的普通松键冒充。实现方核对锁定 `gpui-pre 0.3.4` 后报告：`app.rs::finish_update`（1089–1095）同步调用 `flush_effects`；后者处理 defer（1700–1702），并在 test-support 下自动绘制 dirty window（1712–1724），继续处理绘制生成的 defer。外部 `executor.tick()` 只推进完整任务，无法在一次同步 update 内插入输入；`simulate_next_frame` 只控制 next-frame callbacks。主会话已直接读回这些源码位置；独立可行性复核也未建立满足冻结要求的公开 API 序列。

独立复核补充：doc-hidden 但 public 的 `TestAppContext.app.borrow_mut()` 可用 `App::defer` 提前排队真实 MouseUp；这不等于不存在公开排队入口。但是其契约仅是下次 effect flush 执行，不能保证排在真实 timer 的 offset 修改之后、旧 cursor refresh 之前，也不能证明旧刷新此时确实待执行。依赖未来实现恰好如何 notify 或安排 update 才通过，不能称为与实现独立的先行测试。

`[BLOCKED] #485 | 冻结前缺少可控制的 pending-refresh 时序边界 | 已排除普通 tick、强制 render、next-frame callback 与无保证的提前 defer | 交回 Codex 决定最小私有测试调度接入点，或修订此竞态的分层验证方案。`

建议优先决策私有、仅测试可控制的调度适配：真实 timer 写真实 offset 后暂挂真实刷新队列，派发真实 MouseUp，再放行旧刷新；不新增公共生产 API、不改依赖或框架，不直写选区。该方案超出当前只读观测接线，**仅为建议，未获批准、未实施**。另一选择是明确允许私有 controller/scheduler 级过期回调测试与事件级拖选集成验证分层，不声称事件测试强制了这个精确交错。保持既定 D1–D9，不自行删除竞态验收。

当前停止在测试冻结前，14 项为可恢复草稿而非最终冻结测试；未取得本轮行为 RED，未授权/编写生产横向驱动，尚未批准改变框架、引入调度接入点或放宽验收。下一执行者 Codex，当前执行者待领取，Issue 保持 In progress，PR 保持 draft。

## 限制与回滚

本地不运行 Vega、不编译/构建/打包、不执行 check/Clippy/test/nextest；只做快速格式、差异、元数据和静态核对。测试只交现有 PR Actions，状态不短于 5 分钟查询，除收到完成通知/用户即时请求。没有 CUA。

只回滚本轮横向拖选机制，不动引用折行或用户数据。Issue 保持开放；最终实现与云端验证完成后交回审查，手测通过前不关闭或 Done。

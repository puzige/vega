# #552 首次实现云端结果与修复边界

## 精确版本及原始结果

- 实现与开发测试 head：`77c5c72210d3a300ffc8a111740ac417a39d3ba7`；[Actions 39896652404](https://github.com/puzige/vega/actions/runs/39896652404)，completed / failure。
- Nextest job `120228552010`：`Starting 22 tests across 15 binaries`；`22 tests run: 4 passed, 18 failed, 0 skipped`。这一轮已执行行为，不能沿用首轮宏递归时的“行为未执行”。完整 failed-log SHA-256 `f0e9c550ad99af32d3d8cbaf0294af9f29270455e85705cbaf4755230f4c0dcd`。
- Clippy job `120228552187`：唯一根诊断为 `motion_tests.rs:3` 的 unused import `gpui_kit as gpui`。父模块通配已带入 Kit 的 `pub use ::gpui`；删除冗余别名，不用 allow 或提高递归上限。builtin test 显式消歧保留。
- required check `120230060537` failure。未合并、未用户验收，不能删测或用空集门禁消解失败。

## 2026-10-23 续接状态

旧子执行均不在当前运行目录中，源码停写；所有产物保留。父 fetch 后正常合入 master `6e490aefc18fd0e8a263cbcd6b67570bcbf9348e`，本卡集成 head `5e86eb27d251b3d482c125d77b3687a38de6a6ed`。新增基线为独立权限引导/settings 改动，Workspace 和既有测试源码未改；不授权使用 CUA。主 worktree 用户改动不碰。

## 中断恢复检查点

2026-10-23 本轮重新核对旧执行 `01M5KQB2XW5VXQS2R464VZ51BM/01M5KQB2XWMZXA4GZK0X1J8PPD` 已 interrupted；工作树没有保存任何源码修复，仅本修复说明未跟踪。保留的原始 failed-log 与 Clippy-log SHA-256 分别与上文及 `a687b43ef3c28788f8f499555f845f62c40a65f1af63a1cedc79ded3618cb54d` 一致。

已 fetch 并正常 merge 最新 master `3277ebb5fd710d7d33080f91b6c5235e1558e0d3`（#344 真实 post-layout 锚点与预览关闭修复）；本卡新 HEAD `baa12c641917b90888fa56b82d2f0af63d99deac`。保留原测试和支持，不从旧 ref 覆盖 core/message_anchors。下一 writer 先保存已明确的帧观测、active 和实际可见控件命中修复，再单独核对 activation 无中间 draw、Diff 和 Terminal；不重复宽泛调查或把叙述当已保存代码。

## 原始诊断与下一验证

W1–W12、120ms 默认 cubic、即时导航/语义/焦点、背景唯一插值、输入/滚动/实体隔离均不减少。

| 失败组 | 原始证据 | 归因及最小下一动作 |
|---|---|---|
| 中间帧/恢复/帧阳性对照 | old fill 0 vs 1；快速切换不在 (0,1)；无续帧阳性对照 | 尚不能归为产品缺陷。fixture 初始化未显式激活窗口，而共享 gate 按真实 active 停动效；先核锁定平台初态并建立真实 active 前置，保持原数值断言 |
| paint 记录 | file/text/Body 同一记录为 2；Diff loading/file row 为 0 | 测试观测/挂载前置待查。读取框架 draw/update 与当前 owner/clip，不改 len==1 为任取/放宽，不造记录。将日志明确限定于同一完成绘制帧；Diff 必须真正渲染 selected view |
| terminal 输入 | live 组合 expected [[13]]，actual [] | 核对真实焦点/active/selected/事件投递和 poll 前置，不给 recorder 注入输入或替代生产 handler |
| terminal close | driver.closed false；weak release 未到断言 | 核对关闭命中和 render arena/旧帧实体引用释放，不把立即 cancel 当 drop，不改生产生命周期来迁就 fixture |
| 二次析构 panic | 初始测试断言失败后 window not found/SIGABRT | 次生清理故障，与根 paint 断言分开；修测试 teardown，不吞根错误 |

唯一修复 writer 可改 Workspace 窄实现、现有测试及其默认关闭的 UI/conversation 观察支持、必要 harness 前置；不得并行改共享 theme/interaction、其他任务或导航/Final/权限算法。若确认需共享实现修正，先报告精确证据供父串行分配。所有行为断言和 22 个回归保留；API/前置/观测错误修责任层并记录理由。停写后固定内容，经非写入者审查及现有 PR #566 Actions；本地仅快速静态/格式检查，不运行或编译 Vega。

## 已保存修复与未决（父实际复核）

执行 `01M5KSGM3G87T90YG5DS8S7T5Y/01M5KSGM3G0ZYVH0EDB7J8FDBB` 因时限 interrupted，已停止写入，非 READY。父已读回实际产物：`motion_tests.rs` 与实现续接文档合计 210+/28-；修复前 HEAD 仍为 `baa12c641917b90888fa56b82d2f0af63d99deac`，tracked diff SHA-256 `832f9b91d0888f71fb5b522e799348faaf7c69f677cd68bd8c44854935dcacac`（不含本修复记录）。测试源 SHA-256 `c2ec68f1673e265a3e3fe2018cffd26dd843c36cbf877d9b2f8c13b1039094eb`。

- `FrameObserver` 透明委托真实 root 的 layout/prepaint/paint，每次实际 root paint 清空 PaintLog/PaintedBodies；SelectionSamples 在真实 root render 的采样前重置，避免跨两帧累加而不放宽单帧唯一性。
- fixture 显式 activate、drain 并断言真实 active 前置。mouse helper 先通过既有 tab_scroll 揭示目标，再用实际 painted label / close child 的 clip 交集命中；之后验证真实 selected/focus/删除，不以直接 open 代替被测事件。
- Diff 维持真实 `begin_refresh(true)` 并断言无 snapshot；Terminal/Diff/file 的最后外部引用放在实际 App update 内释放，触发正常 GPUI effect cleanup，保留 closed/weak-release 断言。teardown 仅已在 unwind 时跳过以免次生析构 abort，不吞原始失败，正常路径仍删除 fixture window。
- 删除实际 Clippy 证明冗余的 gpui 别名及已接入 sampler 的过期 allow；修正文档中的本机绝对路径。Workspace 生产动效和共享源未改，22 个测试定义仍保留。
- 父重新执行 `cargo fmt --all -- --check` 与 `git diff --check`，均 exit 0。仅格式/静态结果，未编译/执行 Vega；原行为失败尚未被新 Actions 消解。

### 尚不能关闭的无中间 draw 前提

`inactivity_without_a_draw_then_quick_restore_adopts_latest_target` 的 render-count 严格断言保留。锁定 GPUI `app/test_context.rs:932–937` 的 deactivate 会 run_until_parked；`window.rs:1862–1878` active callback 更新 active、通知 observer 并 refresh；`app.rs:1710–1723` 测试 effect flush 自动画 dirty windows。因此当前 helper 很可能在断言前已有真实 draw，不能把它当产品恢复失败或将断言删除换绿。

父定点检查 public API：`TestAppContext::skip_drawing`（:151–154）仅支持整个测试永久跳过 root，无公开恢复开关；不能代替“前后同一 sampler、中间不画”的完整回归。下一步固定此保存版本，取得独立静态审查和新一轮同 PR Actions，分别确认其余故障是否修复及该前提的真实结果。若仍受此 API 限制，需可保持原 sampler 身份的公开 fixture/框架能力；不改 registry、升级依赖、模拟证明、删回归或声称通过。该问题解决前不进入删测/合并。

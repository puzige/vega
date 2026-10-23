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

## 修复版本的实际云端结果与下一范围

`0b8acd725c09c6f82c1afb78be1ebd25913afee2` 的 [Actions 39920395850](https://github.com/puzige/vega/actions/runs/39920395850) 完成 failure：Clippy `120303963083` success；Nextest `120303962810` 实际 `22 tests run: 18 passed, 4 failed, 0 skipped`；required `120305101923` failure。failed-log SHA-256 `df27404089d9f041db01b3f9dc318431218afd0e122434d604fd5a765660638b`。本轮 Terminal 输入/释放、单帧 paint、120ms 中间采样等已真实执行通过，不再沿用旧 18 败归因。

非写入者对该精确 head/base `3277ebb5fd710d7d33080f91b6c5235e1558e0d3` 的静态审查未发现新增 actionable 产品/支持/repair 问题；仍明确指出 no-draw 验证阻塞，不是运行通过许可。父随后正常 merge master `b032d0d2ab9ee057137f3e5ad2c0e517011ea1ae`，当前任务 HEAD `acb20d09902bf0aa408565e1364eceeff195b995`，保留 #575。

| 剩余根断言 | 实际结果与源码依据 | 下一最小动作 |
|---|---|---|
| no inactive draw | RenderCount 14 vs 12；公开 activation 自动 draw，与上述静态链一致 | 原断言保留；阻塞不靠删测消解，本轮不改框架/共享源 |
| file scroll positive control | `:1415` offset 未负；`scroll_at` 未先移动真实 cursor，GPUI Div scroll handler 依据 hitbox.should_handle_scroll 而非只凭 event.position | 真实 mouse move 到 body 的已观测可见 clip 内，再发原 wheel，保留 offset/阅读/隔离断言 |
| keyboard focus border | `:814` 无 accent border；测试在 keyboard shift-tab 后调用 pointer_out，MouseMove 改回 mouse modality，而 focus_visible 要求 last_input_was_keyboard | 将 pointer-out 安排在键盘操作前；之后只 frame 并断言真实 keyboard modality/focus，保留原 ring 与几何断言 |
| live stream restore paint | full text 实际 paint 0 vs 1；测试把 ResumeTail 发给仍 focused 的 Composer，但真实 action handler 仅在 scroll-to-bottom 按钮 scope | 用真实已挂载 resume-tail 控件的 focus/鼠标入口触发原 handler；可加默认关闭只读 focus/bounds 观察，不直调私有 resume_tail 或删除精确 text paint/anchor 断言 |

只授权上述三类 fixture/event 接线最小修复及记录，所有 22 项原行为断言保留，no-draw 限制仍阻塞清理/合并。若真实输入前提建立后仍失败，单独报告产品或观测故障，不改无关导航/流式/list 算法。

## 三项事件夹具修复保存（待云端验证）

- 基于 HEAD `acb20d09902bf0aa408565e1364eceeff195b995`，本 writer 仅修改 `crates/vega/src/window/workspace/motion_tests.rs`、`crates/vega_ui/src/conversation_stream/core.rs` 并追加本节；上节父诊断保留。未修改 Git/index、依赖、共享 motion 或生产业务逻辑，未启动 PR/CI。
- 滚轮：`scroll_at` 在原 wheel 前真实 mouse move 到同一位置；`body_position` 要求实际 paint bounds 与 clip 交集宽高为正。文件阳性对照先断言真实 selected、focus 和唯一已绘制 body；原 120 行长内容、负 offset、阅读内容恢复及多实体输入隔离断言均保留，没有直接设置 scroll offset。
- 焦点环：两处 pointer-out 均在键盘导航前，Shift-Tab 后仅 frame；每次读取 ring 都断言真实 keyboard modality 和目标 tab focus。原非空 accent quads、各帧 ring/几何/文字颜色断言不变，另核对关闭按钮颜色在 0/30/60/120ms 不变；不改生产 focus-visible。
- 恢复跟尾：新增仅 `test-support` feature 下的只读 `observed_resume_tail_focus`，只 clone 现有按钮 focus handle。fixture 正常 frame 后确认尚未 following tail，focus 真实按钮并确认焦点，再 dispatch 原 `ResumeTail`；新增 action 后即时 following-tail 断言以区分路由失败与后续 paint 失败。精确完整正文恰好一次实际 paint、最终 following tail、阅读 anchor、Composer 草稿、Terminal 输入与同 entity 断言均保留，未直调私有方法或写 follow mode。
- 静态保留核对 exit 0：HEAD 与工作树测试名称/顺序相同，仍为原 22 项；无删除 assertion 行、无 ignore；`inactivity_without_a_draw_then_quick_restore_adopts_latest_target` 函数与 HEAD 逐字节一致。代码双文件 `git diff --binary -- <上述两条源码路径>` SHA-256：`edb55518b47396bb06c388dd05cf9265d687e721c22482855ba0dc5ec96d42ec`。
- 本轮未本地编译/运行 Vega、check、Clippy、测试、bench 或 CUA。上述三项是否修复实际失败仍待父 Agent 独立审查与新 Actions；没有新执行/通过计数。no-draw 的 RenderCount 14 vs 12 仍是已知 API BLOCKER，严格测试保留，未再调查框架，仍阻塞删测/合并。

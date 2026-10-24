# #549 共享动效支持续接

## 当前规则与范围

- 本次恢复基线及停写 HEAD：`a9c20372f6dec83e96588edb608195e0adcabda8`，分支 `feat/549-overlay-motion`。父已 fetch 并合入 `origin/master` `5a1ec8ad26fa53fd6210a8e58b6ad85e739dd8bb`；实现者不再 fetch/merge，不修改 Git/index 或外部服务。旧恢复点保留在下文，不代表当前 HEAD。
- 当前 AGENTS 与验证政策取消强制 TDD；旧规格中的先行测试、有效 RED、冻结后才实施要求为历史流程，不再阻塞实现。O01–O08、E01–E13 的产品验收不减少，Rust 测试仍不得合入 master，本地不运行/编译/测试 Vega。
- 本阶段只实现供 #550/#552 消费的共享 token 与有限生命周期支持，不宣称 13 个浮层已接入，不接入真实浮层，也不改现有 hover/pressed 或业务调度。

## 本阶段验收

1. 提供 `Motion::DISCLOSURE_TURN_MS = 120`、`Motion::TAB_SELECTION_MS = 120`，保持既有 hover/shimmer 值不变；没有消费者的 token 不预建。
2. `interaction::decorative_transition(duration, visible, window, cx) -> Transition` 仅在 visible、窗口 active 且非 Reduce Motion 时允许非零时长，沿用 Kit 默认 ease_out_cubic。
3. `interaction::decorative_value(id: SharedString, target: f32, duration: Duration, visible: bool, window: &mut Window, cx: &mut App) -> MotionValue<f32>` 委托 Kit `transition_with_status`，不复制曲线/时钟/插值引擎。
4. id 由消费者的真实 owner、对象、属性命名空间组成；初挂当前终态，无关重绘不重播，同 key 连续重定向。
5. 隐藏/禁用、失活或 Reduce Motion 时到达真实 target 且不续装饰帧。失活后没有 draw 即快速恢复，也须在恢复首帧终态；必须记录真实 activation 边沿，不能仅检查本次 active 布尔值。订阅随 channel 生命周期释放，不能每帧新增订阅。
6. 不暂停 terminal、stream、permission、timers 等业务活动；不改变焦点、事件、命中或可访问状态，不新增 UI 文案、依赖、CI、全局动画管理器。

## 最小实现与验证

共享源码仅写 `crates/vega_theme/src/lib.rs`、`crates/vega_ui/src/interaction.rs` 及其子模块，文档仅更新本续接；本阶段不改 `platform.rs`、hover/pressed、消费者或其他树。消费者只使用此接口，不在 #550/#552 另建 activation 或计时引擎；`visible` 由消费者合并实际可见与启用条件。先在本分支停写固定内容，由父独立审查、提交并集成到消费者分支后交现有 Actions；无本地 Vega 执行。源码静态/格式检查不代表类型/借用检查、云端回归或真实系统观感验收通过。

## 恢复检查点 2026-10-23T03:03Z

首位共享实现执行已工具确认 failed 并终止，无具体错误详情；没有新增源码/修改既有源码，只有父先前的本文件。当时 HEAD 为 `1e6ffc3429148ab80ad282f7dc54c7e27db6e053`，Git index 空。不能归为产品、测试或环境的确定失败。父已核定 next step：单一替补只实现本页两个 API 和两个 token，复用已确认的 `use_keyed_state` 初始化 Context 注册 `observe_window_activation`，由 channel 持有 Subscription 并记录 reset 边沿；采样仍完全委托 Kit，不重做全仓调查。

## 本次恢复与共享源码交接

### 基线与实际写入

- 开工只读确认 HEAD 为 `a9c20372f6dec83e96588edb608195e0adcabda8`，状态仅本续接未跟踪，无既有源码改动。按父交接，前两位共享写入者已结束且没有留下源码/运行证据；不重审其他树、不等待前任，不将其执行失败归为产品失败。
- `crates/vega_theme/src/lib.rs`：仅新增 `DISCLOSURE_TURN_MS`、`TAB_SELECTION_MS` 两个 120ms token；既有 interaction 120ms、shimmer 1800ms 不变。
- `crates/vega_ui/src/interaction.rs`：仅声明 `decorative` 子模块并公开重导出两个冻结 API。
- 新增 `crates/vega_ui/src/interaction/decorative.rs`：两个 API 和一个只含中断位、持有 activation Subscription 的私有 channel。没有新增依赖、registry/CI 改动、Rust 测试、注释、业务实体持有或全局管理器。
- 本交接为未提交工作区内容；没有新 commit SHA。父负责 Git/index、独立审查、提交与后续集成。#549 的 O01–O08 / E01–E13 范围保留；本阶段不完成浮层接入，不标卡片 Done。#550 须等待新的限定 followup，不能在本交接后自行开始。

### 实现与生命周期证据（源码静态，不是运行结果）

1. `decorative_transition` 保留精确签名，仅 `visible && window.is_window_active() && !cx.reduce_motion()` 返回传入时长；否则 `Duration::ZERO`。只调用 `Transition::new`，默认 ease_out_cubic 不变。
2. `decorative_value` 在 `window.with_element_namespace(id, ...)` 中使用固定且不同的 `decorative-lifecycle` 与 `decorative-sampler` 子 key；生命周期 `Entity<DecorativeChannel>` 不与 Kit 的 `ValueTransition<f32>` 共用 typed key。
3. Subscription 仅在 `use_keyed_state` 的 init 中注册一次，保存到 channel 字段 `_activation_subscription`；没有 detach、每帧注册或强持有业务实体。GPUI `window.rs:3931–3951` 保存连续渲染 key 的 Entity；`app/context.rs:444–458` 通过 weak entity 观察激活、失活两种边沿。channel 释放时 Subscription 随字段释放。
4. 失活回调将 `interrupted` 置 true；两种边沿都可 `cx.notify()`，但激活分支不清中断位。下次真实采样才 `std::mem::take` 消费中断，给同一个 `decorative-sampler` 传零时长。因此即使失活期间没有 draw，快速恢复首个样本仍要求真实 target，不重放旧路径。
5. 其余采样完全交 `gpui_kit::base::motion::transition_with_status`；未实现插值、曲线、时钟、队列或手工续帧。只读核对 pinned `gpui-base-0.6.1/src/motion.rs:264–343`：初挂 endpoint、零时长更新真实 target、同 live key 从 sampled current retarget，仅 Running/Delayed 请求帧；隐藏/禁用、失活、Reduce Motion 取零时长时不续装饰帧。
6. 原 hover/pressed 实现、platform 接线、焦点/命中/可访问状态和 terminal/stream/permission/timers 代码均未改；业务继续执行的可观察断言仍由消费者云端回归验证，不能由本段源码推断冒充通过。

### 本地允许检查与内容身份

- `rustfmt --edition 2024 --check crates/vega_theme/src/lib.rs crates/vega_ui/src/interaction.rs crates/vega_ui/src/interaction/decorative.rs`：最终退出码 **0**。初次检查只报告新 helper 的一处换行格式差异；按该输出执行 `rustfmt --edition 2024 crates/vega_ui/src/interaction/decorative.rs`，退出码 **0**，随后重查通过；初次结果不记通过。
- `git diff --check`：退出码 **0**。新未跟踪文件不在该命令覆盖内，另用 `git diff --no-index --check -- /dev/null crates/vega_ui/src/interaction/decorative.rs`：退出码 **1**，无空白错误诊断；no-index 报告新增文件差异，不能把退出码写成 0。
- 三个源码完整文件 SHA-256：

| 文件 | SHA-256 |
|---|---|
| `crates/vega_theme/src/lib.rs` | `a76ebb116338853a3d93e6784c8e77d8339cc750f2127296949ee0a5f9d14684` |
| `crates/vega_ui/src/interaction.rs` | `fceed57dbe28c1b92b6bf59f50aa0e360960cc9fb4f6a44efef57695274f8f98` |
| `crates/vega_ui/src/interaction/decorative.rs` | `d6c13ebf119aa7e1c6a8f87fe72fdc937da9b7ef36cd1b6dd67d083cd8c7b0c7` |

- 源码 diff SHA-256：`ed32ed7418cb825b7a5efe840933bf06f463b83038782497aca2dd86e0ba2d70`（3022 bytes）。身份为下面两段 stdout 按顺序直接连接后取 SHA-256，不写入 Git/index；第二条正常退出码为 1（新增文件）。文档完整文件 hash 及含本续接的完整 diff 身份由最终 READY 回报固定，避免自引用。

```sh
git diff --no-ext-diff --binary --no-color --no-renames HEAD -- crates/vega_theme/src/lib.rs crates/vega_ui/src/interaction.rs
git diff --no-index --no-ext-diff --binary --no-color --no-renames -- /dev/null crates/vega_ui/src/interaction/decorative.rs
```

### 未决验证与下一执行者

- 未在本地运行 Vega、cargo check/Clippy/test/build/bench/profile、CUA 或编译；未新建测试 harness、未触发 Actions、未做外部写入。
- 父提供的已有回归：#552 saved22（独立树）包含 no-inactive-draw restore、真实 stream+terminal、实际背景与语义断言；#550 saved28 位于另一树。本实现者未读取或执行它们，不声称本共享实现已通过这些回归。
- **待父独立审查并集成后，用现有 Actions 验证当前共享源码及消费者**：类型/借用/Clippy、生命周期中断与 retarget、隐藏/禁用/Reduce Motion 停帧、无 inactive draw 的恢复、stream/terminal 不被装饰状态暂停。云端 run/job/结果尚未取得。
- 需要开发 Rust 测试时保留可恢复源码/patch、完整 SHA 与实际云端结果；合并前按当前政策清理测试及独占支持，对最终无 Rust 测试 head 跑完整 required CI，明确 `no Rust tests`（0 发现/0 执行）不是行为回归通过。真实系统观感仍待用户手测。
- 本阶段共享源码已完成并交回 READY；交接后实现者停止所有写入，父接手内容版本复核、提交与消费者集成。卡片仍未完成，浮层和 #550 不在本次后续授权内。

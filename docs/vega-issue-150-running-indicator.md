# Issue #150 · Sidebar 运行中 indicator + Composer 运行态按钮（规格冻结）

> 状态：**SPEC FROZEN（待架构师确认后进入实现）**
> 基线：`origin/master @ da4cc48`
> 来源：用户 2026-09-22 实机截图 + Issue [#150](https://github.com/puzige/vega/issues/150)
> 关联功能点：**A1-04/A1-05**（会话列表 / 会话管理）、**A2-17**（中断按钮）
> 前置：R19（主窗口壳层）、R48（sidebar indent ladder）、Issue #67（并发会话 / worker 归属）、A8-02（`ProjectWorkerActivity`）

---

## §1 用户诉求与结论

Issue 原文两条：

| # | 用户原话 | 结论 |
|---|---|---|
| 1 | *"sidebar 转圈圈 参考codex实现"* | 会话行**运行中**时，在行尾显示一个旋转 indicator；该行就是当前有 Agent worker 在跑的会话 |
| 2 | *"发送按钮运行中 改成蓝色"* | 运行中 Composer 右侧渲染的**停止按钮**（现状灰色）改成品牌蓝 |

### §1.1 为什么"发送按钮"实际是"停止按钮"

`crates/vega_ui/src/conversation_stream/render.rs:255-305`：`self.actions.running || self.composer_submit_pending` 为真时渲染 `render_composer_stop`，否则渲染 `composer-send`。两者是同一个 `action_focus[1]` 槽位的互斥分支。因此运行中用户看到的那个圆形按钮**就是** Issue 说的"发送按钮"，它就是 `render_composer_stop`。

现状（`composer_actions.rs:670-700`）：`bg(colors.bg_hover)` + `text_color(colors.text_primary)` + `Icon::Close`，是一个**中性灰**按钮。

---

## §2 现状与根因（已核实）

### R1 · Sidebar 行完全没有运行态

`crates/vega_conversation/src/types/thread.rs:83-107`：`Thread` 只有 `status: active|archived`、`unread: bool`，**没有任何 running 字段**。运行态是**瞬时**的进程事实，不属于 `Thread`（R18 要求 `Thread` 与 `threads` DDL 逐字段对齐），因此**不得**给 `Thread` 加字段。

### R2 · 现有唯一存活信号是 project 粒度，不是 thread 粒度

- `crates/vega_conversation/src/types/project_worker.rs`：`ProjectWorkerActivity` 按 **project_id** 记录 `Weak<()>`；`is_active(project_id)` 回答"这个项目还有 worker 吗"。
- 用途是 `projects_block.rs:284` 的**移除项目**围栏，不是 UI 状态源。
- 无法回答"**哪个 thread** 在跑"，而 Issue 要的正是逐行 indicator。

### R3 · thread 粒度的真值存在于 window 私有 map

`crates/vega/src/app_agent.rs:338-341`：`AppAgentController.active: HashMap<String, ActiveAgentRun>`，**按 thread_id 键**。`begin()`（`:386`）在 spawn worker 前插入，`finish()`（`:476`）在 worker 终结后移除。这是唯一权威的 thread→running 映射。

问题：`AppAgentController` 是 `VegaWindow` 的私有字段（`window/mod.rs:120`），而 `Sidebar` 是 `vega_ui` 里的独立 entity。**sidebar 当前拿不到它**。这是本卡唯一的真实架构缺口。

### R4 · 运行中的 thread 不一定是"打开中的" thread

Issue #67 允许后台运行 + 并发会话：`active` 里的 thread 可以不是 `OpenedThread`。所以 indicator 必须**遍历所有行**判断，不能只看当前打开的会话。

### R5 · 生产行的行尾在静止时确实是空的

生产 organization 列表通过 `organization/render.rs:362/469/967` 调用 `render_pinned_row` / `render_recent_row` / `render_pi_row`，三者传入的 `show_timestamp_at_rest` 均为 `false`（`threads_block.rs:1027-1073`）。因此 `render_row_actions`（`:1229`）在 `!actions_visible && show_timestamp_at_rest` 为假时，`group` 里只剩一个 `opacity(0.)` 的 More 触发器——**行尾静止态是空的**，正是 Codex 放 indicator 的位置。

---

## §3 契约

### 3.1 数据层：新增 thread 粒度的运行态投影

**R6（必须）** 在 `vega_conversation::types` 定义跨 crate 的运行态投影（架构红线：跨 crate 共享类型必须落在 `vega_conversation::types`）：

```rust
// crates/vega_conversation/src/types/running_threads.rs

/// 应用级、thread 粒度的 Agent 运行态投影。
///
/// 真值来自 vega 应用的 AppAgentController.active（thread_id → ActiveAgentRun）；
/// vega_ui 只读消费，不自行推断运行态（设计守则 §2.3：状态必须真实）。
#[derive(Clone, Default, PartialEq, Eq)]
pub struct RunningThreads {
    ids: BTreeSet<String>,
}

impl RunningThreads {
    pub fn is_running(&self, thread_id: &str) -> bool;
    /// 由 begin/finish 两处写入；同 id 幂等。
    pub fn set(&mut self, thread_id: &str, running: bool);
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
}
```

用 `BTreeSet<String>` 而非 `HashSet`：`PartialEq` 稳定、`Debug` 输出可复现，便于测试断言与 `observe_global` 的"值变才刷新"判断。

**R7（必须）** 在 `vega_ui::sidebar` 增加 GPUI global 桥接，与既有 `ProjectWorkerActivity`（`sidebar/mod.rs:167`）完全同构：

```rust
pub struct RunningThreadsGlobal(pub vega_conversation::types::RunningThreads);
impl Global for RunningThreadsGlobal {}

/// 应用级幂等 setter；值未变化时不 set_global，避免无谓重绘。
pub fn set_thread_running(thread_id: &str, running: bool, cx: &mut App);
pub fn thread_is_running(thread_id: &str, cx: &App) -> bool;
```

`set_thread_running` 必须在写入后 `cx.refresh_windows()`，让**所有**窗口的 sidebar 重绘（多窗口一致性，A1-14）。为 `None`（global 未安装）时降级为 `false`，与 `project_worker_is_active` 一致。

**R8（必须）** 唯一写入点是 `AppAgentController` 的 `begin` / `finish`：

- `begin()` 成功插入 `active` 之后 → `set_thread_running(&thread_id, true, cx)`
- `finish()` 成功移除之后 → `set_thread_running(&thread_id, false, cx)`
- **失败路径不得遗留 true**：`window/agent.rs:551`（spawn 失败分支）与 `:596`（正常终结分支）都要覆盖；`begin` 返回 `None`（重复提交）时**不得**写入 true。
- 窗口销毁时的兜底清理由 `window/mod.rs:337` 的 `active.drain()` 处补一条 `set_thread_running(false)`，防止窗口关闭后 global 残留 true。

> 若 `begin`/`finish` 拿不到 `&mut App`（签名只给 `&mut self`），允许改为返回结果由调用方写 global；但**写入点仍必须收敛在 begin/finish 的成功/失败语义上**，不得分散到渲染路径。

### 3.2 Sidebar 行 indicator

**R9（必须）** 生产行（`render_pinned_row` / `render_recent_row` / `render_pi_row`，以及 legacy 的 `render_row`）在该 thread 运行时，于**行尾槽位**显示旋转 indicator；非运行时不显示，行尾静止态保持现状（空）。

**R10（必须）** 槽位与对齐：

- indicator 与既有的 action 触发器同属 `render_row_actions` 的 `group`（`threads_block.rs:1358`），插在触发器**左侧**。
- `group` 是 `.justify_end()`，触发器右对齐；因此 indicator 出现**不得**让触发器发生水平位移（无跳变）。
- indicator 占据现有 28px 高的行尾槽，尺寸 **16px**（对齐 §7 图标的 16×16 光学网格）；行高 32px 不变，**不新增几何 token**。
- indicator 显示时**抑制**静止时间戳（`show_timestamp_at_rest` 为真时也以 running 优先）；hover 时 indicator 与 action 触发器**同时**可见（触发器保持既有 hover 语义）。
- 该行为对 pinned / project child / recents 三种行一致；`selector_prefix` 继续由调用方传入，indicator 自己的 selector 为 `{action_prefix}thread-running-{id}`。

**R11（必须）** 视觉与动效：

- 使用 `gpui_kit::component::spinner::Spinner`（`gpui-component` 0.6.0，已是既有依赖，**不新增 crate**），图标为 `IconName::LoaderCircle`。
- 颜色 **`colors.text_secondary`**（中性）：设计守则 §2.1 要求 sidebar 安静，§7 规定品牌蓝只用于主操作与明确的品牌/Agent 语义；行 indicator 属导航 chrome，取中性。**此为用户可覆盖的默认值**（见 §8）。
- 旋转周期沿用 Spinner 默认 `0.8s`，`ease_in_out`。
- **必须**走 `AnimationExt::with_animation`：GPUI 在 `reduce_motion` 打开时自动退化为静止起始帧并停止调度动画帧（已核实 `gpui-pre-0.3.4/src/elements/animation.rs:82-100`）。不得自建定时器或手写帧循环。

**R12（必须）** 状态真实性：indicator **只**由 `RunningThreads` 驱动，不得由 `unread`、`updated_at`、选中态或任何近似量推断。global 未安装（isolated embedder / 测试未 seed）→ 一律不显示。

**R13（可选，默认不做）** 不新增 sidebar 顶部"运行中"计数、不新增全局汇总指示；Issue 未要求，卡外不扩范围。

### 3.3 Composer 运行态按钮

**R14（必须）** `render_composer_stop`（`composer_actions.rs:670`）的填充由 `bg_hover` 改为品牌蓝：

| 属性 | 运行中（`stopping == false`） | 停止中（`stopping == true`） |
|---|---|---|
| `bg` | `colors.accent` | `colors.accent`（保持，运行尚未真正结束） |
| `hover bg` | `colors.brand_primary_strong` | 无 hover 变化 |
| 图标色 | `colors.brand_on_accent` | `colors.brand_on_accent` |
| `cursor` | `pointer` | 默认（既有行为） |
| `aria_label` | `"停止"` | `"正在停止"`（既有行为） |

**R15（必须）** 不得改动 `composer-send`（非运行态）分支的任何颜色与几何；不得改动 `can_send` 谓词、`action_focus[1]` 槽位、`key_context("ComposerStop")` 与 `StopComposer` 绑定。

**R16（必须）** 不得为了统一视觉把 `danger`/`warning`/`success` 语义改成蓝色；本卡只改停止按钮这一个 surface（设计守则 §4.2）。

---

## §4 非目标

- 不做 sidebar 行级进度百分比、耗时、token 计数（设计守则 §9 禁止装饰性仪表）。
- 不改 `ProjectWorkerActivity` 的语义与用途（它仍是项目移除围栏）。
- 不给 `Thread` / `threads` DDL 加字段，不新增 migration。
- 不做运行态持久化：应用重启后运行态为空（worker 已随进程消失，这是真实状态）。
- 不改 `unread` 的数据来源（仍恒 0，属 S3 范围）。
- 不引入 sidebar 顶部运行计数、Dock badge 或通知。

---

## §5 实现计划

| 顺序 | 文件 | 改动 |
|---|---|---|
| 1 | `crates/vega_conversation/src/types/running_threads.rs`（新） | `RunningThreads` + 单测（set/is_running/幂等/多 id） |
| 2 | `crates/vega_conversation/src/types/mod.rs` | re-export |
| 3 | `crates/vega_ui/src/sidebar/mod.rs` | `RunningThreadsGlobal` + `set_thread_running` / `thread_is_running` |
| 4 | `crates/vega/src/app_agent.rs` | `begin`/`finish` 写 global |
| 5 | `crates/vega/src/window/agent.rs` | 失败分支与窗口销毁兜底 |
| 6 | `crates/vega_ui/src/sidebar/threads_block.rs` | 行尾 indicator（`render_row_actions` + running 入参） |
| 7 | `crates/vega_ui/src/conversation_stream/composer_actions.rs` | `render_composer_stop` 配色 |

单卡 ≤3 个 commit，建议：`feat(A1-04)` running 投影与桥接、`feat(A1-04)` sidebar indicator、`fix(A2-17)` stop 按钮配色。

---

## §6 验收矩阵

| ID | 需求/风险 | 前置状态 | 实际操作 | 预期可观察结果 | 测试层级 | 证据 | 状态 |
|---|---|---|---|---|---|---|---|
| C1 | R6 投影正确性 | 空 `RunningThreads` | set(a,true); set(b,true); set(a,false) | `is_running(a)==false`、`is_running(b)==true`、`len()==1` | UNIT | `cargo nextest -p vega_conversation` | 待实现 |
| C2 | R6 幂等 | 已含 a | set(a,true) 再 set(a,true) | 值不变（`PartialEq` 相等），不重复插入 | UNIT | 同上 | 待实现 |
| C3 | R9/R10 行 indicator 出现 | 已挂载 sidebar，thread 在列表中 | seed `RunningThreads{a}` | `debug_bounds("thread-running-{a}")` 存在，且**不含** `thread-timestamp-{a}` | GPUI 生产挂载 | `nextest -p vega_ui` | 待实现 |
| C4 | R9 非运行不显示 | 同上，`RunningThreads` 空 | 不 seed | `debug_bounds("thread-running-{a}")` 为 `None` | GPUI 生产挂载 | 同上 | 待实现 |
| C5 | R10 无位移 | 同 C3 | 比较 seed 前后 action 触发器 bounds | 触发器 x 不变（右对齐），行高仍 32 | GPUI 生产挂载 | 同上 | 待实现 |
| C6 | R4 非打开会话也显示 | 打开 thread A，B 在跑 | seed `RunningThreads{b}` | `thread-running-{b}` 存在，A 行无 indicator | GPUI 生产挂载 | 同上 | 待实现 |
| C7 | R12 不造假 | global 未安装 | 挂载 sidebar | 所有行均无 indicator | GPUI 生产挂载 | 同上 | 待实现 |
| C8 | R8 真实起止 | 真实 production 入口，MockProvider 回放 | 发送一条消息直到 run 结束 | run 中该行有 indicator；run 结束后消失 | E2E-REAL | 见 §7 | 待实现 |
| C9 | R14 停止按钮配色 | run 进行中 | 观察 Composer 右侧按钮 | 品牌蓝填充 + `brand_on_accent` 图标；hover 转 `brand_primary_strong` | 生产像素 + 单测 | 见 §7 | 待实现 |
| C10 | R15 send 未受影响 | 非运行态 | 观察 `composer-send` | 配色/几何与基线一致（`accent` 填充不变） | 单测 + 像素 | 见 §7 | 待实现 |
| C11 | 回归：项目移除围栏 | 项目有运行 worker | 触发项目移除 | 仍被 `project_worker_is_active` 拦截（R2 未改语义） | 既有测试 | `nextest -p vega_ui` | 待实现 |

**不适用维度**：本卡不涉及持久化恢复（R16 明确不做运行态持久化）、不涉及空态文案、不涉及窄窗响应式（行 indicator 不依赖宽度断点）。

---

## §7 E2E 与证据要求

- **C8/C9 必须真实验收**：用固定位置应用 `~/Documents/Vega/Vega.app`（`ai.vega`）或从 worktree 构建的等价二进制，通过真实 UI 完成「选项目 → 输入 → 发送 → 观察 indicator → 点停止 → 观察恢复」。
- 每张截图关联用例 ID、操作步骤、预期/实际、构建 commit、时间、环境；保存到 worktree 之外的持久目录，记录 SHA-256。
- 纯渲染断言（C3-C7）允许用 `VisualTestContext::debug_bounds` 走 GPUI 生产挂载路径（与 `sidebar/threads_block/organization/tests.rs:124-152` 同构）；这些**不替代** C8/C9 的原生验收。
- 注意仓库既有约定：**合成键盘/鼠标事件驱动不了 GPUI 焦点链**，涉及焦点的走查一律人工。

---

## §8 待架构师确认的决策（实现前冻结）

1. **indicator 颜色**：默认 `text_secondary`（中性，守则 §2.1/§7）。若用户希望与 Codex 一样更醒目，可改 `accent`——请裁决，不擅自改。
2. **indicator 落位**：默认行尾（与 Codex 一致，且是生产行静止态唯一空槽）。若希望落在标题**左侧**（pin 标记位），会挤压标题起始列并影响 R48 indent ladder，需另立规格。
3. **停止中（`stopping`）是否保持蓝色**：默认保持（运行尚未真正结束，状态真实优先）。若希望停止中降级为灰以表达"正在处理"，请裁决。
4. **`begin`/`finish` 能否拿到 `&mut App`**：由实现 subagent 按实际签名选择 R8 允许的两种写法之一，不改变写入语义。

---

## §9 变更记录

- 2026-09-22 · 初版：依据 Issue #150、`origin/master @ da4cc48` 代码核实（`Thread` 无运行字段、`ProjectWorkerActivity` 为 project 粒度、`AppAgentController.active` 为 thread 粒度真值、生产行 `show_timestamp_at_rest=false`、`Spinner` 与 `with_animation` 可用）冻结本规格。

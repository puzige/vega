# Issue #427 · Agent 启动失败在对话中的归属

## 状态与本次修订

**2026-10-26 复核：未实现，规格按当前 master 重写。** 本文件此前只有 2026-10-16 的冻结方案，没有实现提交。复核时 master 为 `e481cd28`，冻结基线 `692e09e5` 已落后 122 个提交，发送链路被 #544/#627 取代，原方案指定的文件与测试路径多数已不存在。本版按当前代码重写范围与实施步骤；用户 2026-10-16 的范围裁决继续有效。

关联 #79（Composer 通用错误位置）、#397（回合底部错误合流）、#300（旧结果校验触发来源）、#544/#627（即时发送回显与提交事实行）。

## 用户决定（继续有效）

用户截图中，点击发送后出现“执行未完成，可安全重试”，错误挂在 Composer 上方。用户裁决：测试版无需兼容旧工具结果，旧数据直接拒绝可以接受；只处理 Agent 错误回到对话中的位置与归属。本卡**不补旧错误格式兼容、不修改用户历史、不放宽恢复/权限/审计校验**。

#79 要求通用 controller error 紧邻 Composer，本卡只覆盖其中的 Agent 发送、准备与运行失败；普通字段、附件、模型保存、Plan 审阅、Skill 投影等非 Agent 执行错误仍按各自既有契约处理。#397 的回合底部单一错误展示继续成立。

## 现状核对（2026-10-26，master `e481cd28`）

### 原始缺陷仍然存在

1. 旧 Edit 终态结果的固定文案含 `old_string/replace_all`，`safe_mutation_failure()` 逐字比对 `code.guidance()`（现为 `oldText`），拒绝并返回 `ConversationError::CorruptRow`（`crates/vega_conversation/src/agent/pipeline.rs` `validate_recovered_projection` / `safe_mutation_failure`）。
2. 准备分支只转发 `Runtime` 错误，其余 `ConversationError` 直接返回，安全原因丢失（`crates/vega_conversation/src/agent/entry.rs`，`Ok(Err(error)) => return Err(error)`）。这种拒绝按用户决定保留。
3. worker 把结果折成 `success = result.is_ok_and(|run| !run.failed)`（`crates/vega/src/app_agent.rs`）。
4. 无 `terminal_failure` 时调用 `ConversationStream::apply_agent_error`（`crates/vega/src/window/agent.rs`），写入 `controller_error` 并把 `controller_runtime_error` 置空（`crates/vega_ui/src/conversation_stream/core.rs`）。
5. `render_composer` 只在 `controller_error_is_run_notice()` 时过滤，而该判定要求 `controller_runtime_error` 存在（`crates/vega_ui/src/conversation_stream/run_notice.rs`），此处为 `None`，于是红字渲染在 Composer 卡片上方（`crates/vega_ui/src/conversation_stream/render.rs`）。

### #544/#627 已经补上的部分

普通 Composer 发送改走 ordinary submission：立即本地回显用户文本/图片，并有提交事实行。准备/启动/提交失败以 `SubmissionFailure` 分类内联显示在所属用户回显之下，带“重试 / 核实状态”入口（`crates/vega_ui/src/conversation_stream/ordinary_submissions.rs`，渲染在 `render.rs` 的列表行回调）。因此原方案 A01/A02/A07 的大部分位置与归属要求**已由 #544 满足**，不需要新建对话流 entry。

### 仍然存在的四条缺口

| 缺口 | 现状 | 位置 |
|---|---|---|
| G1 重复展示 | 同一失败既有对话内提交失败行，又有 Composer 上方笼统红字 | `agent.rs` 终态分支与 `ordinary_submissions.rs` 同时生效；`render.rs` 只过滤 run notice |
| G2 具体原因丢失 | 非 `Runtime` 准备错误不转发事件，`terminal_failure` 为 `None`，只剩粗粒度 `SubmissionFailure` 分类 | `entry.rs` 的准备分支；`app_agent.rs` 的 `is_ok_and` |
| G3 非 Composer 入口无归属 | Automation / ApprovedPlan / CredentialContinuation 的启动拒绝，以及 reasoning 非法、模型选择忙、store 不可用等前置守卫仍只写 `controller_error` | `agent.rs` 共 12 处 `apply_agent_error` |
| G4 已启动但无助手 message_id | `apply_agent_runtime_error_for(None, …)` 时 `controller_runtime_error` 为 `None`，被当作非 run notice，落在 Composer 上方 | `core.rs` `apply_agent_runtime_error_for` |

### 已漂移或失效的方案前提

- 原方案指定的 `agent/entry.rs`（app 侧）、`tests/issue79_composer_error.rs`、`tests/core_flow.rs` 已不存在；前者迁入 `vega_conversation` crate，仓库自 2026-10-23 起禁止 Rust 测试进入 master，无 `tests/` 目录。
- 原方案第 4 步“新增本地对话 entry（`StreamEntry` 变体）”建立在 #544 之前没有用户回显的前提上；现在 ordinary 提交事实行已承担该职责，新建 entry 会产生第二套归属。
- 原 A02 要求“不显示已发送 user echo”，与 #544 的即时回显有意冲突。**以 #544 为准**：回显是本地投影，不代表已落库；错误归属仍绑在该次发送的身份上。
- `ConversationStream::apply_provider_preflight_error` 当前零调用点；provider 前置失败实际由 `SubmissionFailure::PreflightRejected` 承担。原 A03 的分类前提已变化。
- 原方案要求“Vega 完成后交 Codex 审查、不自行开 PR”，与 AGENTS.md 当前流程（主 Agent 可直接开 PR、跟进 CI、合并）冲突。按 AGENTS.md 执行，独立审查仍须由非实现写入者承担。

## 范围

**做：**

1. 消除 G1：ordinary 提交失败已有对话内事实行时，Composer 上方不再重复同一次 Agent 失败。
2. 收敛 G2：准备阶段失败保留安全分类并送达 UI；类型化原因优先于通用原因，未知原因只显示安全的阶段性通用文案，不新增 raw error / payload / 凭据通道。
3. 归属 G3/G4：无助手 message_id 的 Agent 失败进入对话流并绑定本次发送或本次回合的身份；不得挂到上一条成功助手消息，不得伪造已发送用户消息或助手回复。
4. 保持既有边界：草稿逐字保留、恢复入口可用、迟到/跨线程回调不污染当前对话、不改变拒绝结果。

**不做：** 旧工具结果兼容、修改用户历史或数据库、放宽校验/权限/审计、重开 #79 的普通字段与保存错误布局、新增 Rust 测试到 master、改 Provider API 或依赖。

## 验收矩阵

状态一律为“待实现、待 CI”，本次修订未运行任何 Vega 代码。

| ID | 前置与操作 | 可观察结果 |
|---|---|---|
| A01 | 已有成功助手回复；存在被当前规则拒绝的旧 Edit 终态结果；再发送 | 拒绝仍发生；零新 user/assistant 落库行、零 provider 请求；草稿逐字保留；旧工具结果不变；上一轮仍为成功；本次失败只出现一处，位于对话流本次发送之下 |
| A02 | 新任务草稿，无 durable thread/message；准备失败 | 失败在对话内容区域可见；不因显示错误创建 thread、空助手回复或落库用户行；本地回显与草稿保持；Composer 无悬挂红字 |
| A03 | provider / 凭据 / 引用 / 推理配置准备失败 | 保留既有安全分类与必要恢复入口；错误归本次发送；同一错误不同时出现在对话与 Composer |
| A04 | 已开始、零文本或有部分文本的助手回合失败 | 原 message_id 的回合底部单一错误；保留已有输出、耗时与具体原因；通用 fallback 不覆盖它 |
| A05 | worker 结束失败而无 `Error` 事件；分别覆盖未接受与已接受消息 | 依显式启动/助手归属选择 A01 或 A04；不依据“最新一条助手消息”猜测 |
| A06 | 同一尝试重复投递错误，随后投递更具体原因 | 只保留一个正确错误区域；类型化原因优先；不显示 raw error / payload / 凭据 |
| A07 | 同一草稿重试、再次失败、随后成功接受 | 未接受时保留草稿与一个当前失败提示；新尝试接管身份；接受后清除本地启动错误，无残留空白；不额外启动请求 |
| A08 | A 发送后切 B 再切 A；旧尝试/代际回调迟到 | 旧回调不污染 B 或 A 的新尝试；已有回合错误仍属于其 owner；缓存保留与全量重建都不伪造未落库历史 |
| A09 | 长历史、窄窗口、浅深主题；滚动离开尾部再回到错误 | 错误参加对话虚拟列表、测量与正文列对齐，随内容滚动；不固定在输入框旁，不被折叠的工具/思考隐藏；键盘与恢复操作可用 |
| A10 | 普通保存/字段错误；现有网络重连、手动压缩失败与权限卡 | 非本卡错误不被误分类；既有运行错误、重连、压缩、权限和凭据恢复回归保留 |

A09 依赖实现后由用户手测；其余为待 CI 的静态与行为验证。

## 实施方向

按缺口顺序推进，每步独立可回滚；不重构整个错误系统。

1. **G1 去重。** 在 `crates/vega_ui/src/conversation_stream/render.rs` 的 `render_composer` 过滤条件上补一条判定：当 `controller_error` 对应的是已被 ordinary 提交事实行表示的本次 Agent 失败时不再渲染。判定依据用现有 `ordinary` 行状态与身份，不新增字符串匹配。保留 `apply_controller_error` 等非 Agent 错误的原有位置。
2. **G2 原因收敛。** 在 `crates/vega_conversation/src/agent/entry.rs` 的准备分支为非 `Runtime` 失败补充固定、安全、内容无关的阶段性失败投递（复用现有 `SubmissionFailure` 分类，不新增开放字符串通道）；在 `crates/vega/src/app_agent.rs` 的结束路径把该分类带到 UI，`terminal_failure` 存在时保持其优先级。
3. **G3/G4 归属。** 在 `crates/vega/src/window/agent.rs` 区分“未接受发送”与“已接受助手消息”：前者沿用本次发送的 ordinary 身份，后者沿用 `apply_agent_runtime_error_for` 的 message_id。`apply_agent_runtime_error_for(None, …)` 不得再写 `controller_error`；无 owner 时走安全通用分类并归属当前发送尝试。移除这些 Agent 路径对 `controller_error` 的重复写入。
4. **核对清理。** 确认 `apply_provider_preflight_error` 与 `LocalizedCopy::Conversation` 等当前无调用点的入口是否需要随本次删除；删除只限确认无调用且不影响既有分类的项，其余留给独立卡。

允许文件：`crates/vega/src/window/agent.rs`、`crates/vega/src/window/ordinary_send.rs`、`crates/vega/src/app_agent.rs`、`crates/vega_conversation/src/agent/entry.rs`、`crates/vega_ui/src/conversation_stream/{render.rs,core.rs,ordinary_submissions.rs,run_notice.rs}` 及必要 i18n 键。必要共享类型限 `vega_conversation::types`。**不修改** mutation/codec/历史校验兼容、压缩算法、Provider API/模型名单、全局权限、依赖、DDL、发布流程或用户运行时文件。

## 验证与交付

遵守 AGENTS.md 本地限制：**不编译、不构建、不打包、不运行 Vega 代码或测试**，本地只做差异、格式与静态检查。自动检查交 GitHub Actions：

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest list --workspace --message-format json   # 清点测试数
cargo nextest run --workspace --no-tests=pass
```

当前仓库无 Rust 测试，预期为 `no Rust tests`（0 发现 / 0 执行）；空集不等于行为验证，须在交付记录中如实标注。

合并条件：待合并最新 head 的 required check `check (fmt, clippy, test)` 通过，且由非实现写入者完成独立审查。合并后置 In review，等待用户手测；用户明确通过前不关闭 Issue。

用户手测：打开含被拒绝旧结果的会话，保留上一条成功回答，发送新草稿；确认拒绝仍发生，但只在对话流本次发送下出现一处失败，Composer 上方无悬挂红字，草稿与上一轮结果保持。再在正常会话验证发送、停止、具体运行失败、重连与权限卡的既有位置。**不得通过修改真实数据库制造通过。**

回滚：以新 PR revert 本次实现并重新通过 required check；不回滚用户历史、不清理被拒绝结果、不降低校验。

## 变更记录

- 2026-10-16：冻结方案，基线 `692e09e5`，分支 `feat/427-agent-error-placement`。
- 2026-10-26：复核确认未实现；按 master `e481cd28` 与 #544/#627 后的实际链路重写范围、缺口、实施方向与验证方式；文档迁入 `docs/427/`；A02 与 #544 回显的冲突以 #544 为准；原“新增 StreamEntry”方案取消；交付流程对齐 AGENTS.md。

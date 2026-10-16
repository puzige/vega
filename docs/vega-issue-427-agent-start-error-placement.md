# Issue #427 · Agent 启动失败在对话中的归属

## 冻结范围与用户决定

2026-10-16。基线 `692e09e5`。角色：Codex 分析/审查，Vega 执行。关联 #79、#397、#300。

用户截图中，点击发送后出现“执行未完成，可安全重试”，错误挂在 Composer 上方。用户裁决：测试版无需兼容旧工具结果，旧数据直接拒绝可以接受；只处理 Agent 错误回到对话中的位置与归属。本卡不补旧错误格式兼容、不修改用户历史、不放宽恢复/权限/审计校验。

#79 当时要求通用 controller error 紧邻 Composer。本次裁决覆盖其中的 Agent 发送、准备和运行失败；普通字段、附件选择、模型保存等非 Agent 执行错误仍按各自既有契约处理。#397 的回合底部单一错误展示继续成立，本卡补齐没有助手 message_id 的启动失败。

## 验收矩阵（先于实现计划）

| ID | 前置与操作 | 可观察结果 | 层级与状态 |
|---|---|---|---|
| A01 | 已有成功助手回复；合成一条被当前规则拒绝的旧 Edit 终态结果；再发送 | 启动拒绝；零新 user/assistant 行、零 provider 请求；草稿逐字保留；旧工具结果不变；错误为本次发送的独立对话行，上一轮仍成功 | app + owned store/MockProvider + GPUI，待 CI |
| A02 | 新任务草稿，无 durable thread/message；准备失败 | 对话内容区域可见启动失败；不借显示错误创建 thread、空助手回复或已发送 user echo；草稿保持 | 生产提交入口 + GPUI，待 CI |
| A03 | provider/凭据/引用/推理配置准备失败，已有安全分类 | 安全原因与必要恢复入口保留，错误归本次尝试；同一错误不同时出现在对话和 Composer | 分支级 app/GPUI，待 CI |
| A04 | 已开始、零文本或有部分文本的助手回合失败 | 原 message_id 的回合底部单一错误；保留输出、耗时及已有具体原因；通用 fallback 不覆盖它 | 事件 + GPUI，待 CI |
| A05 | worker 结束失败而无 Error 事件；分别覆盖未接受和已接受消息 | 依显式启动/助手归属选择 A01/A04；不依据“最新一条助手消息”猜测，不把错误挂到上一条成功回复 | controller 生产回调，待 CI |
| A06 | 同一尝试重复投递错误、随后投递更具体原因 | 只保留一个正确的错误区域；类型化原因优先，未知原因不制造具体诊断；不显示 raw error/payload/凭据 | 状态投影，待 CI |
| A07 | 同一草稿重试、再次失败、随后成功接受 | 未接受时保留草稿和一个当前失败提示；新尝试接管身份；MessageStarted 接受后清除本地启动错误，无残留空白；不额外启动请求 | app/GPUI，待 CI |
| A08 | A 发送后切 B、再切 A；旧尝试/代际回调迟到 | 旧回调不污染 B 或 A 的新尝试；已有回合错误仍属于其 owner；缓存保留和全量重建不伪造未落库历史 | 原 route/generation/lease 生产栅栏，待 CI |
| A09 | 长历史/窄窗口/浅深主题；滚动离开尾部再回到错误 | 错误参加对话虚拟列表、测量和正文列对齐，随内容滚动；不固定在输入框旁、不被折叠工具/思考隐藏；键盘与恢复操作可用 | GPUI 几何/状态，待 CI；用户手测待做 |
| A10 | 普通保存/字段错误；现有网络重连、手动压缩失败与权限卡 | 非本卡错误不被误分类；已有运行错误、重连、压缩、权限和凭据恢复回归保留 | 既有回归，待 CI |

新回归应使用 `issue427` 名称前缀，真实调用生产入口/投影，禁止以直接改私有状态代替整条提交失败链。可在 GPUI 测量/纯状态测试中设置必要 fixture；app 集成至少有一条来自真实旧结果拒绝的零请求/零落库路径。

旧位置断言的指定更新：`tests/issue79_composer_error.rs` 中凭据/引用等 Agent 发送失败改验独立对话行，保留草稿、零已发送回显、恢复操作、无占位及普通 controller 保存错误的原断言；`tests/core_flow.rs` 的 Agent 通用失败改验对话错误，MCP warning 独立性仍须保留。app/controller 的旧 controller_error 文案断言仅在本卡 Agent 来源内改验具体对话节点与 owner，不删除这些失败路径、安全或数据断言。

## 已确认的根因与调用链

只读本地会话证据显示，截图的上一轮生成 handoff 已成功结束，最终 primary HTTP 200。随后点击发送的内容未落入 messages，也没有新模型运行诊断；不是该成功回复发生了执行失败。

触发本次准备拒绝的合成等价输入是旧版终态 Edit 结果：

```text
Tool error: write/edit failed (edit_multiple_matches) Multiple matches. Add context to old_string or set replace_all to true.
```

在 `crates/vega_tools/src/error.rs`，当前 `guidance(EditMultipleMatches)` 改为 `oldText` 相关固定指导；`crates/vega_conversation/src/agent/pipeline.rs::safe_mutation_failure` 逐字比较当前指导。`validate_recovered_projection` 因而拒绝旧结果，`prepare_run_with_images_and_reasoning` 的事务回滚。这处拒绝按用户决定保留，不能靠兼容、忽略该历史项或删除数据让新发送通过。

错误展示链路：

1. `crates/vega_conversation/src/agent/entry.rs::run_thread_task_with_images_reasoning_and_mcp` 的 preparation 分支只为 Runtime 类错误转发 Error，其他 ConversationError 直接返回。
2. `crates/vega/src/app_agent.rs::run_agent_worker_with_mcp` 把结果折成 success=false。`window/agent.rs` 结束回调无可用具体原因时调用 `ConversationStream::apply_agent_error`。
3. `crates/vega_ui/src/conversation_stream/core.rs::apply_agent_error` 写入 `controller_error`；`render.rs::render_composer` 将此字符串放到 Composer 卡片上方。
4. `run_notice.rs` 的现有回合投影依赖 message_id。准备事务回滚时没有新助手行，不能将错误附到上一条成功回复，也不能靠创建空助手补 owner。

## 行为契约

- 已接受运行的失败继续走所属 message_id 的 run notice。已知具体原因优先；同原因去重，同回合不同真实原因按 #397 在同一错误区域保留。
- 未接受的发送失败使用独立的本地启动错误 entry，位于这次发送发生的对话边界，与正文同列。它表示一次失败的发送尝试，不冒充助手回复或已发送用户内容；不存在上轮助手 owner 的隐式回退。
- 只展示既有闭合分类映射的安全文字。未知启动失败可用 `Could not start this request. Retry or reopen the conversation.`；不承诺任意已开始动作都可安全重复，不展示底层 CorruptRow 字符串、调用 ID、路径、原输入或凭据。
- 同一未接受尝试重复回调更新同一 entry。当前尝试重试/再次失败不堆积相同错误；新尝试获得新 owner；接受 MessageStarted 后去掉旧本地启动错误及其布局占位。已有失败回合的持久化错误不因此删除。
- 发送 owner 必须与现有 thread/entity、route generation、trusted-action lease、run generation 配合。旧 owner、外部对话或重复终态不能发布到新 owner；不能只凭当前 UI 正在显示哪条助手消息判定。
- 本地启动 entry 不入 messages/tool_calls，不新增数据库字段或表。相同 stream 缓存内可保持；全量重建或应用重启不伪造未落库历史错误。已开始的历史失败仍沿用既有恢复展示。
- Agent 相关启动/运行错误从 Composer 的 controller error 展示中排除，不能仅靠文字匹配去重，也不能全局隐藏错误来“修位置”。普通非 Agent controller error 继续可见。
- 保留原始草稿、附件、恢复操作、权限门禁、预算、请求次数、工具结果和停止语义。滚动与虚拟列表沿用现有策略，不新增固定浮层、卡片、说明段或自动重试。

## 按顺序实施

1. **先补失败用例。** 在现有 app/controller 测试中，以 owned 临时 store 和 MockProvider 构造 A01；只合成必要的历史 row、严格审计与旧固定反馈，不复制真实会话/数据库。确认拒绝和 rollback 保持，同时当前 UI 错位断言可以暴露问题。补 A02/A04/A05 的 owner 差异。
2. **明确失败目标。** 在 `crates/vega/src/window/agent.rs` 的 submit/preflight/start/terminal 回调中区分未接受发送与已接受助手消息。保持并使用现有 lease/generation；必要时在 ActiveAgentRun 的 MessageStarted 路径记住已接受助手 ID，不能在结束后用最新助手猜 owner。为异步发送尝试保存稳定的本地身份；若身份必须跨 crate 共享，唯一共享定义放 `vega_conversation::types`，不增加测试专用公开 API。
3. **保留安全原因。** 为非 Runtime 的 preparation 返回补充固定的阶段性失败投递，或通过 worker terminal update 传递安全类别；在已有类型化终态原因存在时保持其优先级。现有安全通用类足够时直接复用，不建立开放 raw error 字符串通道，不改校验结果。只涉及该准备/终态结果传递，不重构整个错误系统。
4. **新增本地对话 entry。** 在 `conversation_stream/model.rs` 增加启动错误语义项，持有尝试身份与安全原因；在 `core.rs`/`run_notice.rs` 添加接收、去重、清除方法。新增 entry 需要穷尽匹配时，仅补齐 `phase.rs`、`message_anchors.rs`、`content.rs`、`attachments.rs` 等现有消费点，不重构其无关逻辑。在 `render_rows.rs`、`message_anchors.rs::estimate_entry_height/estimate_stream_entry_height`、`content.rs` 的列表行维护与实际 entry match 处补齐自然高度、刷新失效和正文列对齐，复用 `plain_error_notice` 和 danger/body/spacing token。通过列表渲染，不在 Composer 包装层另挂提示。
5. **审计 Agent 调用点。** 将 `apply_agent_error` 及 Agent 流内 provider preflight、reference/credential failure 等路径导向第 2–4 步的目标。已知 message_id 沿用 `apply_agent_runtime_error_for`；无助手 ID 的错误导向独立尝试 entry。移除这些错误对 controller_error 的重复写入，保留非 Agent 保存/字段错误。
6. **完成矩阵并回报。** 覆盖新尝试、迟到回调、重开、零文本、窄布局与安全恢复，不删既有断言换绿。Vega 回报静态检查、待云端用例、文件/提交与规格偏离（应为无），停止写入，交回 Codex 开 PR、读 CI 和集成。

允许文件：上述 app_agent/window/entry/UI entry/notice/render/measure 及对应生产回归；必要共享类型限 `vega_conversation::types`。不修改 mutation/codec/历史校验兼容、压缩算法、Provider API/模型名单、全局权限、依赖、DDL、发布流程或用户运行时文件。

## 执行与验证

分支 `feat/427-agent-error-placement` 从冻结基线创建，方案先提交到该分支，Issue 发布可访问 commit 链接后交接。执行时先 fetch、核对基线漂移，保护工作树；本卡保持单一代码写入者。遵守 AGENTS.md、docs/vega-exec-guide.md、docs/vega-issue-workflow.md、docs/vega-design-guidelines.md 及 .agents/skills/karpathy-guidelines/SKILL.md；用户当前范围决定优先。

本地仅快速格式/差异/静态检查，不编译或运行 Vega，不启动桌面或真实外部进程 E2E。供 Actions 验证：

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace -E 'test(issue427)'
cargo nextest run --workspace
```

新测试与云端结果当前均未运行，状态为待验证。只依据待合并最新 head 的 required check `check (fmt, clippy, test)` 合并；CI 每五分钟查询一次。Vega 不自行开 PR/合并，完成后同卡交 Codex 审查。合并后 In review，Issue 开放、分支/worktree 保留，等待用户纯手测。

用户手测：打开可被当前校验拒绝的旧测试会话，保留上一条成功回答，发送新草稿；确认拒绝仍发生，但只有对话流中的独立启动错误，Composer 无悬挂红字，草稿和上轮结果保持；再在正常会话验证发送、停止与具体运行失败的既有位置。不能通过修改真实数据库制造通过。

回滚：以新 PR revert 本卡实现并重新通过云端检查；不回滚用户历史、不清理被拒绝结果、不降低校验。方案/API 出现冲突由 Vega 在同卡交回 Codex，不自行扩展范围。

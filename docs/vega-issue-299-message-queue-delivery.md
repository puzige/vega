# Issue #299 后端实现交付记录

规格：`docs/vega-issue-299-message-queue.md`。本记录覆盖 store、conversation、runtime、app controller 和 config；主 agent 已集成独立 UI worktree 的队列叠层、设置和 11 个 GPUI 回归，详见 `docs/vega-issue-299-message-queue-ui-delivery.md`。独立上下文测试 worktree 的 3 个回归也已集成。

## 已实现

- 原生会话运行时按全局默认或单次选择接受 Queue / Steer。配置字段为 `agent.running_message_behavior`，旧配置和首次配置缺省 `queue`；可见名称为「排队 / 插话」。菜单切换使用持久配置 CAS 保存，并更新全局默认；旧异步读取不能覆盖新设置。
- `pending_messages` 表保存稳定 ID、线程、顺序、目标 generation、原始正文及图片。入队和返回投影在同一事务内完成；队列上限 64 条。交付以同一事务写入真实 user / assistant 行并留存已交付标识，重复 ID 不会再次消费。
- 队列自动派发只接受已登记的 thread / stream / pending ID，并使用实际持久序号回显。前台切换到其他会话时，后台队列仍属于原会话。停止、失败、Length、TurnLimit、计划审核及待处理权限不会继续自动派发。
- Runtime 只在完整工具批次之后或正常最终响应边界取一条本 generation 的 Steer；不取消正在执行的工具。落库回执成功后才追加给模型。插话会关闭旧 assistant 段，写入新的 user 和 assistant 段，后续事件改归新 assistant。
- Queue 与 Steer 均分别保存原始正文和仅供模型使用的文件展开快照。历史及回显使用原文，context source 的内容、指纹和字节上限使用相同线程 / user ID 的已交付快照。沿用文件围栏、引用上限、附件验证和凭据门禁。
- Steer 后自动上下文压缩使用新的 assistant 所有者。已结束的 live tool 组需逐项通过 ID、输入、输出、文本偏移、完整调用集合、顺序和 assistant 正文校验，再转成既有历史投影参与 source equality；保留原 source fence 和 checkpoint CAS。Computer Use 使用同一纯净化投影函数验证回执。
- 会话级异步操作令牌覆盖加载、恢复、入队、移除、提升、取回编辑与默认方式切换。恢复期间禁止新的 worker 启动，过期完成回调不能释放当前操作。活跃运行的队列加载只读取投影，不释放 runtime claim。
- 取回编辑先原子删除，成功后才调用 UI 的 `accept_pending_edit`；即使删除后的重新加载失败，已取回消息仍交给 UI。UI agent 提供 ACK 期间已有新草稿时的恢复缓冲。
- 安全边界只通过 SQL 读取下一条匹配 Steer。队列投影先读取轻量元数据，复用未改变消息的已验证图片；新增项才读取和解码 payload。

Codex ACP 仍维持现有新建任务能力边界，不新增运行中 existing-thread Queue / Steer。

## 修改路径

| 层 | 主要文件 |
|---|---|
| 数据与配置 | `crates/vega_store/migrations/0019_pending_messages.sql`、`src/pending_messages.rs`、`src/context_compaction.rs`、`src/config.rs`，以及 migration 注册和版本断言 |
| 会话服务 | `crates/vega_conversation/src/pending_messages.rs`、`types/pending_messages.rs`、`types/events.rs` |
| 持久化与上下文 | `crates/vega_conversation/src/agent/{entry,events,persistence,pipeline,compaction}.rs` |
| Runtime | `crates/vega_runtime/src/agent/{mod,loop_}.rs`、`src/computer_use/mod.rs`、导出 |
| App | `crates/vega/src/app_agent.rs`、`window/{pending_messages,agent,artifact,render,session,mod}.rs` |
| 回归 | `vega_store/src/pending_messages/tests.rs`、`vega_runtime/src/agent/tests/steering.rs`、`vega_conversation/src/agent/tests/{pending_messages,issue299_context}.rs`、`vega/src/tests/pending_messages.rs` 和配置测试 |

`vega/src/tests/pending_messages.rs` 挂在既有 `composer_actions` 的子模块下，复用真实根窗口、订阅和 worker fixture；fixture 等待初始队列读取完成。`issue299_context.rs` 为独立测试 agent 的交付，已注册。

## 回归覆盖（已编写，未本地运行）

| 范围 | 覆盖 |
|---|---|
| Store | FIFO、线程范围、稳定 ID 移除、事务回滚、交付 tombstone 防重放 |
| Runtime | 最终响应后插话续跑、取消 / 轮数上限不消费、阻塞工具期间插话不取消工具、第二次 provider 请求工具配对与新输入顺序 |
| Conversation | 连续多条 Steer 的因果历史、原始引用文本与模型快照、Queue 交付原子性 / 重放拒绝、旧 generation / 引用准备失败保留输入 |
| ACK / 缓存 | 旧 payload 损坏时新入队事务回滚；缓存复用按相同线程与稳定 ID，提升后行为 / generation 更新 |
| 真正压缩 | Queue 引用快照重开后仍冻结；无工具最终边界、工具边界 Steer 后使用真实 conversation compaction hook、持久 checkpoint 和完整 summary 工具组 |
| Controller / GPUI 根窗口 | FIFO 后台派发不改变当前 route；Stop 后暂停直到 Resume；失败与入队结束竞态保留输入；重启恢复 claim 但不自动发送；活跃读取不释放 claim |
| Controller 所有权 | 操作令牌互斥与过期回调拒绝；恢复期间拒绝 worker；End / Length / TurnLimit 权威终态；上一段完成后新 MessageStarted 清除可续跑标志 |
| Controller 操作 | 运行结束后迟到的运行中提交只派发一次；精确 ID 提升到当前 generation；菜单默认持久化且不改变既有 pending 行 |
| Config | 旧配置默认 Queue、Steer 序列化往返、既有 turn_limit 保留 |

## 已实际执行的本地验证

- 开始实现前执行 `git fetch origin` 和 `git rebase origin/master`，成功。实现期间主 agent 已观察到 master 后续前进；最终 commit / rebase / 集成由主 agent 负责。
- 对 35 个后端变更 Rust 文件执行 `rustfmt --edition 2024 --config skip_children=true`：退出码 0；同范围 `rustfmt --check`：退出码 0。
- `git diff --check`：退出码 0。
- 新增代码行静态检索：没有新增代码注释；生产改动没有新增 `unwrap` / `expect`；相关命中仅配置单元测试。
- 主 agent 执行的 Python 标准库 SQLite smoke：全部 19 个 migration 在 `:memory:` 应用成功，`integrity_check = ok`，pending 表列符合预期。此后未修改 migration。

未执行本地编译、构建、打包、cargo check、Clippy、test 或 nextest。新增及既有 Rust / GPUI 回归均需 PR Check / CI 运行，不将它们标为通过。

## 集成与残余验证

- 已集成 `PendingQueueAction::Steer(String)`、`ToggleDefault`、`ConversationStream::accept_pending_edit` 以及带真实 ID / seq 的用户回显；初版 UI 已由截图对应的叠层实现替换。
- 云端需首先验证编译、Clippy 和相关进程内测试，再完成仓库要求的全套检查。真实 macOS UI / IME / 焦点顺序及截图几何未在本地启动验证。
- 特殊工具的压缩比较保持严格：无法与持久记录的规范投影精确匹配时保留现有失败恢复路径，不放宽 source fence。新增实际自动压缩回归覆盖普通 read 工具；Computer Use 和 Skills 的既有回归仍需 CI 检查。
- 重启后未交付队列以暂停状态恢复，用户须显式继续。队列最多 64 条，每条沿用既有附件边界。

## 建议手测

1. 原生任务运行时依次输入 B / C，确认队列顺序、真实交付后回显及新草稿保留；切换另一个会话，再确认后台任务继续且当前 route 不变。
2. 工具运行期间将 C 点「插话」，确认工具照常完成，C 在安全边界进入当前任务，B 在任务自然完成后执行。
3. Stop、模型失败、输出 / 轮数上限后确认队列暂停，移除 / 编辑 / 显式继续均可恢复；退出重开后队列仍保留且不自动发送。
4. 取回编辑 ACK 前继续输入新草稿，确认原消息由恢复入口保留，不覆盖新草稿；删除失败不产生可再次发送的副本。
5. 设置及队列菜单切换「排队 / 插话」，确认重开应用后默认持久化，既有待发项语义不变；检查键盘 Enter、Shift+Enter、Tab、Escape、IME 组合输入及浅深主题。

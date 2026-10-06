# Vega A4 首版：新建 Codex 任务

日期：2026-10-06。状态：A4 用户流程与系统规格；[A4-C1 runtime 卡](https://github.com/puzige/vega/issues/281)已进入 In progress，完整功能与原生验收尚未开始。

用户已确认的流程：**在 Vega 新建任务时选择 Codex，由 Codex 完整执行任务。** 本文将这个流程拆成可实施的契约；版本和上游证据见 [调研报告](vega-acp-codex-research.md)。产品归属为 PRD 的 A4-01～A4-05。

## 1. 首版完成判据

用户选择已有项目或 worktree，在新任务中选择 Codex，发送一个编码需求。Codex 在该目录完成编辑和命令执行，Vega 展示回复、工具活动、必要的审批及真实 Git Diff；用户可以停止，重启 Vega 后可以继续同一 Codex 会话。

完整首版包括执行和恢复两部分。仅能 initialize、聊天，或者只展示模拟工具活动，均不能标记 A4 完成。费用数据缺失时显示未知；真实改动以工作区为准。

## 2. 新任务与界面契约

| 场景 | 预期行为 |
|---|---|
| 打开 New Task | 默认沿用现有 Vega 后端；提供 Agent 选择器中的 Vega / Codex |
| 草稿选择 Codex | 保留输入和附件，显示 Codex 连接状态；遵守 R69，尚未提交时不创建持久任务 |
| Codex 未配置或无法启动 | 保留草稿，显示配置入口和可操作原因；不回退到另一个后端发送 |
| 未认证 | 显示 Codex 登录入口，认证由 Codex 管理；取消后保留草稿 |
| 首次提交 | 先物化 Vega 任务，再建立并持久绑定外部 session；绑定落库完成后才发送 prompt |
| 执行中切换任务 | 后台执行继续；活动和审批始终归属原任务 |
| 打开历史任务 | 展示持久化的 Agent 来源；后续发送使用原绑定 |
| 改变 Agent | 只允许在未提交的草稿中切换；已有任务需另建任务 |
| Codex 模型/推理/模式 | 根据 Agent 返回的配置生成控件；等待设置响应后显示确认值 |

Codex 编码任务必须绑定一个用户选定的目录。附加目录只采用用户明确选定的列表。首版直接使用已选工作区，是否创建 worktree 沿用 Vega 现有项目流程；ACP 连接自身不负责隐式 checkout。

Composer 显示实际后端和模式。Codex 使用自己的配置选项，现有 Native 的 Provider、模型和权限菜单只作用于 Native 任务。Agent 显示名、可执行文件路径等配置位于 Settings → Agents，避免占用任务输入区。

## 3. 模块与共享类型

| 所属模块 | 职责与改动入口 |
|---|---|
| vega_acp（新 headless crate） | 稳定 v1 协议、进程、握手、配置、session 和协议事件；自有有界 stdio runtime，无 GPUI 依赖 |
| vega_conversation | 任务后端分派、ACP 事件转换、外部审批队列、持久化投影和恢复协调 |
| vega_store | 新增 migration、profile、session 绑定、外部活动和用量快照的存取 |
| vega / app_agent | 保留运行 owner / cancel / 后台状态；在构造 Native Provider 和 Tools 之前分派后端 |
| vega_ui | Agent 选择、连接状态、动态配置、审批选项、外部活动、未知用量状态 |

协议层提供自身的 headless 类型；跨 UI、app 和 Store 服务的公共业务类型继续由 `vega_conversation::types` 定义。UI 访问 conversation 服务，数据库行与协议 JSON 留在各自边界内。

首张实现卡需要确定的业务类型：任务后端 Native / ACP、Agent profile 引用、session 绑定、连接代次、外部轮次、外部活动、外部审批选项与 responder、外部上下文快照、外部累计费用及失败分类。现有 `PermissionDecision::Once/Always/Deny/Timeout` 无法完整表达任意 ACP options，需保留外部选项 ID 的独立变体。

## 4. 持久化与身份

| 数据 | 持久化要求 |
|---|---|
| Agent profile | profile ID、显示名、适配器类型、绝对 executable、参数数组、经过筛选的非秘密运行配置、版本身份 |
| 任务绑定 | thread ID、后端、profile ID、创建时执行配置、canonical cwd、项目/worktree 关联、附加目录、external session ID |
| 外部轮次 | 本地 run ID、session 绑定、发送意图、执行/停止/中断终态和结构化原因 |
| 活动映射 | 本地唯一 ID 与外部 session/toolCallId 的映射、内容投影、状态、来源 |
| 用量 | 外部上下文 used/size 快照，可选累计 cost、币种、来源；与 Native 单次调用账单分开存储 |
| 审批审计 | 所属任务/run、原 optionId、处理结果、来源；可执行 responder 只存在于当前进程 |

旧任务默认 Native，历史 model 和账单保持原语义。新增 schema 采用增量 migration；版本号以实施时最新 migration 为准。

`tool_calls.id` 当前是全局主键。外部 toolCallId 需要通过 session 与本地 ID 映射，避免不同任务或 Agent 使用同一 ID 时串卡。消息 ID 可选，不能按文本猜测历史去重。

创建 session 与数据库写入不是一个事务：先保存创建意图，收到 session ID 后立即保存绑定，成功后再发送 prompt。若中间崩溃而不能确认绑定，恢复时显示准确状态，不能通过重复 prompt 猜测进度。profile 后续修改不能静默改变已绑定任务的执行目录、权限或恢复所用配置。

## 5. 协议与资源边界

**A4-C1 冻结决策：不引入 ACP Rust SDK。** SDK 2.2.0 虽支持稳定 v1，但其官方 transport architecture 暴露 unbounded channel；现有证据不能证明应用层限流覆盖 SDK 内部积压。runtime 使用 Vega 已批准的 Tokio、Serde 和 `serde_json`，自行实现 ACP v1 stdio 边界，并在入队前执行逐行与队列双重容量限制。逐项限制与超限处理见 [A4-C1 runtime 合约](vega-acp-codex-c1-runtime.md)。

initialize 保存实际 protocolVersion 和 capabilities；不启用 draft v2。首版支持文本与 Agent 声明的图片/嵌入资源；附件超出能力或应用大小上限时，发送前给出明确错误。

Client 暂不声明 `fs/read_text_file`、`fs/write_text_file` 或 `terminal/*`。Codex 自己执行文件与命令，Vega 展示它发出的活动。新增 Client 能力应各自实现授权、冲突处理与生命周期后再声明。

ACP v1 stdio 按换行界定 frame；frame 可以是单个 JSON-RPC 消息或批次数组。Vega runtime 保留批次边界，校验完整 frame 后整体分派，并将需回复的 batch entries 合并为一条 response array。单帧、批次元素、请求数、事件容量、stderr 处理及超限收尾的具体值见 [A4-C1 runtime 合约](vega-acp-codex-c1-runtime.md)。协议读写持续推进，审批等待不阻断其他响应和取消；任何已声明的容量溢出均显式失败且不丢失权限请求、终态或历史。

实现规格已确定单帧、待处理请求、活动队列、stderr retention 和超限收尾边界，详见 A4-C1。ACP v1 stdio 接受至多 16 个元素的批次帧，完整验证后保留批次边界，并按原顺序将 response-bearing entries 合并为一个 response array；不拆分、重排或部分派发。协议读写持续推进，审批等待不阻断其他响应和取消；队列溢出会终结连接，而非丢弃内容后继续。

## 6. 审批与默认模式

首版新 Codex 编码任务建议使用 `workspace-write`，在首次 prompt 前设置并等待 Agent 确认。适配器默认的 `agent` 模式使用 auto_review；不得把尚未确认的默认值显示为用户审批模式。[Codex v2.0.0 模式源码](https://github.com/agentclientprotocol/codex-acp/blob/v2.0.0/src/AgentMode.ts)

审批按 `(thread, run, connection generation, request ID)` 绑定。界面保留 Agent 提供的选项名称和原 `optionId`；提交前验证仍属于当前请求。用户选择只答复一次。取消、停止、断连和过期都终结 responder，旧响应不能投递到新连接。

`allow_always` 的授权范围由 Agent 定义；Vega 不将其写为 Native 工具的永久规则。展示命令或目标时采用有界内容和现有脱敏边界。Codex 活动不经过 Vega Native 工具再次执行。

## 7. 停止、断连与重启

状态至少区分连接中的启动/握手/待认证/就绪/失败，session 的创建/恢复/已绑定，轮次的执行/待审批/正在停止/完成/中断/失败。

停止发送 `session/cancel` 并终结待审批，界面保持“正在停止”。收到稳定 v1 prompt 终态才能确认协议完成；断连或停止超时则记录中断与结果未知，清理本次拥有的进程。不能将“发出 cancel”记作 Codex 已停止。[取消规范](https://agentclientprotocol.com/protocol/v1/cancellation)

| 恢复场景 | 操作 |
|---|---|
| 本地完整历史，Agent 支持 resume | resume 原 session；检查返回配置后允许新提交 |
| 本地历史需重建，Agent 支持 load | 回放写入临时投影，load 成功后原子替换 ACP 历史投影 |
| 回放中断 | 保留原投影，不提交半份历史 |
| 会话不存在或目录不可用 | 保留历史，展示恢复失败；由用户选择新建任务 |
| prompt 结果不确定 | 查看外部历史及真实工作区；不自动重发原需求 |

回放事件只用于重建展示与审计。用户自定义标题、置顶/归档、项目关联等本地属性保留。启动恢复不恢复旧 responder，也不自动继续执行中断轮次。

进程首先按每个活动任务一条连接管理，设置并发与空闲策略。退出时处理轮次和审批，按支持情况关闭 session，等待进程退出；超时清理只针对本次启动且身份仍匹配的进程。适配器拥有的 Codex 子进程也需要明确退出和收割策略。

## 8. 认证、MCP、Skills 与分发

首版认证流程优先支持 Codex 已登录状态及 Agent 返回的 ChatGPT 认证方法。Vega 不读取或复制 Codex 的 OAuth token。API key 和 gateway 方法作为后续独立凭据契约；适配器的 gateway 依赖 Client capability，URL/device-code 也各自有 capability 条件。[Codex v2.0.0 认证方法源码](https://github.com/agentclientprotocol/codex-acp/blob/v2.0.0/src/CodexAuthMethod.ts)

首版不向 ACP 自动导出 Vega Provider 凭据、MCP 或 Skills。Codex 使用的本地配置需在 Agent 设置中明确标识。`mcpServers: []` 只说明 Vega 未传入服务器，不能据此保证 Codex 没有自己的 MCP 配置；实现前须核对启动实际配置及覆盖规则。共享 Vega MCP/Skills 另设授权、同名冲突和撤销规格。

开发接入先支持用户配置的确定 executable 和参数数组，固定官方 codex-acp v2.0.0 release 与其 bundled Codex 0.158.0。本机 CLI 0.157.0 不作为兼容替代，需在 pinned-release 集成验收中验证具体组合。发布包的 helper 分发、许可证、完整性和 macOS 签名需另有交付记录。

默认不设置 `APP_SERVER_LOGS`。日志和诊断保存限量结构化状态；原始 prompt、文件、环境变量和凭据不进入默认诊断导出。实际对话与工具内容属于用户私有的会话存储，需要沿用现有脱敏及删除生命周期。

真实 Git Diff 可以复用 Review；Undo 需要外部改动前的恢复点。首版未提供恢复点时应明确功能不可用，不能把收到的修改后 diff 当作 checkpoint。

## 9. 实施卡建议

| 顺序 | 交付范围 | 必须闭合的证据 |
|---|---|---|
| [A4-C1](vega-acp-codex-c1-runtime.md) | 有界 ACP v1 传输、进程及协议 runtime | 帧与队列上限、握手/会话/权限/停止、超限失败和 owned-child 收尾 |
| A4-C2 | Vega 任务存储、Agent profile 与 New Task 到完整 Codex 执行 | 草稿、发送、流式活动、原 options 审批、真实编辑/命令/Diff、停止、后台归属 |
| A4-C3 | 会话恢复与用量 | resume/load、原子回放、未知结果处理、重复快照、未知费用、子进程收尾 |
| A4-C4 | 认证、配置及产品交付 | ChatGPT 登录/取消、动态模型、helper 身份、安装与版本回归记录 |

除 A4-C1 外，其余仍是拆卡建议，尚未创建。A4-C1 runtime 可与后续 storage/业务类型准备分开实现；跨 crate 共享业务类型与 app/UI 路由仍由后续单一 owner 负责。代码按仓库要求交给专用子 Agent，主 Agent 负责审查、集成与证据。

## 10. 验收清单状态

[调研报告 A01～A18](vega-acp-codex-research.md#待执行的验收矩阵) 仍全部未执行；再补以下边界作为对应卡的验收要求：

- 单帧、批次、消息洪峰与长输出超限：观察真实传输和内部积压；确认 UI、取消及审批能收尾。
- session 创建返回前后分别中断：没有 prompt 重放；已物化任务有准确恢复状态。
- 不同任务收到相同 toolCallId：活动、审批和持久化各自独立。
- 修改 Agent profile、恢复目录变更、Codex 自有 MCP：不静默改变已绑定任务的权限和配置。

自动门禁采用进程内协议/业务替身，真实 Codex、登录、编辑和原生 UI 单独留证。后续版本 QA 卡记录 Vega 版本、ACP 协议版本、adapter、Codex/runtime 实际版本、包身份、覆盖用例及未执行项；握手通过不能代替完整编码验收。

# Issue #299：运行中的新消息排队与引导

来源：[Issue #299](https://github.com/puzige/vega/issues/299)，用户要求参考其他 harness 调研并实现。2026-10-13 制定；规格先于实现。功能归属 A2-18 排队消息 / A3-03 agent loop。用户同日追加：在设置里控制默认行为，并要求缩短「引导当前任务」；界面采用「排队 / 插话」，内部 Queue / Steer 语义不变。用户随后提供两张 Codex composer 截图并要求按其交互和叠层布局还原，最新契约见 §2.2；参考图片及私人内容不入仓库。

## 1. 调研与现状

| Harness / 一手证据 | 已核实行为 | Vega 采用的约束 |
|---|---|---|
| [Pi 使用文档](https://github.com/earendil-works/pi/blob/f13c00fb3a4d77ad3faa815057e31d614d42b3f1/packages/coding-agent/docs/usage.md)、[agent loop](https://github.com/earendil-works/pi/blob/f13c00fb3a4d77ad3faa815057e31d614d42b3f1/packages/agent/src/agent-loop.ts) | Enter 的 steer 等当前响应及工具调用完成；follow-up 等完整任务完成；中止后可取回待处理输入 | steer 与 queue 是不同的消费时机；不通过取消当前工具伪装 steer |
| [Codex TUI 输入队列](https://github.com/openai/codex/blob/d583e73c4d1204f1e9f654ef87065e9f0dd07ac7/codex-rs/tui/src/chatwidget/input_queue.rs)、[turn/steer 参数](https://github.com/openai/codex/blob/d583e73c4d1204f1e9f654ef87065e9f0dd07ac7/codex-rs/app-server-protocol/schema/typescript/v2/TurnSteerParams.ts) | 区分 queued、pending steer、rejected steer；steer 带 expectedTurnId，避免误投其他运行 | 消息有稳定 ID，绑定 thread 与运行 generation；待处理不冒充已送达 |
| [Copilot SDK](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/steering-and-queueing) | enqueue 为 FIFO，当前轮结束后处理；immediate 在下一次模型调用前注入；空闲时两者启动新轮 | 默认 queue，显式选择 steer；空闲发送维持现有行为 |
| [ACP Codex adapter](https://github.com/agentclientprotocol/codex-acp/blob/9e1690ff79aad55f173721907c7a65d9fe8b8d64/src/CodexAcpServer.ts) | 部分新 adapter 提供并声明 steering 扩展，不能据此假定所有 ACP adapter 支持 | 本卡不猜测 ACP 能力，不把原生队列的承诺投射到未接通的 Codex 会话 |

Vega 当前在 `ConversationStream::submit_message` 和 `VegaWindow::submit_composer` 两层拒绝 active run 期间的提交。原生 loop 没有后续输入通道。运行生命周期已按 thread / generation 归属，消息通过 conversation persistence actor 落库。Codex ACP 当前 controller 明确只允许新建任务，不支持 existing-thread 后续输入。

## 2. 产品契约

### 2.1 范围

实现 **Vega 原生会话**（项目任务与独立任务）的运行中 queue / steer。现有空闲发送、模型/权限/Skills、附件、文件引用、IME、Shift+Enter、停止、计划审核与后台会话保持其契约。

Codex ACP 的后续会话、steering 扩展协商属于其现有未完成能力，不在本卡改造；该 backend 不显示可用的 queue / steer 操作，不新增虚假接收。不得用 cancel + 新 prompt 代替 steer。不新增 provider、依赖、自动安装或发布配置。

### 2.2 可见操作

- 「设置 → 通用」提供「运行中发送消息」选项，值为「排队」与「插话」，全局持久保存；首次和旧配置缺省为「排队」。「插话」对应 steer，表示运行中补充要求或调整方向。复用既有配置读写/验证路径，不另建一套设置存储。变更即时影响后续提交，不追溯修改已经接收的消息，也不改变 Codex 能力边界。
- 运行中保留可编辑输入框与停止入口，Enter/主发送动作遵循默认设置；不在 composer 内增加常驻的双模式按钮行。
- **按用户 Codex 截图还原待发条**：待处理消息在 composer 上沿后方叠放，左右略内缩，浅细边框和圆角，composer 作为前景遮住待发条下缘。左侧队列/插话图标、可选附件缩略图和单行省略的用户内容；右侧依次为转向图标加「插话」、垃圾桶、「⋯」。不加标题、副标题、卡片统计或实现说明。无待处理消息时不占位。
- 每条 queue 的「插话」按钮把**这一条**按稳定 ID 原子转换成当前 thread/generation 的 steer；原顺序内的其他 queue 不受影响，全局默认不被改变。已交付/正在原子交付的消息不能重复提升、移除。无活跃任务时提供明确恢复动作，不能让按钮假装正在 steer。
- 垃圾桶移除该条；「⋯」打开与截图同类的轻量菜单，包含「编辑消息」及「关闭排队」（默认 queue 时）/「开启排队」（默认 steer 时）。开关直接修改同一全局设置，保存成功后同步 composer 和通用设置；失败保留原值并显示可恢复错误。截图中的 side chat 不属于 Vega 已有能力时，不增设无效菜单，不扩展新侧聊系统。
- 「编辑消息」只在持久队列原子移除成功后取回原文及附件，不提前复制成第二条草稿。已有草稿时禁用且提供可访问原因；若等待确认期间用户输入了新草稿，保留新稿并暂存已取回消息，提供「继续编辑」恢复动作，不静默覆盖或丢弃。队列重读失败不能丢弃已经成功取回的消息。菜单支持 Esc、外部点击关闭、合理回焦，并使用 deferred 绘制以免被 composer 边框盖住。
- 队列停止后显示「继续队列」动作。提交、转换投递行为、移除、编辑、恢复均使用稳定 ID，不用当前可变数组下标定位异步结果。
- 所有新增按钮具备可访问名称；Tab/Enter/Space 操作和焦点路径覆盖；只在真实失败、暂停待处理时保留必要提示，不展示内部 generation、消息 ID 或执行阶段名。

叠层几何使用共享 Layout token，以下为适配 Vega 现有 composer 的逻辑像素，不把裁剪截图像素冒充运行实测：待发条左右内缩 16、每行可见高 36、末尾覆盖区高 8、外圆角 16、缩略图 24、图标沿用 16、操作槽沿用 28；最多三行可见后内部滚动。composer 的既有宽度、圆角、输入高度不变。数值验算：宽度 W 时待发条宽 W−32；W=768 时为736；n条的待发层总高36n+8，composer 向上覆盖8，整个组合相较空队列只增加36n（n取可见行数1–3）。新增区域的点击命中只在可见行内，覆盖区不得吞掉 composer 输入。

### 2.3 消费语义

1. **Queue**：本条提交不进入正在运行的模型上下文。只在当前运行正常结束、worker 的 terminal 确认且无待审批/待回答/失败恢复阻塞后启动最早一条；每次只启动一条，不合并多条为一个 prompt。若线程当前在后台，归属仍是原线程，不能污染正在查看的线程。
2. **Steer**：在当前模型响应及其工具批次完成后的安全边界，将新用户输入加入同一逻辑任务的后续上下文，再发起下一次模型调用。不得打断已开始的 shell/文件操作；不得破坏 assistant tool-call / tool-result 配对。空工具的最终响应边界也必须检查 steer，不能在最后一刻漏掉已接收消息。消费后继续原始任务，原有 run 的取消、权限与 turn-limit 约束仍适用。
3. **竞态**：消息绑定 thread / generation。steer 到达时若该运行已经结束，只能转为该线程可见的待处理后续消息（正常完成时可按队列继续）；不能投给另一个 generation。停止/失败赢得竞态时，待处理消息保留且暂停。
4. **取消与异常**：停止、provider 错误、凭据/文件引用错误、达到 turn limit、人工审批/计划等待、worker 断连均不自动消费后续 queue。已提交尚未落地的 steer 同样保留为可处理输入；不发生静默丢失或自动重试循环。恢复由用户显式操作。
5. **接收与回显**：清空 composer 必须等接收成功；清除的只是对应冻结提交，用户后来输入的草稿不受影响。待处理列表和已发送 transcript 分开。只有 durable user message / runtime consumption 证据才移到正式历史，不得把入队显示成模型已经处理。
6. **消息完整性**：保存原始文本、附件引用和所需提交意图；本卡 queue / steer 的用户历史显示原始文本，模型使用展开后的文件引用快照，该快照参与上下文预算和指纹。保持现有 credential guard、文件引用解析、图片校验及权限边界。不能为了 steering 绕过这些检查。提交准备失败保留可恢复输入。Steer 不得偷偷改变活跃任务的模型、权限或 Skills 配置；queue 重新走正常发送验证。
7. **上下文压缩**：插话切换 assistant 片段后，自动压缩使用新的活动消息归属。此前片段的 live 工具配对与持久历史投影必须在严格验证内容、工具 ID、参数、结果和完整集合后对齐；不得通过跳过来源指纹、归属或一致性检查让插话继续。

### 2.4 生命周期与恢复

- 队列以 thread 为单位，切换任务、stream 重建或历史刷新不能丢失。
- 接受后的待处理输入持久化；重启恢复为**暂停**，不自动执行用户遗留任务。已交付 ID 不可重复发送。可复用现有持久化设施；需新表则新增递增 migration，UI 不直接操作 SQLite。
- 持久化或交付失败不能提前从队列移除。交付与用户消息创建应有原子归属/去重约束，防止崩溃或重复终态造成重复执行。
- 不输出 prompt 正文、图片内容、文件内容或凭据到诊断日志；队列存储沿用现有 conversation 数据边界。

## 3. 实现分工与路径

主 agent 负责调研、此规格、审查、集成、PR/CI 与看板。底层实现 subagent 在 `feat/299-message-queue` 独占 store / conversation / runtime / app controller；UI subagent 在独立 worktree 的 `feat/299-message-queue-ui` 独占 UI / theme。共享 API 先约定，主 agent 最后集成，避免并发编辑同一文件。

允许修改的模块：

- `crates/vega_store`：必要的队列表、CRUD/原子转移、migration、默认投递设置的配置持久化与进程内存储测试。
- `crates/vega_conversation`：共享 UI/应用类型、队列服务、原生 run 接收/持久化/事件投影、相关测试；保留 runtime 无 UI 依赖。
- `crates/vega_runtime/src/agent`：有界 steering 输入边界（异步通道或 conversation-owned 持久队列 source，每次最多消费一条）、安全边界、上下文续接、相关 mock provider 回归。Runtime 不反向依赖 conversation；使用其已有 ChatMessage / event 边界，由 conversation 转换。持久表作为待处理输入的单一来源，移除/取回和消费须原子协调，不维护互相漂移的第二份队列。
- `crates/vega_ui/src/conversation_stream`、`settings` 及所需共享 token：composer 模式、待处理列表与操作事件、收发状态、通用设置选项、GPUI 测试。
- `crates/vega/src/app_agent.rs`、`window` 中相关会话/agent/设置路由、对应 `tests`：异步入队/恢复、generation 归属、后台队列派发、终态暂停和恢复、默认设置投影。
- `docs/vega-issue-299-message-queue-delivery.md`、`docs/vega-issue-299-message-queue-ui-delivery.md`：实现者交付记录。

实现者先将本规格的验收场景转成测试，再实现。具体内部函数拆分由实现者决定；若上述契约与现有架构存在冲突，先带证据交回主 agent 修改规格，不自行缩减。

## 4. 验收矩阵（实现前建立）

| ID | 前置 / 操作 | 预期可观察结果 | 证据层级 / 状态 |
|---|---|---|---|
| Q01 | 原生任务运行，排队 B、C | 当前请求不包含 B/C；正常结束后依次独立执行 B、C，各一次 | runtime/controller mock，待 CI |
| Q02 | 当前工具批次阻塞时发送 steer B | 工具未被取消；完整工具结果后、下次模型请求前加入 B | runtime mock，待 CI |
| Q03 | 无工具最终响应边界发送 steer | 已接收输入不丢失；继续处理，或可见地转为同线程后续输入 | runtime/controller，待 CI |
| Q04 | 输入 A 被接收期间继续键入 D | 只移走 A，D 和其附件保持 | GPUI，待 CI |
| Q05 | 移除/取回 B，另有草稿 D | B 精确变更，C 与 D 不丢失；可键盘操作 | GPUI/controller，待 CI |
| Q06 | 排队后停止、provider 失败、turn limit | 不启动后续；消息保留，显式继续可恢复 | runtime/controller，待 CI |
| Q07 | 入队/落库失败、重复终态 | 草稿或队列仍可恢复，零重复 worker 启动 | storage/controller 故障注入，待 CI |
| Q08 | A 运行并排队，切到 B / 重建 A stream | 队列只属 A，不污染 B；正常完成的 A 可继续 | controller/GPUI，待 CI |
| Q09 | 重启恢复待处理队列 | 内容/顺序保存，恢复暂停，已消费项不重放 | store/conversation，待 CI |
| Q10 | 权限/计划等待、文件引用/凭据/图片校验失败 | 不跳过现有门禁，不在阻塞时自动启动后续 | conversation/controller，待 CI |
| Q11 | 多次 steer 后读取历史并继续对话 | 用户/assistant 因果顺序、工具配对、图片/文本一致，无重复回显 | persistence/runtime，待 CI |
| Q12 | 同时停止与提交 / generation 切换 | 消息不投给其他运行，未消费项保持可见可恢复 | controller/runtime，待 CI |
| Q13 | 空闲发送、IME、Shift+Enter、Codex ACP | 原有行为保留，ACP 不出现虚假可用队列 | GPUI/controller，待 CI |
| Q14 | 原生 UI 浅深主题与窄窗口 | 主动作和停止可发现；默认/运行/暂停/失败状态与键盘路径清楚 | 用户合并后手测，未执行 |
| Q15 | 旧配置/首次启动，保存另一默认并重启 | 缺省 queue，选择可保存/读取；运行中 Enter 按配置发送，空闲行为不变 | config/settings/GPUI，待 CI |
| Q16 | 已有队列时切换默认，单次覆盖投递行为 | 只影响新提交；已有消息保持其行为；单次覆盖不改全局设置 | controller/GPUI，待 CI |
| Q17 | 在待发条点击插话/垃圾桶/更多，键盘打开菜单 | 精确操作对应消息；插话不改全局默认；菜单开关与通用设置同步；编辑保留其他草稿 | controller/GPUI，待 CI |
| Q18 | 待发条叠层、附件、长文本、多条、浅深主题 | 附件缩略图/单行省略；右侧操作始终可用；composer 遮下缘且输入命中正常；菜单不被边框覆盖 | GPUI布局/绘制 + 用户手测，待 CI/未手测 |

## 5. 验证、交付与回滚

本地仅 `git diff --check`、格式和轻量静态检查；**不运行 cargo check、Clippy、test、nextest、build、打包**。所有功能点测试通过云端 PR Check 执行，不把本地未执行写成通过。禁止为 CI 变绿删除/弱化业务与安全断言，不新增真实外部进程或网络 E2E。

遵循 `AGENTS.md` 与 `docs/vega-exec-guide.md`：新增代码不写注释（仅必要 unsafe SAFETY 例外）、不用非测试 unwrap/expect、无同步 UI IO、新依赖须先裁决。

PR 使用 `Refs #299`，不得自动关闭。云端门禁通过后按仓库流程 squash 合并并置 In review，等待用户手测；保留本卡分支/worktree。回滚通过 revert 本卡 PR；schema 仅增不删，保留未发送消息数据，不静默删除用户输入。

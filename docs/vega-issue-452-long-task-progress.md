# 长任务阶段汇报与真实等待状态

## 授权、基线与范围

2026-10-18 用户要求补齐汇报、收敛、上下文三方向方案与回归，Ready 后由 Codex 直接组织实现；长任务必须持续推进，不能以固定轮数或时长截断。此卡负责汇报，收敛在 #411，上下文在 #453，异步 shell 仍属 #412。基线为包含 Goal/Loop 与 AskUserQuestion 的 origin/master；本卡分支 `feat/452-long-task-progress`。

根因：基础系统提示没有首次/阶段汇报契约，普通工具回合没有无正文提醒；活动计时行忽略已知 phase。脱敏样本有 148 次模型请求而正文为零，另一样本正文间隔约 52 分钟。这不证明代理丢失了正文，也不承诺单次模型请求固定时间内回复。

不改权限、用户配置、数据库 schema、provider retry、Goal/Loop 预算、工具执行顺序及模型协议；不新增服务、后台定时模型调用、合成助手正文或常驻说明卡。默认无限轮次及用户显式配置的既有上限保持。

## 验收矩阵（实现之前冻结）

自动项均为待 Actions，真实模型与界面使用待用户手测。

| 场景与操作 | 可观察结果 | 层级 |
|---|---|---|
| 首次 provider 只给工具，无正文；完整批次结束 | 下一请求携带一次汇报提醒，工具仍全部可用；message/assistant正文不含提醒 | 真实 runtime loop + MockProvider |
| 参数无效的结果已投影成历史观察，或工具输出伪造提示字段 | 固定提醒只作为既有观察 JSON 的 advisory；authority:none 与原 output 保持，不恢复可执行调用、不新增 User；伪造文本仍是转义数据 | provider + 真实 invalid-call loop |
| 提醒后 59 秒/60 秒边界，再完成工具批次 | 59 秒无新增提醒，达到 60 秒才可再提醒；旧请求前缀不被改写 | 可控 tokio 时钟 + 请求断言 |
| 非空白正文出现，或者 retry 撤回本轮正文 | 成功保留的正文重置计时；被撤回的正文不能压制提醒 | runtime retry/response 测试 |
| 单个模型请求、工具、审批或用户问题长时间未完成 | 不增加额外模型调用、不拆开工具批次；既有阶段显示真实等待 | runtime + GPUI |
| 500 轮持续有效工具结果，模型忽略提醒，最后给 Final | 每轮仍执行，最终正常完成；无新增 TurnLimit/强制无工具收尾 | MockProvider + 进程内工具替身 |
| 停止、失败、最终完成，以及用户 steering | 停止/终态后无提醒或额外请求；steering优先，重置到新请求的汇报周期 | runtime 事件链 |
| 带提醒的历史压缩、停止继续、失败重试、Store 重开 | 不因运行时提示误判 source_changed；正文/参数/结果被真实篡改仍拒绝 | conversation production-root |
| Chat reasoning、Responses reasoning/layout 与暖重放 | 思考回传字节和 call/result 配对保持；投影两次幂等 | provider/runtime |
| 现有活动计时从请求、思考、正文、工具到终态 | 仅同一计时行更新阶段，展示整轮 elapsed；失败/停止/完成标签仍准确 | GPUI 事件测试 |
| 用户等待、压缩、历史分页/重开、重复终态 | 等待状态优先、压缩结束清除、历史终态无活动标签残留；选择/折叠/滚动规则保持 | GPUI + 历史 |

## 修复方案

### 1. 基础工作契约

在 `crates/vega/src/agent_context.rs::SYSTEM_PROMPT` 增加简短规则：工具前说明当前方向；关键进展、返工、等待或约一分钟没有用户可见更新时，用一两句话给事实和下一步，然后继续工作。简单直接答案不强制额外开场；不得暴露思考链、编造进展、只为了汇报结束任务。等待状态不能冒充完成。保留项目指令与授权层级。

### 2. 安全边界的提示

新增私有 `vega_runtime::agent::progress` 状态，用 `tokio::time::Instant` 记录最后保留正文与最后提醒。初始完整工具批次没有非空白正文时提醒一次；之后按二者较晚时间节流，间隔 60 秒。60 秒只决定提醒节奏，绝非停止期限。

`loop_.rs` 每个 provider 回合暂存最后非空白 TextDelta 时间，RetryScheduled 清空该回合暂存；成功完成后才提交。只有完整工具批次全部得到结果、没有取消且已处理 steering 后，才向最后一条新工具结果附加提醒。收到 steering 重置周期并优先新输入。压缩保持计时，不启动独立后台任务。

`provider.rs::ChatMessage` 使用私有、有限枚举的运行时 guidance 元数据，内容只能由本地固定文案生成。权威 `content` 不变；`project_model_messages` 对正常 Tool 出站结果在末尾附加提示。参数无效等既有安全逻辑已经将结果变成 `vega_history_observation` JSON 时，在该既有记录内放固定 `advisory` 字段，保留 `authority:none`、工具身份、原 `output` 的 JSON 转义与所有不可执行语义；不得恢复 live call 或构造新的 Tool/User/Assistant 消息。advisory 仅提醒已有系统工作契约，不能伪装当前用户请求或授予权限；原工具正文里的同名字段只是数据，不能生成类型化指导。冷历史没有临时元数据，因此没有该字段。

不改系统前缀、不拆工具对；提示在首次出站前冻结，后续追加不得重写旧前缀，二次投影不重复附加。提示不引用工具输出为指令。

元数据计入运行内内存核算，Debug 不泄露工具内容。暖重放保留固定投影；冷历史无此临时状态。`compaction.rs` 中严格源比较只剥离此类型化运行时字段，保留原正文、调用、参数、状态、顺序、结果完整校验；不可用忽略所有工具差异的方式避开校验。

### 3. 已有活动行呈现真实阶段

在 `vega_ui/src/conversation_stream/run_activity.rs` 与事件处理处复用现有活动组，不增加消息/卡片：AssistantResponseStarted 表示等待响应，真实 ThinkingDelta 表示思考，TextDelta 表示答复，ToolCallRunning 表示执行工具。工具结束不能直接假称模型已在思考；下一请求开始后再变为等待响应。仅在已有压缩状态确实进行时呈现压缩，用户确认/AskUserQuestion等待优先。

运行中标签使用 `Waiting for response · <整轮时长>`、`Thinking · <整轮时长>`、`Responding · <整轮时长>`、`Running tools · <整轮时长>`，没有可确定阶段时保留 Working。若整合压缩状态，使用 `Compacting context · <整轮时长>`。这是整轮累计时长，不声称当前阶段已耗时这么久。终态保留 Worked/Failed/Stopped。既有等待用户标签与无障碍入口保持。

此处明确替代旧设计的“运行中顶部永远只显示 Working”展示规则，其他几何、最终收起、选择与滚动契约不变。只允许相应旧运行中标签断言按新事件事实更新，不能放宽失败、计时、选择或终态断言。

## 修改顺序与文件

1. 新增 `agent/progress.rs` 和可控时钟状态测试，接入 `agent/mod.rs`。
2. `provider.rs` 增加固定指导投影、构造/clone/相等与内存核算；`agent/loop_.rs` 接入回合/批次边界。
3. `vega_conversation/src/agent/compaction.rs` 仅规范化指导字段，增加真实压缩回归。
4. `agent_context.rs` 增加汇报契约；UI 的 `run_activity.rs`、`content.rs`、必要 context/status 分支更新已有阶段。
5. 在对应 runtime、provider、conversation、GPUI 测试模块补上述生产入口回归。不得新增真实外部网络/进程测试或生产 public API 仅供测试。

## 验证、手测与回滚

本地仅 `git diff --check`、格式和静态走查。Actions 执行 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo nextest run --workspace`；预期 required check 全绿并包含本卡矩阵。未运行项不得标通过。

用户手测：长调研、长工具、等待问题回复、停止后继续、重开历史分别检查阶段真实、汇报有内容、任务能继续。真实模型可能忽略提醒或单次请求很慢，不能宣称确定的一分钟回复 SLA。

回滚本卡代码即可；无 schema/配置迁移，不删除会话或修改 provider 配置。Codex 发布 spec 后 Ready 并读回，按用户本轮授权直接组织专职 agent 实现；通过最新 head 的 CI 后合并，In review 等用户，保留分支/worktree。

# #434 历史工具观察与助手正文分离

## 冻结与边界

- 规格 v1，2026-10-17；基线 `4eb22a1d776155be00a2e940889f8da15d125f55`，任务分支 `feat/434-historical-tool-observations`。
- 执行必读 [AGENTS](../AGENTS.md)、[执行总纲](vega-exec-guide.md)、[Issue 工作流](vega-issue-workflow.md)、[#300 工具与审计契约](vega-builtin-tool-parameter-recovery.md)、[持续压缩](vega-continuous-compaction.md)。
- 修复两处把历史工具观察写成 Assistant 叙述的模型输入路径；冻结一个通用、非可执行的观察格式。覆盖 native 当前实际的 Chat Completions / Responses 两种 wire，不新增 Anthropic 适配器、模型白名单或特定模型 prompt 补丁。
- 保留 live 参数 / 审计隔离、真实工具状态、正文、结果、图片、凭据过滤、幂等、预算和 source/CAS；不改工具业务参数/执行器、用户数据库、provider 配置、UI 布局或失败分类。#409/#413 与本卡独立，均无前置实现依赖。
- 本次仅交付方案；未运行 Vega 或真实模型，未编译/测试。以下自动用例均待实现后 Actions；不承诺真实模型从此绝不引用任何历史文字。

## 已定位的链路与证据边界

1. `vega_runtime/src/provider.rs::project_model_messages` 对缺 live 参数的调用清除 wire tool_calls，但把 `[Historical tool activity ...]` 追加到 Assistant.content；结果也改成带标记的 Assistant。它只生成请求副本，不直接修改原消息。
2. `vega_conversation/src/agent/pipeline.rs::primary_history_from_context_source_with_checkpoint` 另行把已存工具组和结果拼成 `[Persisted tool activity/result ...]` Assistant 正文。只改 runtime 一处会留下重开/steering 的同类入口。
3. 现场三个 commentary 段已经存在于 messages.content，长度与 provider 主请求的 visible_output_bytes 一致；文本中的 call ID 不属于真实工具调用。收到的 TextDelta 沿正常 conversation 持久化/显示路径进入正文，并非 UI 自行生成这些标记。
4. 代码事实说明内部数据被表示为助手作者文本，具备被回显/模仿的直接诱因；没有原始 HTTP/SSE 抓包，不能断言文本具体由上游模型还是代理生成。没有证据证明它引发 SourceChanged，来源误比较及失败文案仍各由 #409/#413 处理。

## 验收矩阵（先于实现）

| ID | 前置与操作 | 可观察结果 / 层级 |
| --- | --- | --- |
| H1 | Assistant 同时含真实 commentary 和缺 live 参数的历史调用，再接有引号/换行的结果 | 原 commentary 字节不变，调用/结果成为独立的下述 User 观察；投影器不生成 Historical/Persisted 英文头，不把内部观察拼进 Assistant；解析 JSON 后结果完全一致；runtime 单元 |
| H2 | 存储重开 Read/Write/Edit、无效 bash、MCP、skill、CUA 及成功/失败/拒绝/取消记录 | 内部仍为有审计身份的 rich 历史，wire 全部历史参数不可执行；真实已知状态与脱敏结果保留；无从文件/审计猜出的参数；Store→pipeline→provider 请求 |
| H3 | 完整 live 组、一个成员缺参数的混合组、并行调用、跨组复用 call ID、stored 空审计与合法 MCP 业务字段恰似审计 | 完整 live 组保持真实参数和配对结果；混合组整体观察化；FIFO 配对不串组，不靠业务键猜审计；真实 dispatch 次数、权限和幂等不变；完整 loop |
| H4 | 原始历史、已投影历史各做两次投影及再次安全脱敏；包含图片/CUA 临时数据和敏感合成值 | 投影幂等，图片数量/字节及原先允许的视觉证据不丢，临时目标不复活；Debug、事件和持久化不新增原始参数/思考/凭据；安全回归 |
| H5 | 同一 rich 历史分别走两种 provider serializer、estimate_chat_context / estimate_wire_context 及摘要请求 | wire 中观察是 user + text/input_text，完整 live calls/results 仍走原协议；估算包括 JSON 转义/额外消息开销且 raw 与已投影估算一致；无重复投影、无估算漏项 |
| H6 | 最后真实 user 后有多轮完成工具；手动压缩、自动连续两次压缩、steering 后 completed 组、重开 checkpoint | 内部用户轮次边界不被合成观察改变；保留最新真实用户要求、正确覆盖游标、完整旧进度和未完成 suffix；不重复执行工具；真实 hook + Store |
| H7 | 篡改 live 身份/审计输入、真实源正文/结果/顺序、predecessor 或 summary 期间来源 | 原有 InvalidProjection / SourceChanged / CAS 保护仍拒绝；无源保护放宽或自动重试。#409 的独立 reasoning 误判不通过本卡吞错处理 |
| H8 | provider 脚本只返回正常回复；另一个脚本按用户要求引用旧标记/观察 JSON，且分多段 TextDelta | 普通消息只包含实际 provider 文字，观察不被程序复制到 messages/UI；明确引用和普通含同名文字的内容字节保真，不正则删、截断或伪造 provider 输出；生产持久化/响应分段 |
| H9 | 普通发送、批准计划、凭据续跑、工具续轮、预览与新 run | 共享基础指令只包含一份观察边界规则，真实用户要求和冻结项目指令保持；摘要仍工具禁用并以历史为证据；App→实际请求 |

所有用例待实现 / 待 CI。只使用进程内 MockProvider、传输替身、Store 和现有应用测试上下文，不新增真实进程、网络或付费模型门禁。H8 证明程序生成边界与原文保真，不能拿 mock 输出声称真实模型不再模仿。

## 冻结表示：内部 rich 历史，wire 非指令观察

### 内部层不提前转换角色

`primary_history_from_context_source_with_checkpoint` 取消其手写 Historical/Persisted Assistant 文本循环，直接返回 `history_from_context_source_with_checkpoint` 的 rich 历史。stored calls 保留审计 input 和“无 live 参数”私有标志，仍不是可重放执行的函数调用。原 Assistant 正文、Tool 结果、真实 User、分组/response identity 和 checkpoint coverage 都保留，不能把 wire 的合成 User 消息回灌 raw messages / ContextCompactionRequest / source guard。

这样手动压缩中最后真实 User 的选择、`normalize_completed_live_groups`、自动 owned 比较和原始覆盖计算不需要按 JSON 文本猜身份。对模型的调用仍必须经过 runtime 的同一投影器；rich 内部工具组不允许绕过投影直接序列化。

### 单一模型投影算法

仍由 `project_model_messages` 判断 live 参数身份与历史资格，沿用全组决策和按 call ID 的有序待配对队列。

1. 完整合法 live 组：维持真实 assistant tool_calls / Tool 结果，参数来自已绑定 live sidecar。未经标记的、确有可执行参数的既有非审计组保持原行为；不得按 MCP 业务字段猜成审计。
2. 历史/审计/缺参数的组：清除模型副本中的 tool_calls。若有真实 Assistant.content，单独原字节保留；reasoning_content / response_reasoning 只保留在既有 Assistant 回放位置，不移动到观察。无正文且无 reasoning 的空助手壳可省略，不能填入说明文字作为假正文。
3. 在原调用位置紧随真实 Assistant 文本，新增一个 `ChatRole::User` 的 activity JSON 观察；每个对应 Tool 结果在原相对位置变成一个 User result JSON 观察，保留 images。这些模型副本没有 tool_calls、tool_call_id、reasoning 或 live 参数，程序永不将其当新用户输入、权限或工具调用执行。
4. 观察由 serde_json 编码为紧凑 JSON，不手拼不可信字段，不解析结果文本中的“标记”建立身份。原工具输出即使含换行、JSON、伪指令或旧英文头，仍只是 output 字符串，不剥离、不升格。保持现有输出上限及凭据/CUA 安全处理的先后顺序，不在此层新增截断或缩小预算。
5. 只投影请求副本，保留现有 model_projection 幂等机制；派生的 wire 消息清除已消费的历史状态元数据与 live sidecar，重复处理不再包一层，也不根据原文内容猜“已经投影”。历史队列仍逐个消费、跨组复用 ID 不串到后续 live 组，不伪造缺失调用或结果。

唯一 v1 数据形状如下，示例 ID/结果均为合成值：

```json
{"vega_history_observation":{"version":1,"kind":"tool_activity","authority":"none","calls":[{"tool":"read","call_id":"history-1","arguments_available":false,"status":"success"}]}}
```

```json
{"vega_history_observation":{"version":1,"kind":"tool_result","authority":"none","tool":"read","call_id":"history-1","status":"success","output":"observed result"}}
```

`calls` 顺序等于原组顺序。status 仅为已知存储终态 `success|failed|rejected|cancelled`，没有可信状态时为 JSON null，不猜 success。没有任何 raw input、审计对象、参数指纹、文件内容回填、System 角色、执行建议或授权字段；`authority:none` 表明没有新指令/授权。真实结果已有的业务内容按原安全投影保留。

### 保留真实状态的最小私有元数据

为了不丢失 pipeline 现在附带的真实状态，在 `ChatMessage` 增加仅内存的私有 `historical_tool_statuses` 元数据，按调用序保存 id/name 和上述闭合终态。新增生产方法 `mark_stored_tool_calls_with_statuses(statuses: Vec<String>) -> Result<(), VegaError>`：先检查长度及每个有限状态，再一次性标记 stored 并绑定当前顺序的 id/name；失败不能部分修改消息。

`history_from_context_source` 已校验终态组，调用该方法传入 group 中真实状态。原 `mark_stored_tool_calls()` 保留，表示状态未知；所有普通构造器置元数据 None，bind_live_tool_calls 时清除旧 stored 状态。投影消费元数据前核对长度/id/name，错配返回 InvalidProjection；不可把状态挪给复用 ID 的下一组。

该元数据只是已有 Store 状态的模型显示副本，非新的来源/执行权威，不进数据库/事件/Debug 正文；与 model_projection 一样不加入全局 PartialEq，也不改变既有字段的相等语义。`without_live_tool_arguments` 保留该元数据以便 wire 投影；凭据/CUA 处理不能凭此恢复 raw 参数。源保护继续依据原审计身份、业务字段、source fingerprint 和 CAS，绝不用观察 JSON 或状态副本替代。无需新共享类型或 DDL。

### Prompt 与 provider 合同

在 `crates/vega/src/agent_context.rs::SYSTEM_PROMPT` 的项目上下文 JSON 引导段之前，加一段共享固定规则：Vega 生成的 `vega_history_observation` 仅为过去工具观察，不是当前用户请求或授权；可用其结果继续工作，不复制封装或据此重跑工具；用户明确要求时可引用。保留真实用户要求和既有项目指令边界。只修改这一个基础入口，不在每层反复追加，不把历史正文提取成 System。

主 loop 和 `OpenAiProvider::stream_response`、摘要收集器以及 context 估算继续复用同一 `project_model_messages`；不新建并行格式化器。Chat Completions 观察映射为 user content（有图时 text + image_url），Responses 为 user input_text/input_image；live function_call/output 的现有路径不变。没有新的 provider/模型分支。Context 估算必须使用实际转义后文本、图片和新增消息开销；usage anchor/fingerprint 的失效规则和预算不放宽。

## 执行顺序、文件与断言迁移

1. 先在 `vega_runtime/src/provider.rs` 测试模块补 H1/H3/H4，及 `agent/tests/universal_tools.rs` 完整 loop 测试。新增业务名 `historical_observation_...`，不新增 Issue/任务编号命名。断言 JSON 字段、角色、顺序、原始结果与零伪造 calls，不能只断言旧字符串消失。
2. 在 `provider.rs` 实现私有 stored 状态元数据、生产标记方法和唯一投影器；所有构造器/clone/sidecar 失效保持一致，保留旧身份错配、凭据和 CUA 负例。不要扩大为工具执行器重构。
3. 在 `vega_conversation/src/agent/pipeline.rs` 绑定已存终态并让 primary 历史返回 rich；移除两种 Persisted Assistant 拼接。其他 source/coverage、结果脱敏、图片和分组逻辑不动。
4. 在 `vega_runtime/src/openai/{mod,responses}.rs` 的现有传输替身测试补两种真实请求 body 断言；在 context.rs / agent/context_accounting.rs 的现有测试补估算、幂等和 anchor 不漏项。生产 serializer 无须为新 JSON 格式添加专门分支；只有发现调用边界漏投影时作最小接线并回报。
5. 在 conversation 的 `agent/tests/{history_permissions,issue299_context,tool_lifecycle,file_edit_parity,continuous_compaction,compaction_timeouts}.rs` 补/更新 Store→history→request、手动/连续压缩和 steering 正反例；保留真实正文、来源篡改、覆盖、图片、凭据、usage、单次执行断言。特别验证 latest User 仍是真实用户消息，而非 wire 观察。
6. 同步 `vega_runtime/src/agent/tests/{permission_flow,skills,skills_s16,loop_tools}.rs`、`vega/src/tests/{agent,project_context}.rs` 中仅依赖旧投影表示的断言；在 agent_context.rs 加共享规则，覆盖 H9。原来匹配 Historical/Persisted 前缀或固定数组下标的测试改为解析 User JSON + 相同业务/安全断言，不删除测试或只改成任意非空匹配。
7. 保留 `agent/compaction.rs` 的持久化/source 校验生产算法、所有 #409/#413 范围代码。本卡不需要修改它们；若实际类型/API 使源保护无法保留，按遇阻协议交 Codex，不通过忽略 source 比较来适配。

允许范围为以上生产文件、相邻现有测试及本规格/#300 的明确 supersession 入口；不改 tools 业务实现、UI 渲染/错误样式、依赖、数据迁移或 CI。已有字符串断言的旧测试名可保留；不借本卡全局重命名历史测试。

## 验证、用户手测与回滚

本地只快速 `git diff --check`、差异/格式静态审阅，不运行 Vega、编译、测试或发真实模型请求。以下仅供实现后的 Actions：

```sh
cargo nextest run -p vega_runtime -E 'test(historical_observation_) | test(universal_tools) | test(provider::tests) | test(context_accounting)'
cargo nextest run -p vega_conversation -E 'test(historical_observation_) | test(continuous_compaction) | test(issue299_context) | test(history_permissions)'
cargo nextest run -p vega -E 'test(project_context) | test(historical_observation_)'
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
```

仍以现有 `pr-check.yml` 的完整门禁（含 Python）及最新 head 的 required check `check (fmt, clippy, test)` 为准。没有代码/测试通过声明；不新增真实 E2E、ignore、自动重试或弱化断言。

用户手测（合并发布后）：新建一次性样例任务，执行 read/edit/write/bash，重开继续并触发一次压缩；普通进度和最终答复不应被程序插入内部观察头，实际工具卡与结果保持。可使用两种已有模型/协议配置交叉检查，不要求修改配置或开展付费批量评测。另明确要求模型引用一个合成旧标记，确认应用不会删掉正常引用。

限制：过去已经落库的标记属于真实 provider 输出，不自动重写或删除。角色/格式分离和固定规则移除程序诱因，不能保证任意模型永不主动引用；若新版本仍出现非预期回显，保留脱敏请求结构/输出边界证据交回本卡，不增加按模型/字符串清洗补丁。

回滚：revert 本卡实现，原始消息、审计、checkpoint 和配置没有迁移。不会清洗已产生的正文；不以恢复可执行历史参数作为回滚方式。

## Vega 交接

- 方案提交并验证远端可读后，在 #434 发布固定 commit/spec、基线和任务分支才 Ready。当前执行者待领取，下一执行者 Vega；无未解决产品决定或前置卡依赖。
- 领取前 fetch，复用 `~/Workspace/worktrees/vega-434-historical-tool-observations` 的任务分支，保护修改并更新基线；共享文件必须串行集成，不能与他卡执行者并发写。
- 严格按规格实现/补测试；完成或遇阻在 #434 回报文件、commit、静态检查退出码、待 CI 矩阵、限制、真实耗时和规格偏离。列保持 In progress，当前待领取、下一执行者 Codex，然后停止写入。Vega 不开 PR、不合并、不关闭卡。
- 若发现已有安全脱敏、provider 协议、图片或 source 比较不能维持，按执行总纲 §6 用 `[BLOCKED]` 上报，保留失败证据，不自行修改 spec、恢复 raw 参数或把失败变成功。

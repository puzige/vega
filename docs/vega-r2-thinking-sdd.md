# Vega R2 Thinking SDD

**版本** v0.2 · 2026-09-05 · 状态：实现冻结（R2）

**来源**：`R2-REQUEST-REVIEW.md`、`R2-IMPLEMENTATION-CONTRACT-DRAFT.md`、`ZCODE-UI-REFERENCE.md`。

本文冻结 R2 的 provider/model thinking 能力、应用级偏好、请求映射、运行内 reasoning 传递及并发保存边界。本文不新增 provider 协议、不读取真实凭据、不发送真实 provider 请求，不修改 SQLite schema。

## 1. 目标与不变量

R2 必须让 Settings 中选定的 provider + model thinking 选择真正进入 Chat Completions 请求。能力和偏好以精确的 `(provider, model)` 对为键，能力可编辑，偏好在保存成功后成为下一次运行的权威值，并可在重启后恢复。

以下不变量冻结：

1. 运行开始前解析一个不可变 `FrozenReasoning`；同一 run 的所有工具轮和 HTTP retry 复用同一快照。运行中 Settings 的修改只影响后续 run。
2. `ProviderDefault`、`Disabled`、`Unsupported`、`Unknown` 是不同状态。未知能力显示“提供方默认”，不能显示为 off，也不能猜测 wire 字段。
3. GLM-5.3/GLM-5.3-FLASH 的标准 API 只声明 `low`、`high`、`max`；不声明 disabled。Coding Plan 映射不适用于 CPA。CPA/custom endpoint 必须显式声明协议和能力。
4. 原始 GLM `reasoning_content` 仅在运行内存中有界保留，并在需要继续工具轮时按原序完整回传。它不进入 SQLite、Debug、日志、成本/用量估算或可见正文。既有 `ThinkingDelta` 事件契约保留；UI 只能把它作为思考状态处理，不能渲染原文。
5. R1 thread model durable 选择、pending、owner、route、generation 及失败恢复守卫保持不变。thinking 偏好不增加 thread 列。

## 2. 持久化作用域与格式

新增由 `vega_store::paths` 管控的独立、带版本的 `reasoning.toml`。独立文件确保 reasoning worker 不会用 stale `AppConfig` 快照覆盖 `config.toml` 中的 provider/default/permission/model 设置；不新增依赖或 DDL。

逻辑记录按 `(provider, model)` 精确匹配。每个 profile 同时保存协议声明、能力和偏好，避免同名模型跨 provider 串用：

```toml
version = 1

[[profiles]]
provider = "zhipu"
model = "glm-5.3"
protocol = "zhipu_chat_completions"
support = "required"
efforts = ["low", "high", "max"]
supports_disabled = false
preserve_reasoning_content = true
preference = "max"
```

实现内部使用 typed enum；文件中的字符串由严格解析和校验转换。缺失 profile、缺失协议或未声明模型解析为 `Unknown`：Settings 显示“提供方默认”，请求省略控制字段。`preference` 只允许该 profile 已声明的选择；GLM-5.3/FLASH 不能保存 `disabled`。

## 3. 配置读取、保存与并发

Settings 与 composer 均通过共享的 `ReasoningSettingsController` 发起保存。controller 持有 authority、draft、generation、operation id 及精确 profile key；保存 pending 时阻止依赖该选择的 run 发起。

worker 的保存步骤：

1. 重新读取当前 `reasoning.toml`，只对目标 `(provider, model)` 的目标字段应用 patch，不写回旧的整份配置快照。
2. 使用唯一临时文件，完成 flush/fsync 后，在 rename 前比较目标文件的 fingerprint。
3. 若 fingerprint 未变，原子 rename 并同步父目录；成功 ack 必须携带 operation/generation/profile identity。
4. 若目标文件在读取后发生变化，先以读取时快照、当前版本和 candidate 做一次确定性的三方合并。只要外部修改与 candidate 字段不相交，就把精确 patch 合并到当前版本；同一 profile、同一字段冲突时拒绝本次写入，外部版本获胜并 reload。不得盲目覆盖或无限重试。

fingerprint 检查到 rename 之间仍存在非合作外部写者的竞态窗口；实现可以检测已发生的外部修改，但不能宣称对该窗口提供无损 CAS。进程内的所有 reasoning 写入必须通过同一个共享保存协调器串行化，不能由多个 controller 各自 single-flight。独立 `reasoning.toml` 不参与 `AppConfig` 写入。

保存失败时以真实磁盘 readback 作为下一份 authority；若 readback 不确定则清除
authority 并 fail-closed，同时保留失败 draft。晚到 ack 若不匹配
operation/generation/profile/route 则丢弃。Settings 关闭、切任务、切模型或刷新后，旧回调不得改写新投影。

## 4. 请求类型与运行冻结

跨 crate 的共享选择和投影类型放在 `vega_conversation::types`；`vega_runtime` 保持 headless，不能反向依赖 conversation。runtime 使用自身的 `FrozenReasoning`/`ReasoningSelection` 运行类型，并通过既有 conversation 边界接收，不能在 store/runtime 与 conversation 间形成循环依赖。store 只负责持久化表示及转换。

`AgentRequest` 和 `ChatRequest` 携带冻结选择。worker 使用 app/controller 传入的
owned config snapshot/path 重新确认当前唯一 provider，并在构造 provider、读取
Keychain 或发送前同时校验 frozen `(provider, model)` 与真实 `(provider, model)`；
缺失、无效或发生 owner 变化都以 typed error fail-closed。测试复用同一 owned seam，
不读取宿主用户配置。`ChatRequest` 的 retry 使用同一个不可变 request，不能在每次 attempt
重新读取 Settings。

带工具调用的 assistant round 维护有界 reasoning accumulator。收到 `ThinkingDelta` 时追加原始字节；flush assistant tool-call message 时保存完整 `reasoning_content`，下一轮按原顺序发送，随后才发送 tool result。普通最终 assistant 文本仍只保留可见 content；reasoning 不进入最终正文或持久化消息。

## 5. Wire 映射

当前只实现 Chat Completions：

| 冻结选择 | 已声明能力 | wire 行为 |
|---|---|---|
| `ProviderDefault` | 任意 | 省略 `thinking`、`reasoning_effort`、`clear_thinking` |
| `Effort(x)` | `x` 在 profile `efforts` 中 | 只发送该协议声明的字段和值 |
| `Disabled` | `supports_disabled=true` 且有明确 disabled 操作 | 发送该协议的真实关闭操作 |
| `Unknown`/未声明 | 任意 | 只允许 provider default，省略控制字段 |

OpenAI-style endpoint 只在 profile 声明的合法子集发送 `reasoning_effort`，不能把 `none` 当成通用关闭值。GLM 标准 API 的 enabled effort 发送 `thinking.type=enabled` 与声明的 `reasoning_effort`。同 run 内存 reasoning 回传不自动发送 `clear_thinking=false`；跨 run 保留模式若未来需要，必须是独立显式配置。GLM-5.3/FLASH 不提供 disabled 选项。CPA 不自动套用 GLM 或 Coding Plan 映射。

assistant tool-call message 的 `reasoning_content` 必须逐字保留并序列化；Debug 实现只输出存在性/字节数，不输出正文。

## 6. Reasoning 有界策略

计数单位为 UTF-8 bytes，并使用 checked arithmetic：

- 单个 SSE reasoning delta：`64 KiB`；
- 单次逻辑 provider call / assistant turn：`256 KiB`；
- 单个 run（跨全部工具轮）：`1 MiB`。

任何上限超出均返回 typed `ReasoningBudgetExceeded`，取消当前流并进入失败收口。不得静默截断、继续发送不完整 `reasoning_content` 或把部分内容称为完整保留。预算覆盖 retry/tool round 的运行内 accumulator；不得因 UI 或 Debug 投影复制原文而扩大预算。

## 7. 验收设计

验收不访问真实 provider、Keychain 或网络 API。

### E2E-REAL app/controller

使用 owned temporary config/data/DB、真实 Settings/controller、真实 run 入口和 `MockProvider` provider 边界：

1. 编辑精确 provider/model 的 capability 和 preference，保存成功后关闭、重建 controller、重启恢复；并发修改其他 profile/字段不得被覆盖，同字段冲突要 reload 外部版本。
2. pending 保存期间不得发起 run；保存失败保留旧值；切模型、切任务、切 Settings route 后晚到 ack 不得污染新 UI。
3. MockProvider 脚本两轮工具调用，断言所有轮次使用相同 FrozenReasoning，第二轮 assistant message 含第一轮完整、按序的 reasoning_content；可见文本、持久化消息、Debug、成本估算均不含原文。

### Loopback HTTP

使用真实 `OpenAiProvider` 和 loopback scripted SSE/HTTP server 捕获 body：

1. Provider default 省略控制字段；支持的 effort 发送精确值；显式 disabled 只发送已声明的真实关闭操作；GLM-5.3/FLASH disabled 与未声明值 fail-closed；unknown/CPA 未声明协议省略字段。
2. retry 的每次 body 相同；两轮 tool request 逐字回传原 reasoning_content。
3. SSE reasoning 分片超过 64 KiB、单轮超过 256 KiB、全 run 超过 1 MiB 均得到 typed failure，不能发送截断内容。

保留既有 `ThinkingDelta` parser/事件与 redaction 测试；不为 R2 删除或改名。性能 bench/soak、真实 provider dogfood、权限和 `@file` 逻辑均不在本卡。

## 8. 文件归属

- store：`crates/vega_store/src/reasoning.rs`（版本格式、严格 codec、读取、atomic write/field merge）；不改 migrations。
- shared boundary：`crates/vega_conversation/src/types/reasoning.rs` 及 `types/mod.rs`；pipeline/entry 只传递冻结选择，不持久化 reasoning 原文。
- runtime：`provider.rs`、`agent/mod.rs`、`agent/loop_.rs`、`openai/mod.rs`；保留 `openai/sse.rs` 的 `ThinkingDelta` 契约。
- app/controller：`vega/src/app_agent.rs`、`window/agent.rs`、`window/mod.rs`、`window/reasoning.rs`、`window/session.rs`、`window/render.rs`；共享保存协调器和 run snapshot 必须复用 R1 owner/generation/route 守卫。
- UI：`vega_ui/src/settings/*` 与 conversation composer 相关模块；只维护 typed projection 和请求事件，不直接读写文件或 SQLite。
- tests：store codec/field merge、runtime loop/wire、app/controller E2E；使用 mock/loopback，不使用真实 provider/Keychain。

## 变更记录

- v0.1 (2026-09-05)：冻结独立 reasoning.toml、精确 provider/model profile、显式协议能力、run snapshot、reasoning 有界预算及并发保存边界。

## v0.2 主 Agent review 裁决

- 运行内 reasoning_content 回传与 clear_thinking 模式相互独立；本卡不由内存回传开关自动发送 clear_thinking:false，也不宣称跨重启/跨run推理历史保留。
- 未声明/Unknown 协议不得声明关闭或推理原文回传等矛盾能力；未启用回传时工具轮不附加 reasoning_content（包括空字符串字段）。保持既有ThinkingDelta事件及预算计数。
- FrozenReasoning 的模型必须与实际 ChatRequest/AgentRequest 的模型一致；不匹配和非法选择在发送前拒绝。
- OpenAI-style显式合法子集可包含 xhigh；通用词法表不能遗漏它。GLM5.3/Flash标准API仍限low/high/max，不支持disabled。none只走明确Disabled语义。
- 所有进程内保存经共享协调器串行化，外部改动检测后的字段合并不等于原子CAS；最后检查至rename的非合作外部写者竞态仍存在。
- 已成功写入磁盘不因关闭Settings而撤销；过期ack仅不得回写错误UI，后续重载应恢复真实磁盘权威值。
- run worker 的 owned config owner 校验必须同时匹配 provider 与 model，且在任何
  provider construction/Keychain/HTTP 前拒绝 A→B 同模型的陈旧 profile；测试必须经过
  同一实际 worker seam。
- Provider `SettingsSaved` 在 reasoning save pending 时只记录一次 refresh intent；
  reasoning ack 后再启动 catalog reload。失败 ack 的 typed error 与精确 draft 穿过
  这次 reload，fresh catalog 不能发布 Loading/Ready 清除它们。
- 任一 catalog invalidate/start 都必须先把仍可见的 Settings projection 切到
  `Loading`（或在无法启动 worker 时立即发布当前错误 Ready），使新 generation 与
  view 状态一致；在 worker 返回前的编辑不得产生携带旧 generation 的 Saving。Loading
  不清除失败 draft，fresh authority 返回后恢复 held typed error。
- v0.2 (2026-09-05)：补充协议省略、精确模型绑定、合法档位和保存权威状态边界，保留v0.1其余规范。

## v0.3 测试可观测性补充

worker spawn 语义的测试证据必须来自真正的 `run_agent_worker` entry，而不是
controller 尝试创建线程前的计数。`VegaWindow` 在 `cfg(test)` 下持有自己的
`AgentWorkerStartProbe`，真实 worker closure 将该 handle 传入，worker entry 的第一步
递增它。pricing 与 model-selection 的阻断断言读取各自 controller 的 probe，因此
并发 gpui 测试不会污染零 spawn 或成功一次 spawn 的证据；每个 `MockProvider` 的
request 计数仍独立断言零/一请求。生产构建不包含该 handle，也不使用进程级测试计数，
测试套件无需串行化且不删除原有断言。

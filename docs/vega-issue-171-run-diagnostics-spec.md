# Issue #171 — 运行时间线与失败诊断规格

**状态：** 已冻结，实现契约  
**Issue：** [#171](https://github.com/puzige/vega/issues/171)（保持 OPEN，等待桌面验收）  
**基线：** 2026-09-26 当前 `origin/master`（含 #146）

## 目标与边界

持久化一条脱离对话正文的运行时间线，能回答“哪个 run 的哪次 provider、压缩或工具阶段开始/结束/失败、使用了哪些安全的数值元数据”。重启后可按 thread/run 读取。诊断写入失败不得更改任务原有的成功、失败或取消结果。

诊断表和导出对象只保存 allowlist 元数据。Vega 原有 `messages`、`tool_calls`、`context_checkpoints` 仍按现有功能保存内容；#171 不把这些内容复制到诊断，也不能把整个数据库或原始 tracing 文本作为“诊断导出”。

本卡交付存储、typed read API 和一个**仅由显式用户请求调用**的 allowlist export API；不包含诊断查看器 UI、自动导出、完整请求/响应导出或真实 provider 验收。Reader/Exporter 只在调用方发出显式请求时工作，不自动写文件、复制到剪贴板或发送网络。

## 当前代码路径与缺口

| 责任 | 当前路径 | 观察结果 |
|---|---|---|
| 普通 run 身份与入口 | `crates/vega_conversation/src/agent/entry.rs` | 创建 assistant message ULID，作为消息 ID，也可直接用作普通 run ID；该入口创建 `PersistenceActor`，并发运行 runtime 与 event processor。 |
| Run/tool 事件与必需落库 | `crates/vega_conversation/src/agent/events.rs`、`agent/persistence.rs` | `RuntimeEvent` 包括 tool proposed/running/output/finished、usage、summary status、Finished/Interrupted/Error。现有 actor 使用 bounded channel 和 ack；必需 transcript/tool/usage 落库错误会终止当前运行，不能把诊断命令混同为这些必需写入。 |
| Provider loop | `crates/vega_runtime/src/agent/loop_.rs`、`provider.rs`、`openai/mod.rs`、`openai/sse.rs` | ProviderEvent 已有 TextDelta、Usage、Done/StopReason；runtime 每轮 provider 调用及工具执行发生在循环中。OpenAI adapter 在内部重试，但对外错误只有 status/retryable/message，实际重试数没有结构化返回；成功响应的 response headers 也没有进入 stream metadata。 |
| Provider 错误与脱敏 | `crates/vega_runtime/src/error.rs` | `VegaError::Provider` 保留 raw message，但自定义 Debug/Display 不输出正文；status 与 retryable 安全可见。`VegaError::Tool` 的 Display 会格式化 message，诊断记录不能用 `%error`/`to_string()` 推导分类或直接写错误字符串。 |
| Summary/compaction | `crates/vega_conversation/src/agent/compaction.rs`、`crates/vega_runtime/src/context.rs` | 已有独立的 summary provider 请求、timeout、output truncation、source/projection 校验与 usage；但空 summary 和格式错误都归为 `InvalidSummary`。`ContextCompactionStatusFailure::from_error` 又把 timeout/truncation/format/projection 合并为 `InvalidSummary`，持久化状态表也只有该粗粒度码。 |
| Summary 持久化 | `crates/vega_store/migrations/0009_context_compaction.sql`、`0010_context_compaction_status.sql`、`crates/vega_store/src/context_compaction.rs` | `context_compaction_status` 已是 content-free 的追加生命周期记录，但没有普通 run/attempt 关联；保持现有 UI 状态兼容，新细节放独立诊断记录，不重写历史表/行。 |
| Tool 记录 | `crates/vega_runtime/src/agent/loop_.rs`、`crates/vega_conversation/src/agent/events.rs`、`crates/vega_store/src/tool_calls.rs` | Runtime 事件可识别工具启动和终态；现有 `tool_calls` 表已有 call ID/status/exit code/duration，且另存展示输出。诊断仅引用校验过的 call ID 和安全终态字段，不复制输入、输出或 MCP 工具自定义名称。 |
| SQLite 迁移 | `crates/vega_store/src/lib.rs`、`crates/vega_store/migrations/` | 当前版本已有 #146 的 `0014_execution_duration.sql`（assistant run 总时长）；诊断 schema 必须新增 `0015_run_diagnostics.sql` 并注册于 `MIGRATIONS`，不能改旧 migration。迁移以 `PRAGMA user_version` 递增、SQL 编译期嵌入、逐 migration 事务执行。Store 本身是同步单连接 API；异步工作者需在 blocking 线程使用连接。 |
| Application tracing | `crates/vega/src/main.rs`、`crates/vega_conversation/src/agent/compaction.rs` | 源码已有少量 `tracing` 调用及 `tracing-subscriber` 依赖，但 `main.rs` 未发现 subscriber 初始化。Tracing 不是可靠持久化源。新的日志字段必须是安全 allowlist 元数据，不得把错误/请求/响应正文写进去。 |

## 建议的数据契约

### 身份、阶段与状态

- 普通 agent run 的 `run_id` 使用现有 assistant message ULID；手动、独立压缩操作使用该 operation 的新 ULID。自动压缩是当前 run 的子阶段，不创建脱离 parent run 的 run。
- 每个 run 分配一个 ULID 根 `attempt_id`；每个逻辑阶段调用再分配独立 ULID。根 attempt 的 `parent_attempt_id` 为空；primary model、context summary、tool attempt 均指向根 attempt。Provider adapter 内部重试属于同一个逻辑 provider attempt，单独保存实际 `retry_count`；unknown 保持 NULL，不伪装成 0。
- `phase` 是闭合枚举：`run`、`primary_model`、`context_summary`、`tool`。`state` 是闭合枚举：`started`、`succeeded`、`failed`、`cancelled`、`interrupted`。各 attempt 通过一条 start 记录和至多一条 terminal 记录组成；崩溃后只有 start 的记录表示不完整，不在启动时编造失败原因。
- Provider call、summary call 和工具调用都是不同 attempt；同一 tool call 的 `tool_call_id` 是可选关联字段。工具成功后下一次 provider 请求若失败，两者仍共享 `run_id`，按事件自增 ID 还原先后顺序。
- 时间使用 UTC Unix 毫秒。每个转移事件记录 `occurred_at`；terminal 事件另带单调时钟算出的 `duration_ms`。按 SQLite 自增 `id` 排序，不以可能受系统时钟调整影响的 wall clock 排序。

### 持久化结构

新增 append-only 表命名 `run_diagnostic_events`，字段契约如下：

| 字段 | 约束/语义 |
|---|---|
| `id` | SQLite 自增序号，查询顺序。 |
| `thread_id` | 必填，FK 到 `threads`，`ON DELETE CASCADE`；删除 thread 时同步删除其诊断。 |
| `run_id`, `attempt_id` | 必填且有长度上界；run 与逻辑阶段调用的稳定标识。 |
| `parent_attempt_id`, `tool_call_id` | 可空；父阶段关联与经过字符白名单校验的既有 tool call ID。 |
| `phase`, `state`, `failure_code` | CHECK 约束闭合枚举；失败码由应用定义，禁止写 provider 错误文本。 |
| `occurred_at`, `duration_ms` | 必填 UTC ms；duration 仅 terminal 有值，非负且可安全转换为 SQLite INTEGER。 |
| `stop_reason` | 可空闭合枚举 `end` / `tool_use` / `length`。 |
| `input_tokens`, `output_tokens`, `cache_read_tokens`, `cache_write_tokens` | 可空非负整数，未知为 NULL。 |
| `visible_output_bytes`, `tool_output_bytes`, `tool_truncated` | 可空非负计数和布尔值；永不保存文本正文。 |
| `http_status`, `request_id`, `retry_count` | 可空数值/字符串；重试未知为 NULL。Request ID 仅从 `x-request-id`、`request-id`、`openai-request-id` 三个 response header 获取，并通过严格字符、长度校验，否则 NULL。 |

建议索引 `(thread_id, run_id, id)` 与 `(thread_id, id)`。所有枚举/标识长度/整数边界在 store 写入 API 校验，并由 DDL CHECK 二次约束。读 API 只返回具名 typed struct，不返回原始 JSON、`Debug` 字符串、SQLite row dump 或相连的对话内容。

### 失败分类

Summary 的详细诊断需拆分为：`summary_timeout`、`summary_truncated`、`summary_empty`、`summary_format_invalid`、`summary_projection_invalid`、`summary_source_changed`；Provider 错误保留 `provider_http`、`provider_transport_or_stream`、`provider_protocol` 及 provider rejection 的闭合类别。普通 run 可另有 `cancelled`、`interrupted`、`context_over_limit`、`reasoning_limit`、`tool_failed`、`tool_rejected`、`unknown_safe_failure`。

实现审查补充（2026-09-26）：摘要输入/结果预算超限映射为 `context_over_limit`；结构投影和系统消息错误映射为 `summary_projection_invalid`。已有的摘要前置状态也保留闭合安全分类：`summary_source_too_large`、`summary_aggregate_too_large`、`summary_images_unsupported`、`summary_no_compactable_prefix`、`summary_already_attempted`。`SummaryInputOverLimit` 在调用 provider 之前发生时，仍须产生摘要阶段的 start/terminal 诊断，并证明 provider 未被调用；未知内部错误继续使用 `unknown_safe_failure`。

目前 `VegaError::Provider` 无法在不解析 raw message 的情况下区分 transport 和 SSE/protocol failure。应添加闭合来源枚举（例如 Http/Transport/Protocol），provider 分类只根据枚举与 HTTP status 决定；不得从 message 文本正则猜测。Summary collector 中 empty 与格式 framing 错误需由不同 typed `ContextRuntimeError` 表达。现有 compaction UI/status 可继续映射到 `invalid_summary`，新诊断事件保存细分类。

Usage 仅记录 provider 已报告的 token 数；未报告时为 NULL。`visible_output_bytes` 只累计模型可见文本，不能累计 thinking。Request ID 的输入仅来自 `x-request-id` / `request-id` / `openai-request-id` 三个 response header；值限制为 1..=128 ASCII `[A-Za-z0-9._:-]`，不符合就置 NULL。Tool call ID 限制 1..=128 ASCII `[A-Za-z0-9_-]`，不符合就置 NULL。

### 写入、读取、导出与职责

1. Runtime 在 provider/summary attempt 边界发出 content-free typed lifecycle metadata；conversation 层给事件补齐 `thread_id`、`run_id` 和 parent attempt，并从现有 tool lifecycle event 映射工具终态。Runtime 不依赖 `vega_store` 或 UI。
2. Diagnostics 经 conversation/store 单独的 bounded best-effort writer 写入 SQLite。`try_send` 队列已满、worker 关闭、SQLite 错误、数值转换错误或诊断任务 panic 只导致该条诊断丢失；不得等待诊断 ack、取消 token、改写正常结果、panic 或使必需的 transcript/tool/usage 写入降级。建议与现有必需写入 actor 分离，避免诊断磁盘故障或队列阻塞拖住任务主结果。
3. SQLite 是重启后本机查询的 canonical timeline；tracing 只给开发者即时观察，字段限于随机/opaque ID、phase/state/failure code、duration、usage/byte counts、status、校验后的 request ID 和 retry count。任务面向用户的错误文案保持简短，不展示 raw provider/tool error message。
4. Reader 可按 thread 或 run 读取自增序号排序的事件，并把只有 start 的 attempt 表示为 incomplete。不得从启动时猜测准确 crash 原因。
5. 显式导出只序列化 `run_diagnostic_events` allowlist projection 和 schema version；不得 join/export `messages.content`、`tool_calls.input_json/output_text/output_full_path`、prompt、response、reasoning、凭据、任意 headers、endpoint 或原始日志。没有显式用户动作不得写文件、复制到剪贴板或发送网络。

## 验收矩阵

所有测试使用 MockProvider、进程内 metadata fixture、固定时钟/ULID 或临时 SQLite，不新增真实进程或网络 E2E。只用明显的 fake key 与 canary 文本，不读取用户 DB、不读取真实凭据、不连接真实 provider。

| 场景 | 必须断言 | 脱敏/隔离断言 |
|---|---|---|
| 正常 run + primary model | run 与 provider attempt ID 稳定；记录 start/end/duration、StopReason、已知 usage、可见文本 bytes；重开 DB 后读到相同有序事件。 | DB 新诊断表、typed Debug/Display、tracing/export projection 中无 key、Authorization、prompt、response、reasoning。 |
| provider HTTP 错误与重试 | phase 为 primary model；闭合 `provider_http`、HTTP status、实际 retry count；仅保留白名单且通过验证的 request ID。 | 不保存响应 body、任意 header、raw error message 或凭据。 |
| transport / malformed SSE | typed `provider_transport_or_stream` / `provider_protocol` 区分；已接收的可见 output bytes 与已知 usage 保留。 | 不保存 IO/parser 错误文本、部分 response body。 |
| Summary timeout / length | `summary_timeout` 与 `summary_truncated` 分离；truncation 保留 stop_reason=length、可见 bytes 和已知 usage。 | 不保存部分 summary 或 reasoning。 |
| 空 Summary / 格式错误 / invalid projection | 分别持久化 `summary_empty`、`summary_format_invalid`、`summary_projection_invalid`。 | 不保存 invalid summary/projection。 |
| Summary/provider HTTP 或 transport 错误 | Summary attempt 与其 parent run 关联；provider 类别、status、retry 数按已知情况记录。 | 不保存 summary request/history/provider message。 |
| 工具成功后后续阶段失败 | Tool attempt 成功、已有 tool call ID 可关联；随后 model/summary failure 在同一 run 中排后且准确分类。 | 不复制 tool input/output、完整工具 stdout/stderr 或 prompt。 |
| Cancel / interrupted | run 和活动 attempt 终态为 cancelled/interrupted；存在可计时的 elapsed duration。 | 不存取消异常内部文本。 |
| 诊断队列满、SQLite query-only/写失败、writer 关闭 | Agent 原有成功/失败/取消结果不变；必需 transcript/tool/usage 写入仍遵循现有失败契约；诊断 writer 的 error 不进入 UI run failure。 | 不记录诊断失败的 payload 或 SQLite exception message。 |
| 重启 / 不完整 attempt | 按 `run_id` 重开 Store 后读出完整记录；只有 start 的 attempt 标记 incomplete，不伪造 terminal。 | 只读诊断 projection 不拼接对话正文。 |
| 显式 redacted export | 输出 schema version + allowlist 字段；拒绝无 run/thread identity、拒绝自动触发。 | 在 key、Authorization、prompt、assistant body、thinking、summary、tool input/output canary 中逐个断言不存在。 |

建议新增定向测试模块：`vega_store` 覆盖 migration、schema CHECK、输入边界、按序查询与 reopen；`vega_runtime` 覆盖 provider 类型化分类/summary 子类；`vega_conversation` 覆盖 run-parent-tool 关联、写失败隔离和 canary 脱敏。该规格评审阶段**未运行测试**；实现卡再执行 affected-crate Nextest/Clippy、`cargo fmt --all -- --check`、`git diff --check`，不跑本地 workspace 全量。

## 风险与限制

- Provider 成功 headers 和实际 retry count 当前未暴露；跨 `vega_runtime` / `vega_conversation` 增加安全 metadata API 时，必须覆盖 Debug/Display 与 raw error 隐藏测试。
- Summary timeout/truncation 已类型化，但 empty/framing 仍合并，且 coarse context status 不能作为新诊断失败码来源。
- 必需 PersistenceActor 的 ack 契约不适用于 best-effort 诊断；直接复用现有 ack 路径会让诊断 I/O 影响任务结果。独立 writer 的进程退出 flush 策略还需实现与测试。
- `token_usage` 当前不关联 provider attempt；诊断事件存储 per-attempt 数值可避免修改既有成本账本，但会存一份数值副本。不能错误地把某次 compaction usage 算到 primary model attempt。
- 线程删除后的诊断保留策略未在 Issue 定义；本草案建议 FK cascade，与该 thread 内容一同删除。
- 只存安全元数据并不能证明无泄漏；新增 typed DTO 必须逐个允许字段，禁止 `serde_json::to_value(VegaError)` 或通用 tracing field 抽取。

## 冻结决策

1. 本卡只提供本机 typed read API 与显式调用的 allowlist export API，不增加查看器 UI，也不自动导出。
2. 删除 thread 时 FK cascade 删除对应诊断事件。
3. Request ID 只接受 `x-request-id`、`request-id`、`openai-request-id` 三个 response header；值仅接受 1..=128 ASCII `[A-Za-z0-9._:-]`，其余为 NULL。
4. 诊断 writer 独立于现有必需 PersistenceActor；队列满、SQLite 错误、worker 关闭或转换错误均可丢弃单条诊断，绝不能改变任务结果或阻断必需写入。
5. 只使用 MockProvider、临时数据库、fake credentials 与 canary 文本；不读写用户数据库、不读取真实凭据、不请求真实 provider。

## 实施变更记录

- 2026-09-26：父任务裁决并冻结以上默认边界；以 discovery draft 为基础落入当前实现分支。阶段验收与原始命令输出记录于 [交付记录](vega-issue-171-run-diagnostics-delivery.md)。

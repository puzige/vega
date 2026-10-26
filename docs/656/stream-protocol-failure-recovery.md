# 流内 provider 协议错误被误报为 invalid data，且一次即终止整个 run

版本：v1，2026-10-26。Issue：[#656](https://github.com/puzige/vega/issues/656)。相关：[#543](https://github.com/puzige/vega/issues/543)。基线：`e47afcba`。

## 现象

运行中的会话在没有任何 HTTP 失败的情况下突然以一条红色提示结束：

> The provider returned invalid data. Retry or switch models.

提示出现在流已经正常跑过若干轮工具调用之后。截图里本轮已经完成了 5 段 commentary 与 2 次工具读取，随后直接失败，输入框仍然可用，但整轮任务的进度作废。

## 证据

只读检查 `vega.db`（安装版 0.1.264）：

- thread `01M5WE3Z60971YYRYM9D9WDNZ4`，model `gpt-6-astra`。
- run `01M5WE45VAF0CWC6SM8PHEF7X7` 的 `run` 阶段事件：`state=failed`、`failure_code=provider_protocol`、`failure_source=provider`、`failure_kind=protocol`、`failure_scope=response`、`http_status=200`、`retry_count=0`、`visible_output_bytes=NULL`。
- 该 run 前 7 次 `primary_model` 尝试均 `succeeded`、`stop_reason=tool_use`；`assistant_responses` 有 7 段 commentary，字节区间连续 0→1677；失败那轮没有写入任何文本或 usage。
- 同一 thread 的 run 总耗时 715388 ms（约 12 分钟），失败尝试耗时 40412 ms。
- 跨模型历史：`provider_protocol` 共 36 次（2026-10-17 起），涉及 `gpt-6-astra`、`gpt-6.1-sol`、`deepseek-v4.1-flash`、`step-5-preview`、`gpt-6-luna`；19 条样本中 14 条 `visible_output_bytes` 为 NULL。

`http_status=200` 与 `failure_source=provider` 说明 HTTP 响应已建立、失败发生在流内部，不是连接失败，也不是 runtime 侧 reasoning replay 校验。

Vega 按 `docs/171` 不落盘 SSE 原始帧，代理侧 `usage_events` 为空，因此无法从历史数据反推具体触发分支；这也是本次要把拒绝原因结构化进诊断字段的原因。

## 根因

`crates/vega_runtime/src/openai/sse.rs::SseAssembler::absorb` 把四类互不相同的输入一律判为「协议无效」，而 `invalid_protocol()` 产生的 `ProviderFailureKind::Protocol` 是不可重试的：

1. **任何带 `error` 键的帧**（`chunk.get("error").is_some()`）。包括 `"error": null` 这种普通 chunk，也包括 CPA 在 HTTP 200 之后中途下发的结构化过载帧 `{"error":{"type":"service_unavailable_error","code":"server_is_overloaded"}}`（#543 已单独记录）。上游过载被当成客户端解析错误。
2. **未知 `finish_reason`**。`map_finish_reason` 只认 `stop` / `tool_calls` / `function_call` / `length` / `max_tokens`，其余一律 `StopReason::Unknown`，随后被 `reason == StopReason::Unknown` 直接判死。流结构本身是合法的。
3. **usage 计数不自洽**。`usage_counts` 要求 `cache_read + cache_write <= input`、`miss <= input`，否则整轮响应作废。会计字段是元数据，不是内容。
4. **`responses.rs` 把 provider 错误帧映射为 `Rejected`**，与 Chat 路径不一致，同一上游条件在两条协议路径上得到不同结论。

失败后没有恢复机会：`recovery.rs` 的可重试集合只有 `Transport | StreamInterrupted | Timeout | Http`，且要求 `retryable: true`。`Protocol` 不在其中，于是 12 分钟、7 轮工具成果在一次帧级异常后全部作废。

## 修复

新增失败分类 `ProviderFailureKind::Overloaded`，并把「流内结构化错误帧」与「真正的结构错误」分开：

- `chat_compat::stream_error_frame` 只信任闭合词表里的容量标记（`server_is_overloaded`、`overloaded`、`overloaded_error`、`service_unavailable`、`service_unavailable_error`、`capacity_exceeded`，以及结构化 `status` 为 503/529）。命中即 `Overloaded`；结构化但非容量类错误帧是「供应商拒绝完成」，映射为 `Rejected`；只有非结构化 payload 才仍是 `Protocol`。不读取、不展示上游自由文本。
- `sse.rs`：只有非 null 的 `error` 才视为错误帧；`error: null` 按普通 chunk 继续。
- `sse.rs`：未知 `finish_reason` 降级为最近的可完成终态 —— 已聚合工具调用时为 `ToolUse`，否则为 `End`。这样本轮文本仍会写入 `AssistantResponseCompleted`，`normal_terminal` 保持为真，不会因为一个陌生枚举值丢掉已经生成的回答。真正自相矛盾的重复 `finish_reason` 仍判协议错误。
- `sse.rs`：usage 只作元数据。缺失或不合法的计数不记录，不再作废响应；缓存分项越界时只丢弃该分项，保留 token 总量。
- `responses.rs`：`response.failed` / `error` 走同一套标记判定，`Overloaded` 与 `Rejected` 与 Chat 路径对齐。
- `recovery.rs`：新增 `emitted_visible` 追踪。`Overloaded` 与 `Protocol` 在**本次尝试尚未产出任何可见文本、且未完成**时可重试，最多沿用既有 `recovery_retry.max_retries = 5`；一旦已经显示文本或工具调用，同样的失败保持终态，避免重复副作用。重试时重置 `emitted_visible`。

诊断链路同步：`RuntimeDiagnosticFailure::ProviderOverloaded` → `DiagnosticFailureCode::ProviderOverloaded`（`provider_overloaded`）→ `failure_kind = "overloaded"` → `RunFailureKind::ProviderOverloaded(fact)` → i18n `domain.failure.provider_overloaded`。新增迁移 `0041_provider_overloaded.sql` 扩展 `failure_code` 与 `failure_kind` 两个 CHECK 白名单，其余列、约束、索引与 0038 完全一致。

## 用户可见结果

| 场景 | 修复前 | 修复后 |
|---|---|---|
| 流内上游过载帧，本轮尚无可见输出 | invalid data，整轮作废 | 自动重试（最多 5 次），仍失败则提示「供应商当前过载」 |
| 流内过载帧，本轮已有可见输出 | invalid data，整轮作废 | 提示「供应商当前过载」，已显示内容保留 |
| `"error": null` 普通 chunk | 可能整轮失败 | 正常继续 |
| 未知 `finish_reason` | invalid data，整轮作废 | 本轮正常完成，文本保留 |
| usage 计数不自洽 | invalid data，整轮作废 | 忽略该计数，响应正常完成 |
| 真正的 JSON/SSE 结构错误 | invalid data | 仍为 invalid data（安全兜底不变） |

## 边界

- 不新增 SSE 原文落盘，不记录上游文案；诊断字段仍只用闭合词表。
- 不改变「已产出可见输出后不重放」的原则。
- 非容量类的结构化错误帧（如内容审核拒绝）不会重试，避免无效循环。
- 不修改 `docs/171` 的诊断契约正文，只扩展其闭合词表成员。

## 验证与交付

静态检查、`cargo fmt --check`、迁移 SQL 的本地 sqlite3 执行与 CHECK 拒绝验证、非实现者独立审查、云端 `pr-check`（fmt/clippy/test）。本地不编译、不运行 Vega、不跑 cargo test/clippy，行为与像素验收由用户手测。

回滚方式：还原上述五个文件的改动并删除迁移 `0041`；若已升级数据库，`0041` 只是放宽 CHECK 白名单，旧代码写入的取值集合是它的子集，回退代码不需要回退迁移。

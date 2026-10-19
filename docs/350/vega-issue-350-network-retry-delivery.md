# Issue #350 网络中断自动重试交付记录

日期：2026-10-13。规格：[网络中断自动重试](vega-issue-350-network-retry.md)。本记录包含实现、静态检查和两轮云端检查后的修复；PR Check 尚未通过，真实网络与原生界面手测未执行，不代表功能已验收。

## 实现

- 主会话通过 `chat_stream_recovering` 使用 lazy stream，建立连接失败和中途断流共享五次重试预算，最多六次请求。默认退避为 1/2/4/8/16 秒，`Retry-After` 优先并限制为 30 秒；旧入口仍使用原来的预算。取消、每次尝试的凭据 guard、60 秒建连时限和 300 秒 Provider 事件空闲时限均在该入口内执行。
- Chat Completions 与 Responses 将读取错误和未正常完成的 EOF 分类为可恢复传输故障；鉴权/请求拒绝、JSON/协议错误、Responses 字节预算超限不重试。Chat 显式 `[DONE]` 但没有 `finish_reason` 仍是协议错误。
- runtime 在逻辑 round 开始发布正文边界；重试清除本轮正文、思考、reasoning replay、未完成工具和输入 anchor 的临时状态。冻结请求与较早轮次的工具结果保持不动。已报告 Usage 仍保留；耗尽保留最后尝试片段并输出一次终态错误。
- conversation 验证 UTF-8 边界，在刷新待写 delta 后持久化正文回滚，持久化成功才发布 UI 事件并 ACK。lazy Provider 在 ACK 后才被再次 poll，因此存储失败会阻止后续 HTTP 请求。生产 `OwnerCredentialProvider` 正确代理恢复入口并保留凭据拒绝。
- UI 用当前 message 的稳定 entry 身份、活动段落和思考计数保存 round 检查点，回滚不依赖历史列表的易变下标。仅显示一条 `正在重新连接… n/5`，恢复/结束/失败/中断时移除；仍保留停止操作。回滚只清除当前估算，已计费用量不减少，旧 run 的 retry 不改变当前 run。

没有新增依赖、数据库 schema、设置、成功提示或原始诊断展示。

## 变更文件

| 范围 | 文件 |
|---|---|
| Provider/协议/恢复 | `crates/vega_runtime/src/provider.rs`、`openai/{mod,recovery,sse,responses}.rs` |
| Runtime 事件/循环 | `crates/vega_runtime/src/agent/{mod,loop_}.rs` |
| Runtime 回归 | `crates/vega_runtime/src/openai/tests/{mod,network_retry}.rs`、`agent/tests/{loop_tools,permission_flow}.rs` |
| 会话与持久化 | `crates/vega_conversation/src/agent/{credential_guard,events,persistence,compaction}.rs`、`types/{events,meter}.rs` |
| 会话回归 | `crates/vega_conversation/src/agent/tests/{mod,network_retry,failure_paths}.rs`，凭据 wrapper 回归在 `credential_guard.rs` |
| UI | `crates/vega_ui/src/conversation_stream/{mod,core,content,network_retry,run_activity,run_notice}.rs` |
| UI 回归 | `crates/vega_ui/src/conversation_stream/tests/{mod,issue350_network_retry}.rs` |
| 规格与交付 | `README.md`、`docs/350/vega-issue-350-network-retry.md`、本文件 |

## 自动回归设计与证据边界

新增 18 个 `issue350_` 测试；既有精确事件序列断言显式适配 transient round-start 事件，保留原有工具、取消和失败断言。下表记录初次提交时的覆盖设计与状态，后续实际云端结果见本文的 PR Check 记录。

| 验收 | 回归与断言 | 执行状态 |
|---|---|---|
| R01/R02/R07 | `issue350_setup_and_stream_failures_share_one_recovery_budget` 混合 503、EOF、429 和后续流故障，五次 retry/六次请求，冻结 body 相同，Retry-After 上限30秒；默认与指数退避单独检查 | 待 CI |
| R03 | `issue350_rejected_and_malformed_responses_never_retry`、`issue350_invalid_request_does_not_retry_and_active_request_can_cancel` 覆盖 400/401/403、JSON、显式 DONE 缺结束原因、reqwest builder 错误 | 待 CI |
| R04/R06 | `issue350_transport_fault_and_response_eof_recover_without_replaying_tools` 使用真实 OpenAI 适配器→`run_agent`，Unicode 尾段被替换，失败思考/工具碎片被丢弃，之前 read 只执行一次，两条 Usage 保留 | 待 CI |
| R05 | `issue350_responses_eof_and_transport_fault_retry_but_invalid_streams_do_not`、`issue350_body_transport_errors_recover_in_both_protocols` 覆盖 Responses EOF、两种协议的真实 body stream 类型错误与字节上限拒绝 | 待 CI |
| R07/R08 | `issue350_recovery_exhaustion_and_cancellation_are_terminal`、`issue350_exhaustion_keeps_last_partial_and_emits_one_failure` 断言耗尽、单次终态、末次片段和取消后无后续请求；请求中取消用受控时钟检查 | 待 CI |
| R09/UI | 三个 GPUI 回归覆盖同一重连行原位更新、正文/思考-only/空输出、恢复与结束清除、停止、已计费 meter、不接受旧 run、前一轮工具及 history prepend | 待 CI |
| R10 | `issue350_retry_rollback_is_durable_before_ui_and_preserves_usage_and_tools` 经过真实会话持久化 actor；在 retry UI 回调内读取数据库确认已截断，再检查重载正文、Usage 两行和一次已完成工具 | 待 CI |
| R11 | Provider guard、OwnerCredentialProvider 代理/凭据拒绝、runtime retry sink 拒绝、conversation 注入 retry 落库失败：分别验证零后续 HTTP、无替换 delta 和无未持久化 UI retry | 待 CI |
| R12 | 既有 `issue67_production_routes_keep_one_background_run_and_origin_stream` 覆盖真实 controller 路由切换时保留原活动 entity；结合新增 UI transient 状态及持久化重载回归。没有新增真实隐藏窗口/网络联合测试 | 待 CI |
| R13 | `issue350_silent_setup_and_stream_timeouts_enter_bounded_recovery` 用暂停的 Tokio 时钟检查 60/300 秒边界，无真实长等待 | 待 CI |

两段证据串联：第一段用 Provider 私有进程内 transport fixture 驱动真实 SSE/恢复流→runtime；第二段用脚本 Provider 的 typed 事件驱动真实 conversation/persistence，再用 GPUI 测试上下文验证 UI 回滚与交互状态。没有为测试扩大公共 UI API，没有把手工合成 retry 事件当成 Provider 重试证据；这不构成真实网络端到端验收。

## 本地静态检查

遵守仓库与用户的本地验证限制，只做格式、差异与源码检查。执行目录为本卡 worktree。

| 命令 | 原始结果 |
|---|---|
| `cargo fmt --all -- --check`（首次） | exit 1，输出 rustfmt 格式差异；未执行编译或测试 |
| `cargo fmt --all` | exit 0，stdout/stderr 为空 |
| `cargo fmt --all -- --check`（格式修复后） | exit 0，stdout/stderr 为空 |
| `git diff --check` | exit 0，stdout/stderr 为空 |
| `git diff --cached --check`（包含新增文件） | exit 0，stdout/stderr 为空 |

云端现有 `.github/workflows/pr-check.yml` 门禁执行：

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
```

需要在云端诊断本卡时可使用：

```sh
cargo nextest run -p vega_runtime -p vega_conversation -p vega_ui -E 'test(issue350_)'
```

上述 Clippy、nextest 命令本地均未运行；没有本地编译、构建、打包或安装。不将格式检查标为类型检查或测试通过；云端结果单独记录如下。

## 首次 PR Check 与修复

[PR #353](https://github.com/puzige/vega/pull/353) 的 [首次 Clippy job](https://github.com/puzige/vega/actions/runs/38411505295/job/115344747588) 在编译阶段退出 101：

```text
error[E0599]: no method named `aria_label` found for struct `gpui_kit::Div` in the current scope
error[E0616]: field `stopping` of struct `conversation_stream::composer_actions::ComposerActions` is private
error: could not compile `vega_ui` (lib test) due to 2 previous errors
```

修复遵循现有 GPUI 模式：重连行先设置稳定 message 对应的 `.id(...)`，再调用 Stateful 元素提供的 `aria_label`；停止回归监听现有 `ComposerStopRequested`，验证正确 thread、重复停止只发出一次请求和终态清理，移除对私有字段的访问，没有扩大公共 API。

修改前已执行 `git fetch origin` 和 `git merge --ff-only origin/feat/350-network-retry`；后者原始输出为 `Already up to date.`。修复后 `cargo fmt --all -- --check`、`git diff --check` 均 exit 0、stdout/stderr 为空。首次 CI 的失败事实保留，修复尚待下一轮云端验证；本地仍未运行编译或测试。

## 第二轮 PR Check 与修复

[第二轮运行 38412409609](https://github.com/puzige/vega/actions/runs/38412409609) 对应 head `54a0cfb`：Clippy 成功；Nextest 完成全部计划执行项后退出 100，原始摘要为：

```text
Summary [176.083s] 2496 tests run: 2494 passed, 2 failed, 5 skipped
FAIL agent::tests::permission_flow::invalid_write_is_atomic_validation_rejection_before_any_proposal
FAIL openai::tests::network_retry::issue350_responses_eof_and_transport_fault_retry_but_invalid_streams_do_not
```

18 个本卡 `issue350_` 测试中 17 个通过。三个 GPUI 回归、两个 conversation 持久化回归、凭据 wrapper、双协议 body 断流、五次共享预算、取消、超时及耗尽均通过；5 个 skipped 是既有跳过项，没有新增 skip 或 ignore。上述结果仅属于 `54a0cfb`，不能代替修复后的下一轮门禁。

两项失败的原因与修复：

1. `permission_flow` 原先只过滤诊断事件，新增 transient `AssistantResponseStarted` 占据首项。只把该 transient 事件加入过滤，完整保留“第一条业务事件必须是脱敏后的 Validation 拒绝”及“不得出现 ToolCallProposed”的原安全断言。
2. 连接故障 fixture 从 `response.bytes().await` 提取错误，再误放到 `send_attempt`。锁定的 reqwest 0.12.24 在 `Decoder::PlainText` 中把 body 读取错误包装成 `Kind::Decode`，而真实连接失败由 client 包装成 `Kind::Request`。生产分类无需放宽。fixture 改用 reqwest 的进程内 `Resolve`，显式 `no_proxy`，在 resolver 内直接返回 IO 错误；先断言 resolver 调用一次、`is_connect()`/`is_request()` 为真且 `is_decode()` 为假，再检查 RetryScheduled、两次请求、完整文本、Done 和无错误。没有真实 DNS 查询、socket 连接或新增依赖；body 断流仍由第二轮已通过的双协议流测试覆盖。

修复前再次执行 `git fetch origin`、`git merge --ff-only origin/feat/350-network-retry`，输出 `Already up to date.`。修复后 `cargo fmt --all -- --check`、`git diff --check` 均 exit 0、stdout/stderr 为空。本地没有执行编译或测试，第三轮完整 PR Check 待运行。

## 待验证与风险

- CI 尚需确认跨 crate exhaustive match、类型检查和全部既有回归；失败必须修复后再合并。
- 真实模型服务、断网恢复、浅深主题/窄窗口、键盘停止和隐藏会话恢复由用户合并后手测。没有真实网络、原生像素或外部工具集成通过结论。
- 主恢复入口最多五次重试，会增加离线等待时间和潜在已计费用量；保留实际 Usage，不推算未报告费用。300 秒可解析事件空窗阈值对极长无事件思考仍可能触发重试。
- 无 schema 变更；回滚按规格通过后续 PR revert 本卡提交。Issue/看板流转、PR Check 和合并由主 agent 继续处理。

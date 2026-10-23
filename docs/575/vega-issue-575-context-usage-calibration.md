# #575：响应用量及时校准上下文环

## 契约版本与范围

- v2，2026-10-23；基线 `5a1ec8ad26fa53fd6210a8e58b6ad85e739dd8bb`。
- 用户最新裁决：采用 ChatGPT 本地任务界面的“最近一次调用总 token”口径，取 `usage.input + usage.output`，不是整个任务累计消耗。
- 本版本取代 Issue 初稿中“只取 input”、待定口径和整轮不降的冲突描述。可在同一次调用内防止重复上报回退；不能用任务级最大值掩盖后续调用或压缩后的真实下降。
- 只修 Native 主调用的展示数据链及必要的来源标签；保持现有布局、交互、分母、预算/压缩决策、provider 协议和费用累计逻辑。Codex 后端没有同一 `UsageUpdated` 来源，不新增其适配能力。

## 事实、根因与边界

1. `vega_runtime/src/agent/loop_.rs` 收到 `ProviderEvent::Usage` 后发出 `RuntimeEvent::UsageUpdated`；运行时每个 provider call 只接受一次终结 usage，重复上报/终结后上报由既有协议校验拒绝。
2. `vega_conversation/src/types/events.rs` 将它映射为 `ConversationEvent::UsageUpdated { message_id, usage, .. }`。`TokenUsage` 有 input/output/cache_read/cache_write，没有额外 total 字段。缓存是 input 的组成部分，不再加一次。
3. `window/artifact.rs::apply_agent_batch_ingress` 按 generation/thread/stream 拦截过期批次；`owns_primary_context_event` 还验证本轮 model。事件按通道顺序交付。同一个 assistant message 可包含多次 provider call；`AssistantResponseStarted.response_id` 可区分调用，不能把 message id 当作调用 id。
4. `content.rs::apply_event` 将 usage 交给累计 meter，但上下文分支为空。`context_control.rs::apply_context_accounting` 写的是请求前 predicted_input；投影/压缩状态也可写估算。根因是缺少本次真实总量的展示接线与来源/生命周期管理，不是费用累计错误。
5. usage 不携带 response id/model/thread，不能声称 UI 能独立鉴别任意乱序的同 message 旧调用。使用已有 app ownership 与有序调用边界，不扩大事件协议。测试覆盖可表达的归属边界，实际 provider 时序由用户手测。
6. 同数量纲不代表数值必定相同。真实 usage 可以纠正高估或低估；input+output 也不保证精确等于下一次输入（新增工具结果、裁剪、重放等会改变它）。界面须如实区分预测输入和最近调用总量。

## 验收矩阵（先于实现）

以下数字均为独立调用，不是任务累计。每项须在开发 Actions 或静态审查中记录实际证据，真实系统集成交用户。

| 场景/前置 | 操作 | 可观察结果 | 验证 |
|---|---|---|---|
| 活跃 Native 主调用；预检 160000；输入上限 400000 | `UsageUpdated(input=120000, output=30000, cache_read=80000, cache_write=10000)` | 分子立即为 150000，非 120000、240000 或任务累计；分母保持 400000；首个真实值允许向下校准估算 | GPUI 事件边界 + 静态 ingress |
| 同一调用已有总量 150000 | 重复同量或较小 usage | 同量幂等；同调用不因低值重复回退；不重复累计 | GPUI |
| 同 message 的下一次调用，新的 response id/预检 revision | 新 usage 为 input=80000/output=20000 | 分子为新调用的 100000，不能保留旧调用最大值或相加为 250000 | GPUI |
| 长工具或等待期 | 最近 usage 后没有新请求 | 保留最后一次已知总量，无定时轮询/伪造增长 | GPUI 状态 + 用户手测 |
| 已有真实总量，迟到的同轮投影或旧 accounting revision | 应用投影/旧预检 | 分子不被旧估算盖回；分母原有投影行为保持；新请求的有效预检仍可展示新预测 | GPUI |
| 压缩边界 | 摘要 usage 后应用 `PostSummaryTarget` 的 40000 估算；下一主调用总量 50000 | 摘要 usage 不写分子；环允许降至压缩后的估算，再更新到新主调用总量；旧高水位不泄漏 | GPUI |
| 无效或不归属事件 | input=0；input+output 溢出；无活跃 run；其他 message；模型重置后旧事件 | 状态不变，不显示 0/回绕/饱和的假数据；app 层拒绝旧 generation/thread/stream/model | GPUI + 静态 ingress |
| run 结束/取消；已有有效总量 | 结束 run，投影异步到达 | 同 stream 内最后已知总量保留，不立即退回冷估算；旧 run 的迟到 usage 不再接受；新 run 或模型/会话重置清理所有权 | GPUI |
| 无 usage、缺预算或重启重载 | 接受预检/已有投影；新建/重载 stream | 保留既有估算/未知语义；无预算也不伪造分母；不新增持久化或查询循环 | GPUI + 静态 |
| 来源与可访问名称 | 真实总量、预检估算两态渲染 | 真实值标为“最近调用/总量（输入＋输出）”，不再叫“估算输入”；预检仍明确为估算；百分比/超限/键盘/悬停交互保持，必要容量说明不删除 | display 行为断言 + 静态 UI |

算术核对：120000 + 30000 = 150000；150000 / 400000 = 37.5%；缓存已在 input 内，80000/10000 不另加。下一调用 80000 + 20000 = 100000；它替换而不累加上一调用。`u64::MAX + 1` 不可表示，应保留既有值而非 wrap/saturate。

## 最小实现计划

1. `context_control.rs` 增加最近主调用总量/来源状态；复用现有 active message/accounting/call identity。真实值使用 checked_add；保留分母。区分每次调用、压缩、run 结束/新 run/模型重置，保护异步投影不覆盖已知总量。
2. `window/artifact.rs` 在既有 generation/thread/stream/model ownership 内单独把 `UsageUpdated` 交给 context，再继续由 `content.rs::apply_event` 消费通用事件及累计 meter；摘要 usage 仍不写环。在真正的新调用（`network_retry.rs::checkpoint_response`）/消息边界重置本调用去重/高水位（不是整个任务保留 max），不改变 meter 的独立消费与计费。
3. `context_usage.rs` / `render.rs` 只改必要数据来源传递和短标签，不改圆环/浮层布局与交互，不把真实值冒充预测输入，也不改分母为供应商验证容量。
4. 按本卡状态转换风险添加少量新的开发 GPUI 回归，不恢复历史集合、不扩大 production public API；使用现有 `gpui_kit` test-support 功能，必要独占锁文件项留痕并在合并前清理。Git/index/commit/push 由主 Agent 统一处理。
5. 实现者停写后，非实现者对精确版本独立审查；现有 draft PR Actions 运行开发回归。实际通过后固定原始源码/patch/full SHA，移除开发测试及独占支持；对最终无 Rust 测试 head 跑完整 required CI，成功后 squash merge。Issue 保持开放/In review 等用户手测。

## 验证和非目标

- 本地只做单文件 rustfmt、diff 和快速静态检查；不运行 Vega、不编译、不执行 Rust/Python 测试。自动验证仅交现有 `pr-check`，CI 每 5 分钟查询。
- 不改权限、安全栅栏、provider 处理、runtime 输入预算/压缩阈值/锚定、meter、schema、通用 CI 或真实用户数据。
- 不处理 `context_control.error` 死字段、#359 容量补载、常显百分比/环内数字、模型总容量定义。最近调用总 token 是展示信号，不冒充 runtime 下一请求的安全判定。
- 不声称与 ChatGPT 全机制一致；仅采用其最近一次调用 total 作为响应后展示口径。静态包分析不能证明所有上游发送时机。
- 重启/切走后 stream 被重建时，仍可按现有投影显示估算；本卡不持久化最近调用快照。
- 回滚：撤销本卡生产差异即可，不涉及数据迁移。

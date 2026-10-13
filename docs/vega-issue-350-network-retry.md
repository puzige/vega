# Issue #350：网络中断自动重试

状态：2026-10-13 实施规格。关联 [Issue #350](https://github.com/puzige/vega/issues/350)、A2-19、[Provider 契约](vega-tech-spec-p1.md#41-provider-抽象)。

## 1. 问题与依据

用户要求网络问题发生时自动重试，并参考 Codex 的 UI 和逻辑。Issue 截图显示回复进入失败态；截图没有暴露底层异常，不能据此断定该次失败的具体网络原因。

源码已确认的缺口：`OpenAiProvider::stream_response` 仅在建立请求时静默重试；Chat Completions 的 SSE transport failure、缺少结束标记的 EOF，以及 Responses 中途断流直接失败。`agent/loop_.rs` 收到流错误便结束整个 run，UI 没有重连过程状态。这是静态复现依据，本地没有编译或运行网络复现。

参考 [Codex 开源重试逻辑](https://github.com/openai/codex/blob/816fa702ec66a4c81b204701a03371a1ac3511e6/codex-rs/core/src/responses_retry.rs)：区分可恢复错误、限制重试预算、退避等待、通过事件呈现 `Reconnecting... n/max`。只采用公开行为与源码依据，不解包桌面应用、不复制私有资产。2026-10-13 用户在本卡实施中要求改为五次指数退避重试；不引入无限重试或 WebSocket fallback。

本规格补齐 §4.1 网络恢复行为，并取代旧实现“流建立后绝不重试”的限制；不改变权限、工具执行、凭据保护、上下文预算和真实用量契约。

## 2. 行为契约

### 2.1 重试边界与预算

- 针对内置 OpenAI-compatible Provider 的主会话请求，包括 Chat Completions 和 Responses。
- 一次逻辑模型 round 的首次请求加最多五次自动重试，总共最多六次 HTTP 尝试。建立连接失败、可重试 HTTP 状态和中途断流共享这一预算，禁止 Provider 与 runtime 两层相乘。
- 网络连接/读取错误、超时、HTTP 408/429/5xx、未收到协议成功结束事件的截断可重试。401/403、其他请求拒绝、畸形 JSON/非法工具数据、权限/凭据/存储/上下文错误不可重试。分类来自类型和协议状态，禁止匹配人类错误文案。
- 无服务端建议时依次等待 1s、2s、4s、8s、16s；保留 `Retry-After` 秒数及 30s 上限。算术：首次请求 1 次 + 重试 5 次 = 6 次；无建议总等待 1 + 2 + 4 + 8 + 16 = 31s。等待和每次请求都服从原 run 的 CancellationToken。
- 主恢复入口的单次请求建立等待上限为 60s；已建立流连续 300s 未产生可解析 Provider 事件则判定超时并使用同一预算。只有 SSE 注释/keepalive 不计为模型事件进展。数值选择给长思考保留 5 分钟空窗，避免静默断线永久挂起；不改变辅助调用的超时策略。测试使用受控时钟，不进行真实长等待。最坏全是建连超时时：6 × 60 + 31 = 391s；没有任何有效流事件且每次立即建立时：6 × 300 + 31 = 1831s（不含请求建立耗时）。
- 重试复用同一逻辑 round 的冻结请求、model/reasoning/工具定义与已完成工具结果；每次实际 HTTP 尝试重新执行既有 pre-attempt guard。不能重新启动整个 agent run，不能重新消费技能/桌面状态或重复压缩上下文。
- 非主会话的一次性调用、标题生成、提交信息生成、上下文压缩等保持既有行为和预算；不扩大重试范围。已有 Provider trait 调用方式应保持兼容。

### 2.2 输出、工具与持久化

- 已完成的 earlier round、已落库的工具结果与已执行的副作用保持不动。工具只有在本 round 成功结束、完整校验后才执行，失败请求缓冲的 tool fragments 全部丢弃。
- 当前未完成 round 已显示的文本/思考不可与新请求结果直接拼接。重试时回到该 round 开始的文本边界，并清理本轮临时思考、摘要、reasoning replay 和工具缓冲；下一次输出替换未完成尾段。
- 每个逻辑 round 开始时发送 transient start 事件，记录正文 offset 以及 UI 的 entry/segment/thinking 检查点，覆盖“只有思考、尚无正文”的中断。rollback 通过有序 runtime → conversation → UI 事件和现有 persistence actor 完成；先清空/提交待处理 delta，再持久化回滚后的正文，随后发布 UI reset。边界必须是合法 UTF-8 字符边界，且不能影响更早的已完成响应、工具卡片、截图、选中任务。
- 已实际报告的用量仍按既有计费规则保留，不伪造未知用量；失败轮次不能成为成功的上下文 anchor，不重放已接收的 Usage 事件。
- 成功恢复时仅移除重试状态，不添加成功横幅。五次耗尽后终结 run，保留已有已完成内容和当前失败尝试的可见片段；显示准确、可操作的终态错误，不无限循环。
- 重试等待为内存中的运行态，不新增 schema，不持久化成永久错误；重启仍使用既有 interrupted/recovery 规则，不自动复活请求。

### 2.3 UI

- 在当前回复就近显示一条 `正在重新连接… 1/5` 状态，后续尝试原位更新。沿用共享语义色、排版和状态组件，暂时断线不显示最终失败红条。
- 状态与真实 retry 事件绑定。收到恢复后的有效输出或正常结束事件时清除；完成、失败、取消、切换/清空会话也必须正确清理。隐藏会话继续运行时，回到该会话能得到正确运行状态。
- 重试期间保持 run 活跃、保留现有停止操作；不能把 composer 变回可重新启动同一个 run 的闲置态。点击停止或键盘停止会立即取消等待，不再发后续请求。
- 耗尽或不可重试失败仍提供现有错误/继续对话路径；错误要区分暂时网络问题与最终失败，不暴露原始 endpoint、key、响应正文或诊断堆栈。
- 不新增说明段、设置开关、完成 toast 或重复状态。适用状态覆盖运行、恢复、失败、取消、空输出、浅深主题与键盘停止。

## 3. 先行验收矩阵

自动回归用进程内 Provider/transport fixture 和 GPUI 测试上下文，均交给云端 PR Check。首次本地源码观察列为未运行，不冒充红灯测试。

| ID | 需求/风险与前置状态 | 操作 | 预期可观察结果 | 层级 | 证据/状态 |
|---|---|---|---|---|---|
| R01 | 未出 token 时连接失败 | 注入 transport error 后成功 | 有重连事件、第二次成功、无最终 Error | Provider/runtime | 待 CI |
| R02 | HTTP 临时失败 | 408、429、503 后成功 | 同一预算；429 保留 Retry-After，上限30s | Provider/runtime | 待 CI |
| R03 | 不可恢复错误 | 401/403/400、畸形 JSON | 一次尝试，明确失败，无重连 | Provider/runtime | 待 CI |
| R04 | Chat 已显示 Unicode 文本/思考后断线 | 下一次输出与前缀不同 | 只替换本 round 尾段，无重复/残留 | runtime/存储/UI | 待 CI |
| R05 | Responses 已输出后截断 | 注入 transport error / clean EOF | 与 R04 一致，正常 completed 不重试 | Provider/runtime | 待 CI |
| R06 | 前一轮工具已完成、当前轮有半截 tool call | 当前轮断线后成功 | 已完成工具只执行一次；半截调用不执行 | runtime | 待 CI |
| R07 | 持续网络错误 | 混合建连失败与流失败 | 最多六次请求，五次重试后 Failed | runtime | 待 CI |
| R08 | 正在退避或等待请求 | 取消 token / 现有停止动作 | 立即 Interrupted，无后续请求，状态消失 | runtime/UI | 待 CI |
| R09 | 可见重连状态 | 恢复文本、思考、正常结束或终态 | 状态原位清除，成功不残留错误 | UI | 待 CI |
| R10 | 已落库正文且有待批量 delta | 回滚、下一次成功、重新加载历史 | 存储与可见文本一致，更早段落保留 | conversation | 待 CI |
| R11 | 请求 guard 或存储失败 | 重试前拒绝/落库失败 | fail closed，不发下一次请求 | Provider/conversation | 待 CI |
| R12 | 隐藏后返回活动会话/恢复历史 | 切换会话或重载 | 不把暂时重连变成永久错误，不复活已停 run | controller/UI | 待 CI |
| R13 | socket 静默挂起 | 受控时钟推进建连60s/流空闲300s | 产生可恢复超时；仍可取消，预算不被重置 | Provider/runtime | 待 CI |

用户手测：回复生成时短暂断网再恢复；重连中停止；长时间离线到耗尽；带已完成命令的会话在下一轮断线；浅深主题、窄窗口下状态和停止入口可读可用。

## 4. 实现计划与归属

1. 专职实现 agent 先补 R01–R13 对应的必要回归，再实现 retry 分类、单层请求预算和 round rollback 事件。主 agent 负责规格、审查、集成及发布门禁。采用 Provider 的兼容默认恢复入口；OpenAI 主会话入口返回 lazy event stream，自己拥有单一预算，先发 retry 事件再等待，用 typed metadata 事件传递实际响应信息。已有直接调用/once 入口保留，不与既有内部重试嵌套。
2. 实现 agent 独占 `vega_runtime` 的 Provider/SSE/agent 与必要测试、`vega_conversation` 的事件/持久化桥接与测试、`vega_ui` 的 conversation_stream 状态/回滚与测试，以及 `vega` 必要的 exhaustive match/活动会话桥接。新增 UI 共享类型经 `vega_conversation::types`，runtime 不依赖 UI。
3. 不新增依赖、数据库迁移或代码注释，不更改卡外业务。优先复用已有 retry、snapshot、run notice 和取消机制。
4. 本地只执行格式/差异等快速静态检查。禁止本地 compile/build/package、cargo check、clippy、test、nextest。云端 `.github/workflows/pr-check.yml` 执行 fmt、Clippy 和 workspace nextest；失败先修复，不绕过门禁。
5. 提交 PR 关联本 Issue（不使用提前关闭关键字）。云端 check 通过后按仓库约定 squash merge，再置 In review 等用户手测；未手测通过不关闭 Issue、不清理 worktree。

回滚方式：通过后续 PR revert 本卡 squash commit，恢复原有重试行为；无 schema 变更。交付结果记录于 `vega-issue-350-network-retry-delivery.md`，必须区分静态检查、云端回归和用户真实网络/UI 手测。

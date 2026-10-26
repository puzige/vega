# #544 接口闭合恢复检查点

旧分析执行 `01M5KQJW67GEWJTP3YMGHWVQR6/01M5KQJW67WXV5PK37VRSZZH2N` 已 interrupted；未保存任何 v2 文档或源码，不能称已冻结/可实现。

父 fetch 后正常 merge master `3277ebb5fd710d7d33080f91b6c5235e1558e0d3`，本卡 HEAD `38e06c16ea4ae6978778cec2076267bd44b062e8`。已发布 proposal v1 `303eec5f3a45d2dfb0c0e136d8ce42db66dd90c2` 保持不变；主规格 A1–A12、无 outbox/schema/跨进程恢复范围保持。取消强制 TDD 只更新流程措辞，不消除真实 owner/证明/历史写入竞态。

前一独立审查的具体未决：app-global claim/identity-checked preflight upgrade/conditional release；实际 commit 后且 title 前写入单调 receipt；primary 首次驱动前 Started；NotCommitted/NotStarted 只在 worker quiescence 与成功 cleanup 后密封；同一 fence 跨 recovery 资格/事务/commit/launch，列出同进程已授权历史 mutator。RunningThreads 是投影，LiveDelegation 是引用计数，per-window TrustedActionCoordinator 非全局独占，delegated ownership 只覆盖 child，均不能冒充 ordinary root lock。

旧分析已定位 sidebar 删除、plan-review 持久化、hydration repair、manual context 入口也须纳入 fence 审计。下一阶段应直接把可核源码证据和未知项写为候选 v2 + 精简实现交接，不再展开通盘审计；v1 字节不变。不授权源码实现，不接管 #535，不将未闭合或未经独审候选称 code-ready。仅 docs/544 可写；Git/PR/云端由父负责，本地仅静态检查，无 Vega 执行/编译/测试或 CUA。

## 实际保存的 v2（父复核）

续接执行 `01M5KSKES3W3P9EZFW4RXDM1TG/01M5KSKES3J80BJQH21KBTBVZR` 因时限 interrupted，已停写。本轮实际保存候选 v2、精确未决调用点图，以及主规格 v4 的 27 行等量替换；不是生产实现或机制安全性通过。父读回全部修改与新文档，独立核对 A1–A12 矩阵逐字不变、v1 SHA-256 不变；相对链接全部存在，UTF-8/LF、差异空白与敏感路径静态检查通过。

子执行的源码定位绑定 master `3277ebb5fd710d7d33080f91b6c5235e1558e0d3` / task `38e06c16ea4ae6978778cec2076267bd44b062e8`。父后续 fetch 并正常合入 master `b032d0d2ab9ee057137f3e5ad2c0e517011ea1ae`（#575 上下文用量），当前集成 HEAD `2b8f694a8494bddd74ed0cdc8c2c8224f8fbc7bf`。该增量在 window/artifact ingress 和上下文状态展示，未改 ordinary preflight/pipeline/entry 的 owner 与事实缺口；调用点图行号仍按其原静态基线解释。没有回退 #344/#575 或恢复旧 Rust 测试。

父取得 #535 的当前开发快照 `5ecf37ddde54115f059a7d965eb179a0102a4768`，观察到 `RunContinued { previous_assistant_id, user_message_id, next_assistant_id }` 以及 active owner 的 predecessor terminal-event 过滤；之后远端 head 又更新为 `ba9c155f6392d3df280fda69aad3d3f4681708ac`，仍不能将任一快照称最终接口。#544 的 claim/receipt 不得结束或重复消费这些 continuation owner；生产集成前仍须重新固定实际最终版本并串行协调。

C1–C6 未决保留，下一步定点关闭 app-global owner 寿命、数据库身份、sealed proof/cleanup/drain 和冲突 writer 的准入，而不是再做宽泛审计或只实现 happy path。本文档可发布以供审查；无测试/CI/产品时延结论，不授权源码实现。

## #535 已合入后的精确兼容基线

父已取得并正常 merge master `988e1e37271c666f05ff8eae46d662324e91afd6`（#535 / PR #563），本卡集成 HEAD `759450219a627ffe59b40824124923b7d37becec`。这取代上节“尚无最终接口”的依赖事实，但不自动关闭 owner/围栏接线。

已静态复核最终接口：`ConversationEvent::RunContinued { previous_assistant_id, user_message_id, next_assistant_id }`；同一 `ActiveAgentRun` 的 generation/thread/stream 不变；`observe_terminal_message` 只接受合法链中的 predecessor，并过滤该 predecessor 迟到的 MessageFinished/Interrupted/Error，新的 MessageStarted 才登记后继 assistant。conversation 原子记录 run-continuation 关系，历史和 UI 以同 run 投影；不恢复已清理的开发测试。

因此 C4 的“取得实际最终签名”部分已满足；**ordinary claim/receipt 如何与既有 active run 的 continuation 授权写入和终态移交共存仍未闭合**。#544 不把 RunContinued 当普通 durable receipt，不释放后继尚能写入的 owner，不复用 pending_id 形成普通 echo；原 queue/steer/Codex/approved-plan/credential 路由仍排除。C1–C3 与 C5–C6 不因 #535 合并解除。下一具体步骤为在此精确合并版上固定 claim、sealed facts 和 mutator 的准入/移交签名，之后才可组织生产实现；不是继续等待他卡或宣称已实现。

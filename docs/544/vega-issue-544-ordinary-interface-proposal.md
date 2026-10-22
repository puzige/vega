# #544 普通提交最小接口冻结候选

状态：父 Agent 方案候选，待独立审查；不是测试 READY 或生产实现许可。对应[主规格](vega-issue-544-send-echo.md) A1–A12、§3.4–3.5；不新增 outbox、schema、跨进程恢复或队列产品。

## 版本与已核事实

- 主规格已发布 `e37023a5d10bc8e05f032708ad2fd7b94ef7ec01`，SHA-256 `c3f74b399ffc90c3c3d7d5b4363eb6b7d5c6cf3e6dfacc9b02ecfac76e3fcb6c`；四项规则增量已被非写入者复审，具体接口尚未冻结。
- 当前源码基线 `6615748f91273bfb8ee424011b3b5a09b3777d2b`，本分支保留旧规格并同步后的 HEAD `f7a74b90fd53142227d7fe170623f2a9366571cf`。不恢复旧 ref 文件。
- `draft_thread` 已分配真实 thread ID，`materialize_draft` 保留它。`AppAgentController.active` 是现有运行 owner；`RunningThreads` 是只读投影，`LiveDelegation` 是引用计数，二者均不可当作排他锁。
- `prepare_run_with_images_and_reasoning` 已有 immediate transaction，但普通准备会先调用 `recover_thread`。恢复分支必须先核资格，不能用该函数改写后的状态反推未启动。
- `MessageStarted` 发生在 skill/actor 准备后；自动标题可能在 commit 后、准备返回前启动。主运行的 runtime future 在 entry 后段才被驱动。

## 1. 单一 owner 与运行占用

推荐在 `AppAgentController` 保留普通提交记录，按稳定 `submission_id`/thread 索引；记录不成为第二套运行表。它持有：原始文本/附件不可变快照、原 Composer revision、稳定 user ID、本次 attempt、曾绑定的 stream 身份、一次接受的引用展开载荷，以及 typed 提交/启动事实。

- UI 普通路由通过原有 automation/queue/Codex/配置/权限守卫后，同一 UI 事件内建立该记录和真实 pending 行，再开始后台 I/O。原 Composer 内容只在归属仍是捕获 revision 时转移一次，后续回执不再清新草稿。
- 前期沿用 `TrustedActionCoordinator` 的 preflight lease；快照不随 lease 释放而删除。原 lease 不被解释为 durable 或主运行权。
- 到准备工作/恢复 attempt 前，同一 UI 事件内复核全局运行投影、现有 `active`/queue busy，并注册原 `AppAgentController::begin` 的运行占用；随后才将普通请求交 worker。记录只引用该 generation/cancel，不另维护运行权威。取消/worker spawn 失败沿原 owner 释放链处理，快照保留为明确终态。
- 把 database/thread/workspace 读取移到该 worker，但不把 UI 捕获的配置或路径当永久授权；conversation 事务内重新验证身份/权限，迟到结果只更新匹配 submission/attempt/generation/thread 的记录。线程普通切换保留 owner，重开同线程重投影；不得向当前另一线程插行或清草稿。视图被释放并不表示允许重启旧 attempt。
- 本 proposal 不承诺窗口关闭后的进程内跨窗口自动接管。现有未保存内容/关闭保护必须保留；若 owner 生命周期不能满足主规格普通视图释放回归，应先修接线再冻结测试，不能临时升级成磁盘恢复。

独审重点：preflight→active 的单次升级、同线程其他入口/多窗口的已有原子 UI 顺序、busy 的真实语义及取消释放是否完整。不得因全局投影为 false 就跳过真实 owner 校验。

## 2. 最小 typed 边界与事实时点

共享 request/receipt 类型放 `vega_conversation::types`，不是 UI 自造同名跨 crate 类型。具体 Rust 名可调整，以下字段及事实域不能合并：

| 候选 | 必需内容/时点 | 不变量 |
|---|---|---|
| OrdinarySubmissionIdentity | submission、attempt、thread、稳定 user ID；恢复另含期望旧 assistant/尾序 | 不复用 pending queue ID、credential ID 或 approved-plan persisted-user 参数；这些路由与普通 owner 互斥 |
| AcceptedOrdinaryPayload | 首次引用展开后的真实正文及原图片顺序/字节快照；只在 owner 内存/请求中保留 | 展示用 safe_live_text 不作比对；恢复不再读引用文件、不以诊断日志存载荷 |
| OrdinaryCommitReceipt | 上述 identity、user seq、assistant ID/seq、已接受 payload 归属 | 仅真实 transaction.commit 成功后、后续 skill/actor 前；重复投递幂等；MessageStarted 不前移 |
| OrdinaryAttemptFacts | commit 为 NotCommitted / Committed(receipt) / Unknown；primary 为 NotStarted / Started / Unknown；另有失败/取消原因 | 取消、join 或通道错误不自动证明未提交；对外未知保守处理；标题与主运行分域 |

推荐为该普通请求使用一份窄的存活 attempt receipt/proof handle：只保存事实，不调度、不决定业务路由。由 worker/conversation 在真实边界更新，控制器保留它到终态处理完；UI event 负责唤醒/投影，通道丢失不能抹掉已记录的提交事实。更新需单调、identity 校验且有明确错误结果；锁毒/证明丢失走 Unknown，不重置 NotCommitted。

- 引用展开只做一次，接受载荷在写入前绑定 handle；恢复复用该载荷，不重新解析原文。前期失败未曾接受载荷时可重新准备原始不可变快照，不能冒用当前 Composer。
- commit 后更新 receipt；若结果不明或 receipt 无法可靠保留，不开始新的主运行，转核实。即使标题已启动，也不伪报所有 provider 请求为零。
- 主运行开始前最后检查对应 cancel/owner，在实际驱动主运行前记录 Started；事件投递失败可以阻止后续启动，但不能反向清除 commit。记录早于副作用可保守拒绝恢复，不能晚于副作用还声称 NotStarted。
- 启动前失败只有已完成且成功写入失败终态、无正在存活旧 worker、proof 确认 NotStarted 才可候选恢复；清理失败/仍运行/状态未知都只允许核实或既有运行处理。

## 3. 独立普通恢复事务

新增独立 conversation 入口，不放宽 `uses_existing_user` 的批准计划校验，不借用 queue/steering。入口接受被冻结的普通 identity、旧 receipt、accepted payload、预期尾序和存活 attempt proof。

1. 控制器先取得原运行占用并单次保留新 attempt；重复请求观察同一 owner 结果，不重复启动。旧 worker 未终止时不恢复。
2. conversation 检查 proof 归属/NotStarted/成功清理事实；再开 immediate transaction，在同一事务核对 thread、原 user role/kind/status/内容、图片数量/顺序/字节、旧 assistant ID/seq/失败终态、无后续历史及预期 next_seq。不得先执行普通 `recover_thread` 改写证据。
3. 原子分配并插入新 assistant（新 ID/后续 seq），不插 user/图片、不再次 claim 自动标题，形成新 receipt；原有权限、上下文与真实历史准备仍执行。
4. commit 后沿同一 skill/actor/primary 顺序处理；取消临界点按实际 commit 事实决定，不能撤销历史。事务内资格检查失败释放新 attempt 的运行占用并保留原提交/恢复原因；不是“重试成功”。

事务结束后其他已授权历史写入的竞态不能靠一次预查询解决。首轮测试必须尝试重复/并发恢复、核实与新历史插入交错、迟到旧 attempt；若现有 owner+事务组合不能阻止失效请求启动，先指出实际入口和缺口，不能宣称 exactly-once 或暗增 schema。SQLite 主键防重不替代上述资格与运行不变量。

## 4. 冻结历史候选与首轮测试

保持现有 UI/模型历史投影，不增加过滤规则：旧启动前失败 assistant 保留原 ID/seq 与 failed，恢复使用新的 assistant ID/后续 seq；已提交 user/images 唯一。`history_from_context_source` 排除当前 assistant/streaming，但不会普遍排除旧空 failed；首轮测试须按实际规则明确包含它，不能在 fixture 中隐藏。

| 目标 | 首轮必需对照 |
|---|---|
| A1–A4、A8 | 扣住真实外部预检/准备回执，下一 draw 已有唯一 pending；真实可编辑新草稿；同文不同 revision/新增附件保留；重复事件不多启动 |
| A5–A7 | 预检/worker/事务失败有明确未提交事实；commit 后 skill/actor 失败保留 receipt；标题与主运行计数分域；取消、通道/join 丢失走准确 Unknown/核实 |
| A6/A7 恢复 | 启动前失败可恢复、已 Started 但空输出不可恢复、清理失败/proof 丢失不可盲恢复；重复恢复/新历史/图片错序/同脱敏显示不同原文均有拒绝对照 |
| A6/A9 历史 | 完整 durable user/旧 assistant/new assistant ID/seq/status 与唯一图片，刷新 UI/实际请求历史；视图释放重开/切 B 后迟到结果不串线程 |
| A10–A12 | queue/steering/approved-plan/credential/Codex/automation 原路由、权限/取消、copy/selection/锚点不回归；不从本卡泛化产品 |

#535 当前独立实现仍活跃，本 proposal 不接管它、不改其验收；普通接口必须与其最终冻结 continuation 版本串行集成。下一步：非写入者审查本候选的 owner/证明和事务闭合性，明确可实现接口后才交唯一 writer 写齐全部测试/独审/现有 Actions 有效 RED。未取得这些前提，不实施生产。

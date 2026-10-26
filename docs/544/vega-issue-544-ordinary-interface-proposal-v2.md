# #544 普通提交接口候选 v2：进程内 claim、密封事实与恢复围栏

状态：**READY-doc / NOT code-ready**。本文件完成有界接口候选，不表示机制已实现、独审通过或获生产实现许可。对应[主规格](vega-issue-544-send-echo.md)全部 A1–A12；[v1](vega-issue-544-ordinary-interface-proposal.md)保留为历史候选，字节不得改写。证据与未决入口见[精确调用点图](vega-issue-544-ordinary-unresolved-callsite-map.md)。

## 0. 基线、权限与非目标

- 父提供的 worktree/branch 为 `vega-544-send-echo` / `feat/544-send-echo`，HEAD `38e06c16ea4ae6978778cec2076267bd44b062e8`；父已 fetch 并正常 merge master `3277ebb5fd710d7d33080f91b6c5235e1558e0d3`（#344；发送边界据父未变）。本轮不运行 Git，不将父提供的版本身份写成独立 Git 核验结果。
- 首读[恢复检查点](vega-issue-544-interface-closure-checkpoint.md)。旧 analyst 中断且未保存 v2；本轮以实际保存文件为交付，不恢复其未落盘推断。
- v1 SHA-256 应保持 `6c6eaadbbd46d8579ffa29528d183b100c8003fba331fb1012fb336648e9a2cd`。主规格原 A1–A12 不削减；仅链接候选、纠正 receipt 时点及过时强制 TDD 表述。
- 仅 `docs/544/` 写入；不写源码、Cargo、workflow 或通用流程文档。不运行 Vega、build/check/test/CUA，不 fetch/commit/push/PR 或访问外部服务；父拥有集成。
- 无磁盘 outbox、schema 扩展、跨进程/崩溃恢复、第二套运行调度器；不接管 #535，不顺带过滤旧 failed assistant 历史。不承诺毫秒、帧率或所有 provider 请求为零。

## 1. 普通边界与同步展示

只适用于 **Native、非运行、普通人类发送**。先完成不做 I/O 的路由/输入门禁，明确排除 queue、steering、send-now、Codex、automation、approved-plan、credential continuation；非普通请求返回 `NotOrdinary` 给原路由，不产生本卡 claim、pending 行或草稿转移。不得绕过 model/reasoning、权限、危险确认、skill intent 与路径围栏；后台执行时仍重验权限/路径，不把 UI 快照当永久授权。

同一 UI 事件的顺序必须是：

1. 捕获原始文字、附件 ID/顺序/字节及 Composer revision；分配稳定 submission ID、user message ID、本地 entry key。draft 已有真实 thread ID，materialize 不得换 ID。
2. 取得下节的进程内排他 claim，关联现有 controller owner；重复同身份事件观察原提交，冲突不把首份提交标失败。
3. **在 provider preflight、draft materialize、数据库/目录/引用读取或 worker 发布前**，同步将唯一 LocalPending 用户/图片组插入真实列表并 notify。纯图片不造空文字行；正文沿现有安全展示投影。
4. 将仍归属捕获 revision 的文字/附件只转移一次，得到可编辑新 revision。之后 receipt、失败、取消、旧事件均不得清 Composer；即使新草稿字节完全相同也保留。
5. 再发布 preflight/准备 worker。确认只原位补 durable 归属/状态，不追加 User/UserImages、不替换 MessageCopy、key、选择对象或额外抢滚动位置。

进程存活期间，原始/accepted payload、附件预览/字节和逻辑行归属必须由不依赖视图寿命的 owner 保留；切路由、缓存淘汰或窗口释放不能丢 payload、释放旧 worker 运行权或让旧 worker 复活。重开同线程重新投影；另一线程绝不接收旧行或清草稿。进程退出恢复不在范围，既有未保存内容保护保留。

## 2. 一个 claim，引用现有 controller，不另建 scheduler

以下是**候选 API 契约，不是现有 Rust API**。跨 crate 共享身份、request、receipt/proof 放 `vega_conversation::types`；app-global 注册/owner 连接留 app 层，UI 不读写 SQLite。

进程内按数据库身份 + thread ID 索引一个窄 claim；数据库身份必须统一各窗口别名，具体规范化接口未闭合（P1-C1）。claim 只做同线程准入/冲突历史写入围栏，引用现有 `AppAgentController` 的 owner ID/generation/cancel 与存活 submission record，不复制 `ActiveAgentRun`、queue dispatch 或再调度任务。UI observer/stream 可换绑，不能成为 proof 的寿命边界。registry 必须保留记录/worker 事实所需的强寿命，而非只存随窗口死亡的 weak 标记。

| 候选操作 | 输入 / 结果 | 必须满足的原子条件 |
|---|---|---|
| `claim_ordinary` | route、database/thread、submission/attempt、controller owner 身份；返回 `NotOrdinary` / `Claimed` / `SameSubmission` / `Conflict` / `Unknown` | 非普通不占用；真实 owner/queue/同进程 writer 冲突不能只读 running bool；新 claim 在任何 worker publish 前可见；锁毒/身份不明 fail closed |
| `upgrade_preflight` | 精确 claim token + preflight lease + submission/attempt + frozen model/reasoning/cancel | `Preflight -> Active(owner generation)` 是同身份的一次升级；不 release 再抢；stale lease 不升级。复用/扩展现有 `begin`，不是第二 owner；已持有 claim 的自己不走 fresh-start self-busy 分支 |
| `register_worker` | claim/owner/attempt + worker token、facts handle | 在 worker 可启动前登记责任；preflight、prepare、primary、失败清理的能力均须可追踪。spawn 失败记录“未发布 worker”，不可凭 UI 通道断开猜测 |
| `request_cancel` / `detach_view` | 精确身份；取消请求或撤销 observer | 只置 cancel/解绑展示，不授予新运行权，不丢快照；窗口 release 不是 quiescence |
| `conditional_release` | 原 claim token、owner generation、密封事实与 worker drain 证明 | compare-and-release，不释放别人的 generation；仅旧 attempt 无再写入/驱动能力且终态已密封时释放。未知保持围栏/核实；记录和可恢复 payload 的保留独立于 busy 生命周期 |
| `reserve_recovery` | 原 receipt + sealed eligibility + 同一排他 claim | 原资格单次消费，并持有到事务校验、new assistant commit 与 launch admission；重复请求只观察已有 reservation/结果，不再插入或启动 |

`AppAgentController.active` 和 `TrustedActionCoordinator` 都是 per-window；`RunningThreads` 仅 bool 投影，`LiveDelegation` 仅引用计数，`delegated_ownership::acquire_for_thread` 对 ordinary root 无排他 owner。不能把其中任何一个改名为全局锁。现有 `AutomationGlobal` 注册也不能未经原子拒绝/释放语义核实就宣称满足 claim。最小接线仍需父确定 owner 在窗口释放后的保留位置和非普通准入参与点，不把“保留在 controller”写成已解决生命周期。

fresh-start 守卫的其他冲突、设置/模型/权限检查继续执行；只为精确 claim owner 提供 checked upgrade。尤其不能在 `finish_agent_preflight` 先释放唯一排他权，再调用会因本人的 trusted-action busy 而拒绝的旧 fresh start。视图切走可以取消或继续原 attempt，但必须由同一存活 owner 收敛，不以当前路由不匹配丢记录。

## 3. 三个载荷域、共享事实 handle 与密封

- `RawOrdinarySnapshot`：捕获的用户原文/附件/revision，用于本地展示和明确未提交后的准备；不从新 Composer 补取。
- `AcceptedOrdinaryPayload`：首次接受的引用展开真实正文 + 图片数量/顺序/实际字节，写入前绑定 submission/attempt facts。恢复只复用它，不重读已改变的引用文件。若已绑定则后续准备复用；首次展开从未被接受的未提交失败可从原快照重新准备。
- `safe_live_text`：仅展示/脱敏投影；同一显示不证明原文相同。payload、图片、凭据不进入诊断日志。

`OrdinaryAttemptFacts` 是 controller/worker/conversation 共享的窄进程内事实 handle：所有更新校验 database/thread/submission/attempt/owner，单调、typed、可失败。事件通道只唤醒 UI，事实不以收到事件为准。状态轴不能压成 success bool：

| 轴 | 非终态 | 可密封事实及限制 |
|---|---|---|
| 提交 | `InProgress`、`CommitInFlight` | `Committed(receipt)` 只来源于实际 commit 成功；`NotCommitted` 只在已知未成功提交、rollback/未进事务结论及全部 worker quiescent 后密封；无法判定是 `Unknown`，不能降格成未提交 |
| 主运行 | `NotYetDriven`（不是恢复许可） | `Started` 在首次驱动 primary future 前单调写入；`NotStarted` 只在旧 worker 不再可能驱动且必要 cleanup 成功后密封；其余 `Unknown` |
| 清理 | `Pending` / `InFlight` | `Succeeded` 需匹配 assistant 的真实失败终态写入成功；`Failed` / `Unknown` 不给恢复资格。未提交且无 assistant 才可明确 `NotRequired` |
| worker 能力 | `Registered` / `Running` / `Draining` | `Quiescent` 需所有相关 worker/子任务已失去再提交、驱动 primary、迟到写入的能力；cancel token、disconnect、收到 terminal、join 错误均不单独证明它 |

### 3.1 commit receipt 必须在实际 commit 后、title 前

`pipeline.rs:1130` 的 transaction commit 成功后，**立即在同一准备链把 identity、user ID/seq、assistant ID/seq、accepted payload 归属写入 handle**，然后才允许 `:1131–1138` 的 title launch，再返回 `PreparedRun`（`:1149`）及进入后续 skill/actor 准备。不是等 `prepare_run_with_images_and_reasoning` 返回才发布。UI receipt 的通知可晚到/重放，但事实写入不能晚于 title/prepare return。

成功 commit 后 handle 更新失败/锁毒不能伪报 rollback 或 `NotCommitted`：停止新的 title/primary 准入，保留已知 committed 或结果不明，转 conversation 层核实。事务 commit 自身返回无法确定的错误也走 Unknown。不能要求 SQLite 与内存事实跨资源原子；保守未知分支正是必需恢复边界。

标题是独立 post-commit provider 请求，取消 token、计费及唯一 claim 沿原规则；`NotStarted` 只覆盖 primary/工具，不宣称所有 provider 为零。恢复不得重复自动标题。

### 3.2 primary、清理与 drain

在 `entry` 中创建 runtime future 不等于已启动；**首次 poll/drive 前**须校验同一 claim/cancel，并成功单调写入 `Started`，再交给 await/join 驱动。不能等 MessageStarted、首 token 或 future 返回才记。保守提前 Started 可以拒绝恢复，晚记不能证明无副作用。

skill/actor 启动失败及取消清理必须把 `finish_prepared_failure` 的真实 Result 写入 facts；现有忽略错误的出口不构成成功清理证据。取消后先封住新 publish/launch 能力，等待已注册 worker drain，记录 commit/cleanup/primary 结果，再由最后有责任的 worker 密封 terminal facts。正常 join 可参与证明，但没有覆盖其发布的全部子任务时仍不够；panic/断线/丢 handle/锁毒均 fail closed，不把 InProgress/CommitInFlight 当终态。

### 3.3 展示状态不是事实轴替代物

| 展示 | 事实 / 行为 |
|---|---|
| Draft | 当前独立可编辑 revision，不是旧 attempt payload |
| LocalPending / Preparing | 同一逻辑用户/图片组；尚无 durable 成功；可以取消；不造常驻准备说明 |
| FailedUncommitted / CancelledUncommitted | 仅 sealed NotCommitted + quiescence；旧快照可原位新 attempt，不清新草稿 |
| Reconciling | 提交/启动/cleanup/worker 任一必要事实未知；保留 payload，提供核实及可见的禁重投原因；UI 经 conversation 核实，不直接 SQL |
| Committed | receipt 匹配后原位转正式；Started 独立，重复通知幂等 |
| CommittedStartFailed / Stopped | 已提交历史不撤销；只有下节全部条件满足才可重启，否则核实/既有停止恢复语义 |

## 4. 已提交恢复：同一 reservation 与历史围栏跨到 launch admission

候选独立 conversation 入口 `recover_ordinary_submission`，不复用/放宽 approved-plan `uses_existing_user`，不借 queue/pending ID。需同时具备原 Committed receipt、**sealed NotStarted + successful cleanup + worker Quiescent**、accepted payload/images 与预期旧 user/assistant/尾序。空 failed、无工具、用户 ID 存在均不够。

1. 用同一 app-global claim 原子保留恢复 attempt，单次消费旧 eligibility；并发/重复/迟到请求无第二次 reservation。即使新 attempt 失败，也不能复活旧资格：明确未提交时重用该 reservation 幂等继续或由新 sealed facts 决定下一动作；未知保持核实。
2. 在新恢复入口最早处保留 `LiveDelegation` 引用，并贯穿准备/移交/终态；它不是排他锁，但必须防另一窗口 `restart_history_page`/hydrate 将中间 assistant 误判孤儿后 repair。取得引用与 repair 的并发准入仍需同一 fence 配合，不能只加 refcount。
3. 同一 claim 持续阻止冲突的同进程已授权 history writer。开 immediate transaction，重验身份、权限/path 前提及 thread；精确比对原 user 的 thread/role/kind/status/正文、图片数量/顺序/字节、旧 assistant ID/seq/匹配 failed、无后续历史和 expected next_seq。**不得先调用普通 recover_thread 改写这些证据。**
4. 在该事务内只分配并插入新 assistant ID/后续 seq，原子记录新 attempt 对应 reservation；不插 user/images，不再次 claim title。事务提交后按 §3 立即记录新 receipt，再继续 skill/actor/primary。旧 assistant 保留 failed 和原 ID/seq。
5. SQLite transaction 不跨 skill、网络、provider 或 UI 等待持有。进程内 claim/fence 则跨资格检查、事务、commit、所有异步准备直到 primary launch admission；启动前在同一 claim 身份下检查仍有效，再 mark Started 并驱动。冲突 writer 在此期间拒绝/等待原语义允许的结果，不在锁外“再 SELECT 一次”后无条件启动。
6. launch admission 后围栏如何移交给现有 active-run 历史写入规则，必须显式定义；不能在 active owner 仍能启动/写入时因 UI terminal/release 解锁。失败按 commit 事实、成功 cleanup 和 quiescence 条件释放。

围栏按 thread + database identity 约束，不扩大成跨线程 scheduler；title 仅元数据、primary 自身合法写入和 #535 continuation 的授权模式需要精确分类，不能全禁也不能默认绕过。现有 UI 与 `history_from_context_source` 的模型历史均按原规则保留旧空 failed assistant；该卡不新增过滤。

## 5. 有界未决项与下一次具体核验

精确证据/入口清单以[调用点图](vega-issue-544-ordinary-unresolved-callsite-map.md)为准。以下是 blocker，不是已完成实现：

| ID / 级别 | 尚未闭合 | 下一次最小核验 / 责任边界 |
|---|---|---|
| C1 / P1 | app-global claim 的数据库身份、跨窗口存活 controller 引用、preflight upgrade 与所有 fresh-start 准入尚无最终 API | 父指定唯一 app 层 owner/registry 接线，复核 window release、generation CAS、spawn 前登记、self-busy 例外；拒绝用投影/child lock 替代 |
| C2 / P1 | commit-before-title handle、首次 drive 前 Started、cleanup Result 与完整 drain 的生产接线未实现 | 定点审核 pipeline/entry 的事实更新及所有早退出口；明确证明由谁密封、handle 失败如何核实，不能以 disconnect/join/cancel 代证 |
| C3 / P1 | 同一恢复 reservation + immediate transaction + launch fence 尚无覆盖所有冲突 writer 的调用契约 | 逐项关闭调用点图的 delete、plan persistence、hydration repair、manual context 与运行/续跑入口；核实阻塞/拒绝及 owner-authorized 写入，不仅新增一次查询 |
| C4 / P1 | #535 活跃且其 continuation/pending/SteeringAccepted 最终接口未取得 | 父协调实际最终签名/身份/owner 移交；#544 不接管、不覆盖、不假设其已冻结；在集成基线上重核边界 |
| C5 / P2 | 重投影/稳定 key、MessageCopy/附件寿命、准确错误/禁用/键盘状态尚无行为证据 | 后续获授权后按 A1–A12 核验实际对象和视图淘汰，不以两份始终存活 view 或字符串匹配冒充 |
| C6 / P2 | 云端进程内观察接线与真实 UI 时延未取得 | harness 先用正式历史行作阳性对照；接线失败修观察层。真实 present/IME/长会话由用户手测，毫秒数未知 |

这些未知不阻止完成本文档，但阻止称 code-ready；尤其未封住冲突 writer 时不能宣称 exactly-once 恢复。父 review 后决定下一授权；本任务不给 production 权限。

## 6. 按风险的必要验证，不设 mandatory RED

按根 AGENTS、[exec-guide §7.2](../vega-exec-guide.md#72-快速交付与按风险验证当前规则)及[当前政策](../vega-rust-test-delivery-lifecycle.md)：取消强制 TDD/测试冻结/有效 RED 前置；不削减验收、安全断言和必要回归。本文仅快速静态核对，不运行产品/测试/CI。以后获实现授权时选择必要云端验证；开发 Rust 测试如使用，固定可恢复源码/完整 SHA/实际结果并在合并前清理独占支持，最终 required CI 与空集结果按政策分别报告，不改 workflow。

| 风险 / 验收 | 必要可观察对照（未来验证，均未运行） |
|---|---|
| A1–A4、A8：展示先于 I/O、转移一次 | latch 扣住真实 preflight/准备，下一有效 draw 已有唯一 pending；同文不同 revision/新附件保留；双击/重复事件/重复 receipt 无新增 user/image、MessageCopy 不换 |
| A5–A7：真实提交与启动事实 | commit 前失败、commit 后 title 前事实点、skill/actor 失败、primary 首 poll、cleanup 失败、cancel 两种临界顺序、disconnect/join/handle poison；断言 Unknown 不当 NotCommitted，title/primary 计数分域 |
| A6/A7：恢复与冲突 | sealed NotStarted 正例；Started 但空输出、cleanup 失败、worker 未 drain、payload/图片错序、同显示不同原文、尾序变化反例；重复恢复只消费一次，冲突 writer 与 commit/launch 的交错不能启动失效请求 |
| A6/A9：历史与 owner 寿命 | 原 user/images 唯一，旧 failed/new assistant 完整 ID/seq/status 与实际模型历史；保留 LiveDelegation 防跨窗口 hydrate repair；切 B、视图淘汰/窗口释放、迟到事件不丢 A payload/重启旧 worker/改 B 草稿 |
| A10–A12：路由、安全与真实体验 | #535 实际最终 continuation/queue/steer、Codex/automation/plan/credential 无普通 claim 误入；权限/path/model guard 保留；选择/复制/锚点、键盘/禁用原因、真实 UI 延迟分阶段由用户验证 |

## 7. 文档完成边界

交付为本候选、精确未决调用点图及主规格的最小链接/时点/验证措辞修正；v1 和父检查点保持原字节。最终回报列父提供 HEAD、文件 SHA-256、无 Git 的确定性文档 diff SHA-256、实际静态检查和 C1–C6 未决；不将未运行验证写 PASS。父负责独审/集成/下一授权；READY-doc 后停止本任务全部写入。

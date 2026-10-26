# #544 candidate v2：精确未决调用点图

状态：**READY-doc / NOT code-ready**。配套[候选 v2](vega-issue-544-ordinary-interface-proposal-v2.md)，不新增实现授权。本文把中断检查点的 P1/P2 落到已读源码位置；不是全仓已封闭的 writer 审计，也不是 API 已实现的声明。

## 0. 证据口径

- 父提供 HEAD `38e06c16ea4ae6978778cec2076267bd44b062e8`，父已正常合入 master `3277ebb5fd710d7d33080f91b6c5235e1558e0d3`；本轮不运行 Git/不访问远端，版本身份按父检查点记录。下列位置在本工作树静态读取，后续以符号重新定位。
- `V`＝本轮读到的当前行为；`C`＝候选必须满足、尚未实现；`U`＝精确接线/竞态仍未闭合。行号不是行为验证。本地只读文本/哈希/快速静态，无 Vega、编译、测试或 CUA。
- 原 [v1](vega-issue-544-ordinary-interface-proposal.md)及[父检查点](vega-issue-544-interface-closure-checkpoint.md)不改写；主规格 A1–A12 保留。下面链接均指当前本地源文件，不暗示 #535 最新外部实现已经取得。

## 1. 普通链：现状 → 必需插入点

| 点 / 精确源位置 | V：当前操作顺序 | C / U：candidate 接口与未决 |
|---|---|---|
| E1 [composer.rs](../../crates/vega_ui/src/conversation_stream/composer.rs):150–195，`can_submit_message` / `submit_message` | guard 后 `:174` 置 submit pending；revision 只在 running 记录 `:175–178`；emit ComposerSubmitted，未插普通行 | C：普通路由接受后同事件建立唯一 LocalPending，先于 preflight/任一 I/O；捕获 revision/附件所有权只转移一次。U/C5：app→UI 同步接线及新稳定 entry metadata 尚无最终签名 |
| E2 同文件 :201–253，`accept_composer_submission` / `reject_composer_submission` | accept 在 `:214` 比文字清输入，`:220/:240` 追加 User/UserImages；reject 清 submitted attachments | C：普通分支拆成本地展示、receipt 原位确认、typed failure；不提前调用旧 accept。新草稿同字节亦不清；原 MessageCopy/附件组不重复 append。非普通保留原语义 |
| E3 [window/agent.rs](../../crates/vega/src/window/agent.rs):1140–1219，`submit_composer` | `:1159` automation 截获，`:1162–1166` Native pending route 入 queue，`:1178–1180` Codex；`:1196` per-window lease 后 `:1216–1219` 发布 preflight worker | C：纯内存 ordinary 分类后 `claim_ordinary` + E1 pending，再 publish。非普通返回 `NotOrdinary` 继续原入口；preflight worker 同样登记/取消/drain。U/C1：app-global claim、重复投递与多窗口冲突 API |
| E4 同文件 :1711–1761，`finish_agent_preflight` | `:1723` 先 release lease；`:1728` 当前 route/settings 不匹配 reject；`:1739–1740` 同步 materialize；skill-pin 或 fresh start | C：核对 claim/lease/attempt 后原子 `upgrade_preflight`，无 release→reacquire 空隙；route detach 不丢 payload。U/C1：skill-pin 延伸 preflight 的 token 移交/后台 materialize 接线必须由同一 owner 持有 |
| E5 同文件 :680–787，`start_agent_run_with_reasoning` | `:700–707` running/本窗口 active/queue guard；`:740–744` trusted-action busy 等 guard；`:759–787` UI 同步读 Store/thread/workspace | C：已有 ordinary claim 的 checked-upgrade 分支只跳过“本人的 busy”，不得跳其他 owner/模型/权限守卫；I/O 到 worker。U/C1：不能简单预先调用 begin 再进入旧 self-busy guard |
| E6 同文件 :794–819、844–906 | `AppAgentController::begin` 后向 AutomationGlobal.runs insert；`:844` running 投影；`:891` spawn worker | C：claim 与现有 generation/cancel 身份连接，worker 在可执行前登记；spawn 失败密封未发布事实。V：global insert/投影不是原子冲突拒绝证明。U/C1：所有 fresh starts 的统一准入参与点 |
| E7 [app_agent.rs](../../crates/vega/src/app_agent.rs):1148–1205，`run_agent_worker_with_mcp` 的 UserMessage 分支 | `:1155` 取 raw content，`:1159–1179` 有界读引用并展开；`:1195–1201` title 用独立 cancel；`:1205` 再看 cancel | C：raw snapshot、一次 accepted expansion、safe display 三域分离；accepted 写入 facts 后恢复复用。U/C2：accepted 的一次绑定和 late cancel 的责任交接 |
| E8 [entry.rs](../../crates/vega_conversation/src/agent/entry.rs):596–705，`run_thread_task_with_automation_inner` | `:616–619` 保留 LiveDelegation；`:637–642` 从 credential/persisted-plan/pending ID 或生成 user ID；spawn blocking prepare | C：新增独立 ordinary identity/shared facts，不借旧 ID 域。新 recovery 入口也必须保留 LiveDelegation 到移交/终态；普通 stable user ID 在 UI 捕获时分配。U/C2/C4：共享配置及 continuation 最终签名 |
| E9 [pipeline.rs](../../crates/vega_conversation/src/agent/pipeline.rs):627–697，`prepare_run_with_images_and_reasoning` | `:647` open store；`:655` delegated owner；普通分支 `:677–686` 先 recover_thread；`:694` 主 immediate transaction | C：独立普通恢复入口先核 sealed eligibility，不先 recover_thread 污染旧证据；所有 SQL 在 conversation/store，不移至 UI。U/C3：新入口的 fence 参数/同进程 writer 接线 |
| E10 同文件 :912–1033 | uses_existing_user 严格批准计划校验；`:949–968` 普通 user insert，`:969–977` image insert，`:979–996` title claim，`:1007` 后新 assistant seq/insert | C：原计划校验保留；恢复事务只新增 assistant，核 user/payload/images/旧 assistant/精确尾序；不新增 user/images/title。U/C3：原子 reservation 和受 fence 保护的 exact compare |
| E11 同文件 :1127–1149 | `:1130` 实际 transaction.commit；`:1131–1138` title launch；`:1149` 才返回 PreparedRun | C：在 commit 成功与 title launch 之间单调记 receipt，含 user seq/assistant seq/identity/accepted payload 归属；不能在 entry 等函数返回才记。写 facts 失败停止新 title/primary，已 commit 不降格为未提交。U/C2：handle Result/核实出口 |
| E12 [entry.rs](../../crates/vega_conversation/src/agent/entry.rs):728–742、820–859 | skill prepare 失败、PersistenceActor::start 失败均 `let _ = finish_prepared_failure(...).await`；actor 成功后 `:850` MessageStarted | C：cleanup 实际成功 Result + worker quiescence 才封 NotStarted/retry 资格；MessageStarted 不替代 durable receipt。U/C2：这些早退及 MessageStarted delivery failure 的 actor drain 责任 |
| E13 同文件 :941–999 | 创建 runtime future `:941`，`:993` join 包进 pipeline，`:996/:998` supervisor/直接 await 驱动 | C：成功写 primary Started 必须早于首次 future poll/drive，不等首 token；construction 不是 drive。U/C2：supervisor 分支、取消、actor/skill watch 及已发布子任务的完整 drain 证明 |
| E14 [window/artifact.rs](../../crates/vega/src/window/artifact.rs):272–282、333–375；[app_agent.rs](../../crates/vega/src/app_agent.rs):496–521 | ingress 匹配 generation/thread/stream；MessageStarted 才 `accept_durable_start` take pending content；queue delivery 是另一分支 | C：普通 receipt 只幂等更新已存在行；与 Started、PendingMessageDelivered、SteeringAccepted/continuation 分离。U/C4/C5：与 #535 最终 ingress/echo identity/owner 移交签名协调，不接管其实现 |

## 2. 不是 app-global exclusion / worker proof 的现有对象

| 精确源位置 | V：能证明什么 | C / U：不能推断 / 最小补口 |
|---|---|---|
| [window/mod.rs](../../crates/vega/src/window/mod.rs):139,147,305,322；[app_agent.rs](../../crates/vega/src/app_agent.rs):369–385、452–507 | 每个 VegaWindow 各自 default AppAgentController/TrustedActionCoordinator；begin 仅拒本 controller active/queue 并分 generation | 不能跨窗口排他。C1：进程内 thread+database claim 引用既有 owner，CAS upgrade/conditional release，不再建 ActiveRun scheduler |
| [trusted_action.rs](../../crates/vega/src/trusted_action.rs):33–83、86–99 | 单个 Arc<Mutex<State>> 的 acquire/release 单飞；cancel_agent_preparation 清 active；lock poison 现有恢复方式不是新 proof 语义 | 释放 lease/清 busy 不证明 worker 已停；新 claim/facts poison 必须 Unknown/fail closed。不能直接把 per-window lease 宣称 app-global |
| [types/running_threads.rs](../../crates/vega_conversation/src/types/running_threads.rs):17–33 | BTreeSet 的 bool 查询/set，无 generation/CAS owner | false 只是不在投影集合，不能解锁、证明无运行或跨窗口发新 worker |
| [subagents.rs](../../crates/vega_conversation/src/subagents.rs):6–34、173–183 | LiveDelegation 是 process static 路径/thread→usize 引用计数；is_live 锁失败返回 false | 不排他、不替代 reservation；保留引用是防 hydration repair 的必要条件但不充分。C2/C3：canonical identity + fence fail closed，不能让 poison false 触发 repair |
| [delegated_ownership.rs](../../crates/vega_store/src/delegated_ownership.rs):166–177 | 仅 parent_thread_id.is_some 的 child acquire；普通 root 返回 None | 不覆盖 ordinary root，也不扩大此卡为通用跨进程 lock |
| [window/mod.rs](../../crates/vega/src/window/mod.rs):192–205、347–357 | on_release 清该窗口所有 active 的 running 投影；terminal cleanup active.drain 后 cancel | 既非 join/drain 证明亦非 conditional claim release；C1/C2：view detach/close 保留 record/payload/worker handle，旧 attempt 不得借 false 重启 |
| [app_agent.rs](../../crates/vega/src/app_agent.rs):588–601 | finish 在本地 identity 匹配后 remove active、stop control | 不能单凭 controller.remove 释放 global claim；仅 sealed+quiescent 精确 generation 可释放 |
| [window/agent.rs](../../crates/vega/src/window/agent.rs):794–819 | AutomationGlobal.runs 写入 generation/weak window owner/cancel | 本处 insert 本身不校验排他，不把自动任务生命周期当普通 proof；是否复用注册容器需父审其全生命周期，不能据命名视为已闭合 |

## 3. 必须参与同一历史围栏的独立入口（有界审计）

围栏保护 recovery qualification → reservation → immediate transaction exact compare/新 assistant commit → 异步准备 → launch admission。不是 SQLite 锁跨网络，也不是 launch 前再读一次。阻止的是与该提交资格/请求源冲突的**同进程已授权** writer；合法 primary/cleanup/title/#535 continuation 必须有明确 owner-authorized 模式，不笼统旁路。下表均为 U/C3，尚未声明已接入。

| mutator / 精确调用链 | V：写入与竞态证据 | C：所需接入 / 仍需核验 |
|---|---|---|
| W1 sidebar 删除 | [vega_ui/sidebar/mod.rs](../../crates/vega_ui/src/sidebar/mod.rs):449–465 `confirm_pending_delete` → [conversation/threads.rs](../../crates/vega_conversation/src/threads.rs):347–366 `delete_thread` → [store/threads.rs](../../crates/vega_store/src/threads.rs):246–252 transaction delete messages/tool_calls/thread。conversation 只按 child 自身/子 agent live 检查，普通 root 本身的 LiveDelegation 不构成拒绝 | 删除须在 conversation/service mutation 边界加入同一 claim fence，不能只给发送按钮加 guard 或依赖 task_mutation_busy。确认 UI 可反映冲突，UI 不做 SQL。删除开始前与 claim 建立互斥顺序，不能先 SELECT 再放行 |
| W2 plan-review 持久化 | [window/session.rs](../../crates/vega/src/window/session.rs):862–924 `review_plan`/`finish_owned_plan_review` 用本窗口 busy/active → [thread_reload.rs](../../crates/vega/src/thread_reload.rs):23–47 `persist_review` → [conversation/plans.rs](../../crates/vega_conversation/src/plans.rs):93–139 → [store/messages/mod.rs](../../crates/vega_store/src/messages/mod.rs):227–305 immediate transaction：改 plan、mode，批准时取 next_seq/插 user instruction 后 commit | 将 review/approval write 与 ordinary recovery fence 序列化，包括另一窗口、handoff 后的最终持久化；继续原 first-wins/批准计划授权，不放宽 uses_existing_user。计划启动也参与现有 owner 的 launch admission |
| W3 历史 hydration repair | [window/render.rs](../../crates/vega/src/window/render.rs):320–330 视图创建即 `restart_history_page` → [conversation/history.rs](../../crates/vega_conversation/src/history.rs):183–197：recover_children；非 is_live 时 recover_thread → [store/recovery.rs](../../crates/vega_store/src/recovery.rs):27–35,39–63：immediate transaction/recover_stopped_with_owner；child repair 在 [conversation/subagents.rs](../../crates/vega_conversation/src/subagents.rs):126–148 | 新 recovery 入口从最早准入起持 LiveDelegation 到终态/有效移交；repair 的 live-check→事务写之间也需同一 fence，防另一窗口先读 false 再迟到 repair。锁毒/事实不明不能当 orphan。只读最新页与 repair 要区分，不能删 repair 或扩大崩溃恢复产品 |
| W4 manual context 独立请求/写入 | [window/context.rs](../../crates/vega/src/window/context.rs):132–191 `start_context_compaction` 仅本窗口 active+trusted lease；`:676` worker，`:772` 调 accounted compaction → [agent/compaction.rs](../../crates/vega_conversation/src/agent/compaction.rs):722–815 读 projection/写 started status/执行摘要，`:1904–1951` spawn blocking install → [store/context_compaction.rs](../../crates/vega_store/src/context_compaction.rs):1064–1085 immediate transaction/source CAS | checkpoint/status/source/context settings 可改变恢复请求的有效上下文，须界定冲突类别和 fence/token 的 worker 寿命。现有 source CAS 不是 app-global 准入，也未保证普通恢复 commit→launch 区间。无 SQLite transaction 跨摘要 provider。保留权限/credential/model identity 守卫 |
| W5 fresh run、queue/steer、automation/plan/credential 及 actor 写入 | E3–E14 已定位普通 shared start；entry 与 pipeline 中 approved-plan/credential/pending 是独立域；actor/主运行是获授权的同线程历史 writer | 原非普通产品路由仍在，但其实际同线程 writer/launch 要参与排他准入或明确合法移交；不能只防第二次 ordinary SELECT。#535 continuation 与 pending echo 最终接口未取得，父需协调实际签名后列出 owner-authorized writer；本轮不抢写或泛化 scheduler |

**剩余范围明确**：W1–W4 是检查点指定、已核到的最小独立入口，不是全仓 mutator 穷尽结论。W5 的 queue/continuation/actor 全部出口、model/permission/skill 变更对 frozen intent 的冲突策略、元数据/title 的可兼容类别仍须父在最终接口上定点关闭；未关闭前 C1–C4 都不能写“恢复安全/可实现”。不以无限审计延迟本文档交付，也不因有界审计而忽略这些 blocker。

## 4. 恢复 reservation 与密封事实的操作序列

| 步 | 候选操作 / 事实义务 | 关联证据 / 未决 |
|---|---|---|
| R0 | 同一 ordinary claim 下验证 Committed receipt + sealed NotStarted + CleanupSucceeded + WorkerQuiescent；持有 accepted 正文/图片和预期 user/old assistant/tail | E11–E13；空 failed 或 NotYetDriven 不构成 proof。原 attempt handle/lifetime 尚需 C2 接线 |
| R1 | 原子 reserve 新 attempt，一次消费旧 eligibility；重复请求观察 reservation，不再插入/驱动；最早处保留 LiveDelegation | §2 live/refcount、W3；取得 ref 与 repair 准入必须同 fence |
| R2 | conversation 独立 recovery 入口；immediate transaction 精确比对 thread/role/kind/status/accepted content、图片数序字节、旧 assistant failed/ID/seq、无后续历史、expected next_seq | E9/E10；不能调用普通 recover_thread 或 approved-plan uses_existing_user 假装通用恢复 |
| R3 | 事务内只插新 assistant ID/后续 seq，登记新 attempt；commit 后立即事实 receipt；不插 user/images、不重新 title claim | E10/E11；reservation 若失败也不能复活旧资格；新事实决定核实/幂等继续 |
| R4 | 释放 SQLite transaction，继续原 skill/actor 准备，但保留进程内 fence；W1–W5 冲突写入不能偷过 | W1–W5；fence 涵盖检查到 launch 的空隙，不是再 SELECT |
| R5 | 同身份 launch admission/cancel 检查，先成功 markStarted，再首次驱动 primary；明确交给现有 ActiveAgentRun 的历史写入规则 | E13；异步 supervisor、actor/skill watch 的移交及终态待 C1/C2 确定 |
| R6 | 失败/取消记录 commit、cleanup 实际 Result，封新 publish 能力并 drain 所有登记 worker 后密封；conditional release compare 原 claim/generation | E12、§2 window cleanup；disconnect/join error/cancel 都不能单独证明终态。未知保留核实、不重投 |

## 5. 历史与展示兼容，不顺带改产品

- [pipeline.rs](../../crates/vega_conversation/src/agent/pipeline.rs):220–277 `history_from_context_source` 排除 current assistant、streaming、minimum seq，不普遍过滤空 failed；旧 failed 无工具时仍进入 chat history。
- [history.rs](../../crates/vega_conversation/src/history.rs):744–759 对 failed/interrupted 即使空文本仍产生 AssistantText；UI 已消费该状态。旧 assistant 的 failed/ID/seq 保留，新恢复 assistant 是新 ID/后续 seq。不得用 fixture 隐藏旧项或本卡顺带过滤 UI/模型历史。
- [threads.rs](../../crates/vega_conversation/src/threads.rs):193–198 `draft_thread`/`materialize_draft` 及 [window/session.rs](../../crates/vega/src/window/session.rs):279–292 为稳定 draft thread/materialize 边界；不得新建另一个逻辑 thread 来绕 claim。
- #535 活跃/共享 pending、continuation、SteeringAccepted：仅有父检查点/本地 shared 接口证据，**其实际最终签名未取得**。需父协调唯一 shared-symbol writer/集成基线；不改其验收，不用 #544 receipt 消费 queue echo，不从旧 ref 覆盖代码或恢复历史测试集合。

## 6. 下一次核验与停止条件

1. **先关 P1-C1**：父 review 明确一个 app-global claim/存活 owner 保存位置；画出两个窗口同 thread、preflight→active upgrade、window release→worker drain 的 generation/token 序列。指出所有 fresh-start 和 W1–W4 的实际 fence 参与点，验证不是投影、weak-window cleanup 或第二 scheduler。
2. **再关 P1-C2/C3**：定点复核 E11 的 commit→receipt→title、E12 cleanup Result、E13 首次 drive 前 Started；对 W1–W5 给出 reservation→transaction→launch 的具体授权/拒绝及失败释放序列。不能用 InProgress/CommitInFlight、join/cancel 作为密封 proof。
3. **P1-C4 并行协调但不接管**：父取得 #535 实际最终 continuation/pending/ingress 接口，串行冻结 shared symbols/移交再决定实现授权。
4. **P2-C5/C6 行为证据待授权/云端/用户**：保留 A1–A12；高风险对照见候选 v2 §6，不强制 TDD/有效 RED。本文仅文档，未运行 Rust/产品/CI，无行为 PASS。

本轮交付完成于文件实际保存及最终静态核对；`READY-doc` 仅供父独审/集成决定。所有未决保留，不授权生产代码；最终回报后停止本任务全部写入。

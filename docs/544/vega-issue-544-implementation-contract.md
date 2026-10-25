# #544 实现契约：普通发送的窄准入与可恢复快照

版本：implementation v1，2026-10-25；主 Agent 接手后冻结的实现动作。保留[主规格 A1–A12](vega-issue-544-send-echo.md#1-验收矩阵先于实现方案)，不以本契约缩减验收。[本轮来源与事实](vega-issue-544-takeover-interface-closure.md)记录旧候选和本次源码复核。旧 v1/v2 的“本轮仅文档/不可实现”是旧阶段边界，不是本次用户确认接手后的授权；代码仍须先满足以下分阶段契约，不能先只做气泡 happy path。

基线：master `b03d0d5f2c7da0efab147a4fec0d26e2adefc4e3`（包含 #535/#603），任务集成 HEAD `8e417a34ff94fb89a2d742a5c2556352f73d52f6`，worktree `vega-544-send-echo`。仅新增本契约及接手记录，不改历史规格字节。两个只读调查均中断，没有完整 READY 或独审通过；下面由主 Agent 对已复读源码作设计决定，后续实现独审独立进行。

## 1. 非目标与不变量

只使 Native 非运行普通人类发送即时可见；不扩 Codex/ACP、queue/steer、automation、批准计划或凭据续跑产品能力，不增 schema/outbox/跨进程崩溃恢复或运行调度器。原模型、reasoning、技能、权限、路径、凭据与危险确认不降低。所有敏感载荷仅保存在对应不可变快照或既有持久化边界，禁止 Debug/日志输出正文/图片/凭据。

一次逻辑发送始终一条 user、一组原图片；准备重试有新 attempt；已提交未启动恢复只插新 assistant，保留旧 failed assistant 及既有 UI/模型历史投影。ID/空输出/failed/MessageStarted 不是提交或主运行未启动证明。确认只补归属/状态，不能清草稿、换 key/MessageCopy 或额外导航。

## 2. 阶段 F：先交付可审查基础类型，不提前接管产品路径

### F1. 数据库身份与 thread admission

在 `vega_store` 新增窄 `thread_admission` 模块，Store 打开连接时缓存 `DatabaseIdentity`，文件库以 device/inode 识别真实数据库，in-memory 每个 Store 独立唯一身份。缓存读取不做 metadata/canonicalize I/O。既有 #576 短写协调不改语义，不跨 async 持它的 permit，不替换其事务队列。

`ThreadAdmission` 是进程内 thread + database identity 的排他 capability，不决定任务先后、不存运行任务、不等待 UI、不给新 scheduler。全局 registry 必须 fail closed，锁毒不能当空闲。建议对外最小接口：

- `Store::database_identity()` 返回缓存的强类型身份。
- `thread_admission::try_acquire(database, thread)` 返回 `Result<Option<ThreadAdmission>, ...>`：已有有效 capability 返回占用，锁毒返回错误。
- `ThreadAdmission` 可克隆，由 worker/async 子任务明确持有；`verifies(database, thread)` 校验精确身份；最后一份 capability 释放才允许新 owner。每次新 owner 唯一 token，旧 clone/drop 不释放新 token。
- 持有能力不自动允许改变其它 thread；没有可公开伪造 token/从布尔投影升级的构造口。

registry 可以弱持 capability，不允许仅由 weak window 决定生命。普通记录的 active capability 放可清空的槽位，恢复内容与 busy 生命分离；worker 自身/被发布的阻塞准备任务持有 clone，UI drop 不可能独自解锁。异常 drop 只证明该 worker 不再有该 capability；普通 attempt 的提交/启动结论仍必须 Unknown，不能由 RAII 自动创造恢复资格。

### F2. 普通提交 handle 和事实

在 `vega_conversation` 新增 `ordinary_submissions` 模块，共享身份/投影可从 `types` 导出。`OrdinarySubmission` 为强持快照和有序 attempt facts 的窄 handle；不得依赖 GPUI Entity 或 UI 生命周期。process registry 按数据库+thread 保存未决普通提交，普通记录保留未提交快照/可恢复错误，完成并已由历史承接的记录才可退役。此模块不启动线程/provider、不访问 UI 或做 SQL。

最小身份/载荷：submission ID、稳定 user message ID、thread/database、attempt counter、冻结模型/reasoning、原始文本及图片对象、草稿 revision；accepted 引用展开正文单独一次绑定，不从 safe_live_text 或新草稿重建。图片保留既有 `ImageAttachment`（不可变共享字节），不复制磁盘 outbox。Debug 只可打印内容无关身份/长度或直接不实现。

最小事实接口须携 attempt 校验并单调：begin/register worker、bind accepted payload、begin commit、record committed receipt、record cleanup Result、mark primary Started、finish worker/seal、snapshot。非匹配 attempt 的迟到结果无效果。新 attempt 只可在旧 attempt 已密封且允许重试时创建，同一资格单次消费；重复调用返回现有结果/拒绝，不再分配 assistant。

- `Commit`：NotAttempted/Preparing/CommitInFlight/Committed(receipt)/Unknown。receipt 包含真实 user/assistant ID、seq、实际持久化 user/image 身份和该次历史边界；commit 之后才产生。错误发生在 commit in-flight 不自动变未提交。
- `Primary`：NotYetDriven/Started；只有完成登记 worker 收敛且无更晚启动可能时，NotYetDriven 才能密封成 NotStarted。Started 永不回退，零输出也一样。
- `Cleanup`：NotRequired（确定未落库）、Pending、Succeeded、Failed/Unknown。不能把调用过函数当成功。
- worker：spawn 前登记；未成功 spawn 显式结束未发布 guard；blocking prepare、主 worker、actor/skill watch 等影响 primary/history 的子任务须持登记能力。只有完整收敛可 seal；panic/丢 handle 的 drop 标异常，拒绝自动重投。
- 未提交重试需 sealed known-not-committed + quiescent；已提交未启动恢复需 committed + sealed NotStarted + cleanup succeeded + quiescent；Unknown/Started/后续历史变化拒绝盲重投。

阶段 F 的提交不接入运行/UI，不能宣称 A1–A12 已实现。必要开发测试验证真实能力互斥、数据库别名、两 thread 独立、drop/clone 生命周期、旧 attempt 无效、commit-in-flight/Started/cleanup失败/未收敛拒绝重试、并发资格只消费一次；不只断字段赋值。只交 Actions，不在本地执行。

## 3. 阶段 I：同一 writer 串行接入，不改已交付 queue 行为

F 固定可审查后接线：

1. `window/agent.rs::submit_composer` 做原有无 I/O 路由和 reasoning 门禁，排除非普通后从 Store 缓存身份取得 admission、建立 ordinary record，同一 UI 事件先调用普通 pending 展示再发布 preflight。重复同事件不把首份标失败。普通 submission 在 app/stream event 上携稳定身份，不用文本比对。
2. 普通 preflight、materialize、open_thread、workspace 解析及 skill-pin 在后台同一能力下顺序准备；现有服务的权限/路径校验仍执行。原 thread ID 不变；成功回 UI 后 checked-upgrade 到既有 `AppAgentController::begin`，不 release/reacquire 全局能力。只避开自己的 trusted lease，其他 busy/model/权限守卫照旧。后台 worker spawn 前登记；相同 capability 随用户 run 传入 `UserSubmission`/config。
3. `run_agent_worker_with_mcp` 普通分支先按原规则引用展开并一次绑定 accepted payload，重试已绑定载荷不得重读文件；`PersistenceActorConfig::with_ordinary_submission` 传可选 handle，不改 queue/批准计划/凭据身份分支。常规 run 返回前最后处理 worker facts，不依赖 UI 通道交付成功。
4. `entry.rs` 在原所有 ID 分支旁明确普通稳定 user ID/attempt assistant ID，严拒与 delegated/pending/credential/persisted instruction 混用；无普通句柄保持原语义。`pipeline.rs` 的普通恢复是独立分支，不放宽 `uses_existing_user`。初次 user/images/assistant 在同一事务；恢复精确比较原 user、实际持久化正文和图片顺序/字节、旧 failed assistant、尾序/next_seq，原子仅插新 assistant。保留原始/accepted/安全展示/实际持久化的不同域，现有 credential redaction 后的真实持久化不能用原文或展示文本代替比较。
5. `pipeline.rs` 在 commit 开始前记 CommitInFlight，成功后立刻写共享 receipt，再允许现有自动标题 launch。title 仅首个真实提交触发，恢复不重复 claim。标题使用独立受限 capability（只写原 title/usage 元数据），不算 primary Started，不允许改 user/assistant seq。其独立后台寿命不能被宣称为主 pipeline 已 drain；若仍持 thread admission，结束前继续占用而非提前放权。
6. `entry.rs` 在 skill/actor/UI事件失败出口记录实际 cleanup Result；primary future 首次 drive 前成功 mark Started 并检查取消/精确能力，随后运行。actor/skill/primary drain 和实际清理完成后才能 seal；事件通道只是唤醒 UI。异常未知保留记录和核实入口。
7. 普通确认 ingress 读取共享 receipt 原位确认；旧 `accept_durable_start` 不再为这条普通记录 append。仍有非普通旧调用时保持兼容。Finished/取消只更新对应身份；窗口关闭只 cancel/detach，已发布 worker 持能力直到收敛。

## 4. 阶段 G：冲突 writer 必须在同一准入下闭合

这些是上述安全恢复的组成部分，不是另一个任务：

| 边界 | 最小参与方式 |
|---|---|
| 所有 Native fresh start，包括 automation/批准计划/凭据/queue dispatch | app start 在产生该次运行写入/worker 前取得或校验 admission；conversation common entry 无显式能力则自行取得，有则 verifies。内部函数不重复抢自己的能力。child 使用自己的 thread 身份，原 delegated 机制继续 |
| `threads::delete_thread` | service 入口先取得独占短 mutation capability，贯穿原事务；与普通 preflight/recovery 原子互斥。原 child/live 删除检查保留 |
| `plans::review_plan` | 从读取/审批决定到 review transaction 持短 capability；审批结束释放后后续 fresh start 仍重新按原未启动恢复语义准入，不伪称跨阶段启动成功 |
| `history::restart_history_page` 及 child repair | 先尝试 mutation capability，再判断 live/repair；占用时只读 latest page，不把“锁忙”当 orphan，也不删现有恢复机制。child repair 按 child thread 再独立参与，不能父 capability 通杀子线程 |
| 手动 compaction / context settings | service 顶层取得 capability，手动压缩跨 provider/落库保留，设置 mutation 持短 capability；自动 primary compaction持原 owner，不误拒绝自己 |
| thread model/mode/permission、线程技能选择及影响恢复输入的修改 | service mutation 同准入；普通准备内部 skill-pin需显式 verified owner variant。元数据 title/pinned/unread 不改变输入/seq，可按原语义兼容；archive/delete 或项目身份变化不可无条件兼容 |
| queue/steer | enqueue/reorder/remove 不进入本卡普通状态机；dispatch 取得 fresh capability；正在运行的 steering/continuation 复用当前 owner能力及 pending generation，不再抢普通 claim；继续原 accepted transaction/旧段过滤 |
| primary/cleanup/持久 actor/title | 只持本 owner 或明确定义的 title受限能力；不能用全局“内部调用”bool 绕过身份 |

所有 admission 尝试都不在持 registry mutex 时做 SQL、I/O、provider、await 或 UI 更新。不把原 #576 写锁作为跨 await 能力；锁顺序固定先短准入检查/取 capability，再原 Store 写事务。业务冲突错误就近可恢复，不虚报保存成功。

## 5. 阶段 U：真实列表、稳定身份和恢复入口

`conversation_stream/ordinary_submissions.rs` 为 UI 接线（必要时新增）：pending 在任何 I/O 前插真实 User/UserImages；纯图无空正文。首次插入用稳定 user ID 生成 `user-text`/`user-images` key，带独立 ordinary metadata，不挤进 pending echo。将捕获 revision 的文字/附件只转移一次，立即给可编辑新草稿；确认不清新草稿，即使同文不同 revision。

确认只补 message ID/seq/状态，不换 key/MessageCopy/选择，不重复 list_append 或滚动。历史分页按稳定 user ID 去重；现有同一 view 的确认保留对象，新 view 可重新建视觉对象但先按 durable ID 合并再投影未决记录。记录存在于进程 registry，不靠同一 view 永存。

失败气泡下仅出现当前可操作的错误/Retry；Unknown 显示核实动作而非盲重发。Retry/核实事件携 submission+attempt；只重试旧快照、不从新 Composer 取值、不覆盖新草稿。按钮有文字名称、键盘焦点与 Enter/Space，disabled原因可获取。取消后内容保留；已提交不回滚历史。未提交可重试，已提交但未启动按 G 安全恢复，Started 后沿既有停止/继续语义，不冒充一次新 user。

## 6. 验证与交付

### 2026-10-25 接线检查点的有界补充

集成检查点 `5805080462bd77c4695ee1bb26a849a1b3cc3366` 已保存，不是完整 READY。下列补充闭合既有 A1–A12，不减少验收：

- 自动标题保留原独立 capability 到实际结束；普通主运行结束后若队列因 title drain 暂时未获 thread admission，不把原 queue 标成失败/暂停，不提前释放 title 能力。仅此可识别的等待原因登记一次可取消的重分派唤醒；每次唤醒重新验证 thread/stream/queue generation、暂停/停止/权限状态和真实 admission。占用解除后按原队列一次分派，不在失败未知时无限盲重试，不改变全局调度策略。等待的取消必须经真实 Composer Stop 控件/动作可达，不只在 controller 内有 cancel；等待态独立投影，不伪装 primary-running 以改变发送路由。有界等待超时后保留队列并转 paused/error，显示可用的“继续队列”恢复入口；不得只提示稍后继续却没有动作。受控时钟越过期限、释放能力并经真实 Resume 入口继续时恰好分派一次。
- raw reasoning 为 None 不代表每次重试可重新选择 provider/default。第一份实际解析成功的 provider identity、resolved model authority 与 effective FrozenReasoning 按 submission 一次绑定；后续 attempt 使用相同逻辑 choice/authority，配置变化不静默转 provider。凭据和当前安全可用性仍重验，原 route/model 失效显示可恢复错误，不借旧快照绕授权。first binding 在后台、先于 accepted payload/provider primary，UI pending 不等它。
- 为验证真实列表/controller 的必要行为，允许仅开发期在 `vega_ui`/必要 `vega` 的 dev-dependencies 启用已经锁定 `gpui_kit` 的 `test-support` feature。只复用现有依赖，不升级/引入新产品依赖；不改通用 CI。先用正式历史用户行作阳性对照，再验证实际提交/回执和列表对象，不能用源码字符串或镜像 helper。所有测试和独占 feature/接线在真实云端验证留痕后移除，最终 head 仍须 Rust 0 发现/0 执行。此授权不允许本地编译或执行。
- 固定 `5805080462bd77c4695ee1bb26a849a1b3cc3366` 的独审发现须闭合：child-specific admission 在 resume validation/create 写入前取得，贯穿 host/runtime/terminal persistence/release bookkeeping，所有 detached blocking mutation 持同一能力 clone。不能先写后取、runtime 后提前 drop，quarantine 不得凭空释放仍有副作用的 owner；两个生命周期边界的冲突 service mutation 必须拒绝。
- history-window replacement 保留未决 ordinary 记录及可发现的内容/恢复控件；记录已跟踪不等于视觉行仍存在。按稳定 user ID 合并/重新投影，text/mixed/image-only 不丢失，已提交用户不重复，当前定位目标/锚点不被恢复投影抢走。旧 submission/attempt 的延迟或重复 Cancel 只取消自身 token/匹配控制器归属，不能 thread-wide 取消后来运行，也不能改变其 computer-use controls。
- 上游 master `429c6a819fc5a30f8280afd2f6ed015d4682bf41` 的 fork/history privacy 与 artifact 改动不回退。集成冲突中 `restart_history_page` 先保留 fork 历史 guard 再按本卡 admission 决定可否 repair，忙时只读；store 同时导出 thread_fork/thread_admission。新结构字段按现有语义完整接线，不借合并扩大 fork 产品入口或放宽安全校验。


保持完整 A1–A12，开发验证按风险选择必要真实生产路径，不造 toy controller。至少覆盖：扣住 prepare 的唯一可见 pending；同文新 revision/新附件保留；双击和重复 receipt；commit 前失败和 commit 后 actor失败的唯一 user/images；commit Unknown/Started但空输出/cleanup失败禁重投；取消与迟到事件；两窗口占用和 view销毁；真实列表对象/key/锚点；#535 queue/steering 与非普通路由保全。

行为/存储/GPUI进程内测试只在 Actions；正式历史行阳性对照先证明 harness 可观测，不用源码字符串冒充 UI。必要测试本轮保留开发源码/patch/完整 SHA/真实结果，最终独立清理 Rust 测试及独占支持，再对最终 head 跑完整 required CI；Python/通用CI不改。无本地 Vega运行、cargo check/Clippy/build/test、打包或 CUA。真实 UI时延/系统集成仍由用户手测；CI不代表用户验收。

主会话仅写 spec/Git/协调，生产由单一专职 writer；F→I/G/U 串行，非写入者独审固定停写版本，最后 head required成功才合并。任何实际 API 或产品边界无法按本契约满足时先带精确证据返回主 Agent，禁止自行降低验收或只交 happy path。

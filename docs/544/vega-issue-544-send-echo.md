# #544 原生普通发送：即时待提交气泡

版本：spec v4；保留 v3（2026-10-22）的 A1–A12。当前接口见[候选 v2](vega-issue-544-ordinary-interface-proposal-v2.md)及[精确未决调用点图](vega-issue-544-ordinary-unresolved-callsite-map.md)；app-global claim、密封证明、历史围栏与 #535 最终接口仍有 P1/P2 未决，状态为 READY-doc / NOT code-ready。按当前政策按风险验证，不强制 TDD/有效 RED；本轮仅文档，未编写/执行测试。

## 1. 验收矩阵（先于实现方案）

证据层级：P＝真实生产对象的云端进程内行为；S＝云端生产存储/事务边界；H＝合并后用户纯手动验收。下列均为待取得的证据，不是 PASS。原始来源为 [Issue 正文](https://github.com/puzige/vega/issues/544) 和 [2026-10-22 最新协调评论](https://github.com/puzige/vega/issues/544#issuecomment-6345780871)；Issue 不代替本规格。

| ID | 前态 | 操作 | 可观察结果 / 不变量 | 证据与对应测试包 |
|---|---|---|---|---|
| A1 | Native、非运行、合法文字；预检/启动准备尚未完成 | 点击发送或发送快捷键；扣住后台完成回执，推进下一次可绘制周期 | 本次文字在真实列表中形成唯一待提交气泡；不等后台回执；不显示持久化成功；Composer 可编辑下一份草稿 | P：T1、T2 的真实事件、行 bounds、内容与 pending 状态；H：实际下一可呈现帧，不以“16ms”硬编码 |
| A2 | A1 的气泡已展示；用户已输入新草稿 | 交付同一 owner 的持久化确认，再交付重复确认/MessageStarted | 原气泡身份、位置和选择对象保留；原位转正式，不再追加 User/UserImages；重复回执无第二次效果；新草稿不被清除 | P/S：T2、T3；比较稳定 key、相邻顺序、列表数量与存储唯一性，不能只断言 emit 次数 |
| A3 | 待提交期间输入新内容，或删改后重新输入与旧内容完全相同的文字；附件也可能新增/移除 | 后台确认、失败或取消 | 依据草稿 revision/提交归属，不依据字符串相等清输入；新草稿和新附件均不丢 | P：T3；同文不同 revision 是必需反例 |
| A4 | 同一普通发送尚在 preflight/preparing；尚未成为运行中队列请求 | 快速双击、连续 Enter、重复控制器投递 | 一个逻辑提交、一组气泡、一次有效启动；不能因第二次事件将第一份标失败；编辑仍可用 | P/S：T1、T3；准备请求与最终持久化结果一起断言 |
| A5 | 尚未提交事务 | 预检失败、配置/权限/路径校验失败、worker 启动失败、事务失败 | 保留本次文字和附件；就近真实失败及明确重试入口；不产生正式消息或模型调用，不残留 busy；不覆盖新草稿 | P/S：T4；各独立生产故障阶段一个代表样例，其他 gate 表驱动 |
| A6 | 用户与助手占位已经提交，但 actor 启动失败或确认通道丢失 | 失败后重试/重新核实 | 区分“已提交但未启动”与“确认未提交”；不得伪报未提交后再插一条；未知结果核实前不得重新投递；原内容、图片及恢复入口仍可获取 | P/S：T4、T5；事务终态、typed receipt、刷新历史后唯一用户消息 |
| A7 | 准备中、提交临界点或已经运行 | 点取消/停止；再交付迟到成功或失败 | 未提交取消保留可重试内容；已提交不撤销真实历史，停止运行按原语义；只结束对应 attempt；取消与提交竞态不产生重复消息、悬挂忙态或假成功 | P/S：T5；H：停止按钮、键盘与原生反馈 |
| A8 | 提交快照包含图片，含纯图片和文字+图片两种；用户随后改附件 | 确认、失败、取消、重试 | 原图片快照属于原提交；新附件属于新草稿；图片预览/字节存活到恢复或确认，不能借移除 Composer 附件释放待提交数据 | P/S：T3、T4；H：真实粘贴/导入与预览 |
| A9 | 线程 A 待提交；切至 B，或销毁 A 的视图后重开；旧事件晚到 | A 的回执、重试结果与 B 的新提交交错 | B 的列表、草稿、附件不变；A 按其 owner 收敛；不得把旧内容放进 B、清 B 草稿或复用 B 的 attempt | P：T5；H：多窗口/真实切换；未持久化快照不能因普通切换静默丢失 |
| A10 | Native 正在运行，已有 Queue/Send-now/steering；或 Codex 路由、自动任务/计划/凭据续跑 | 使用既有发送、停止、队列操作或交付 continuation | 只走既有路由，不套用本卡普通发送状态机；保留 #535 的 pending echo/SteeringAccepted/continuation；Codex 不扩大能力；停止与权限入口仍有效 | P：T6；H：真实后端。#535 冻结测试在集成后的生产树重验，不以本卡通过替代 |
| A11 | 已脱离底部或有消息选区；待提交后确认 | 提交/确认/失败状态变化 | 确认不重建同一气泡的 MessageCopy、选区与邻居；不因确认额外滚到底部；普通发送已有的显式导航策略之外不抢阅读位置 | P：T2、T3；H：拖选、复制和长历史阅读 |
| A12 | 真实应用、长会话、受控慢准备 | 用户记录点击、首次可见内容、持久化/后台确认阶段 | 分开报告两个阶段及真实 UI 阻塞；不把静态发现或合成 draw 当真实时延/帧率实测 | H；P 仅记录调度顺序，所有毫秒数目前未知 |

## 2. 基线、约束与已核实事实

- 分支 `feat/544-send-echo`；独立 worktree `vega-544-send-echo`。只写本文件所在的 `docs/544/`；本阶段不写源码、Cargo、workflows、`docs/README.md`，不 commit/push/建 PR/改 Issue，不嵌套委派。
- 本轮 HEAD：`871f91f246e90753743a256cee1eec315e74effc`。首次写前于 `2026-10-22T16:38:20Z` 开始的核对中执行 `git fetch --no-write-fetch-head origin refs/heads/master:refs/remotes/origin/master`；fetch 后 HEAD 与 `origin/master` 相同，分叉计数 `0 0`，工作树干净，分支无需更新。未操作主仓工作文件。后续有相关远端增量交主 Agent 协调，不覆盖他人数据。
- 已读两 worktree 相同的根 AGENTS，沿路径没有嵌套 AGENTS；必读 [执行指南](../vega-exec-guide.md)、[Issue 工作流](../vega-issue-workflow.md)、[设计守则](../vega-design-guidelines.md)、[交付 skill](../../.agents/skills/vega-kanban-delivery/SKILL.md)、[Rust 测试生命周期](../vega-rust-test-delivery-lifecycle.md)。本地禁止 Vega、构建/check/Clippy、测试、bench/profile、CUA；仅静态检查。
- 当前基线包含 #529 Markdown 引用修复、vendored mdstream，以及 model/render_rows/message_anchors/thinking_body 更新；不从旧 `83fd` 或其他历史 ref 恢复文件/旧测试。

以下为 v3 历史基线的静态定位；当前父提供 HEAD 与本轮已核调用点见[调用点图](vega-issue-544-ordinary-unresolved-callsite-map.md)，后续以符号定位而非机械套行号：

| 只读证据 | 事实与含义 |
|---|---|
| `crates/vega_ui/src/conversation_stream/composer.rs:150–195`，`can_submit_message` / `submit_message` | guard 后仅设置 `composer_submit_pending`、冻结附件并 emit `ComposerSubmitted`；普通发送没有本地气泡；`pending_submission_revision` 只在 running 时记录 |
| 同文件 `:201–253`，`accept_composer_submission` / `reject_composer_submission` | accept 才追加 User 和 UserImages，按文字相等清输入；reject 清 submitted attachments 并重置 guard。提前调用 accept 会混淆展示与确认，不能作为修复 |
| `conversation_stream/mod.rs:130`，`ComposerSubmitted`；`crates/vega/src/app_agent.rs:32–58,509–524` | 当前事件没有普通发送 request/attempt 身份；`UserSubmission` 只有 content/images；`accept_durable_start` 校验 generation/thread/stream 后 take pending content，不能单凭文字解决跨 attempt 归属 |
| `crates/vega/src/window/agent.rs:1139–1260,1710–1761` | `submit_composer` 先做路由/预检；preflight 已在 worker，但回 UI 的 `finish_agent_preflight` 会同步 materialize draft；Native running 路由 `enqueue_composer`，Codex 有独立分支 |
| `window/agent.rs:759–787,891–938`；`window/session.rs:279–292`；`vega_conversation/src/threads.rs:448–497` | UI 同步 `open_thread` / `task_workspace_root` / draft materialize；独立任务目录可能 `create_dir_all`；worker spawn 错误需要释放运行 owner。只确认存在同步 I/O，未测耗时 |
| `window/artifact.rs:250–359` | ingress 先校验 controller generation/thread/stream，匹配 `MessageStarted` 才 accept；queue delivery 是不同分支，必须保留 |
| `vega_conversation/src/agent/entry.rs:637–705,813–859`；`agent/pipeline.rs:767–794,825–845,944–969` | 准备 worker 生成 user/assistant ID，事务写用户、图片、助手占位后 commit；随后才 actor start 和 `MessageStarted`。actor start 失败时可能已经 durable，“未收到 Started”等同“没落库”是错误前提 |
| `conversation_stream/core.rs:605–614,665–715,746–843`；`model.rs:21–27,849–862` | 列表有独立 entry identity、局部失效与滚动快照；MessageCopy 持有选择状态。确认应更新归属/状态，不替换对象或重复 splice |

根因结论：消息可见性被后台准备/确认门控。实际掉帧主因、用户安装版本、点击至帧的时延均未测，不承诺性能收益数字。

## 3. 范围与状态机

### 3.1 路由先分清，不提前伪造成功

本卡仅覆盖 `Native + 非运行 + 普通用户提交`。`submit_message` 保留完整 guard 和输入快照；控制器完成纯内存路由判断，排除自动任务拦截、运行中队列、Codex、计划批准/凭据续跑后，在任何启动 I/O 前同步建立普通待提交展示并 notify。不能将“Native 非运行”直接等同普通任务而误吞 `intercept_automation` 分支。

拟新增的名字是设计接口，不是现有 API：普通提交身份 `NativeSubmissionKey`、本地展示状态、typed 提交结果。跨 crate 的共享身份/结果放 `vega_conversation::types`，不在 UI/app/conversation 各造同名结构；不为测试扩大 production public API。

一个逻辑提交拥有：线程/路由 owner、稳定本地 entry key、submission ID、attempt generation、提交时文本 revision、不可变文本/图片快照、冻结模型/reasoning/skill intent 和持久化身份（确认后才有 durable 标志）。图片行与文字行可保持现有两条 entry，但由同一逻辑提交拥有、作为一组只插入一次。ID 的存在不证明已持久化。

### 3.2 状态与转移

| 状态 | 进入条件 | 展示、可用动作 | 退出与不变量 |
|---|---|---|---|
| Draft | Composer 当前 revision | 现有可编辑输入/附件 | 合法普通发送原子捕获快照；只清走归属于这次快照的 Composer 内容，马上得到新的草稿 revision |
| LocalPending | 普通路由确认，尚无 durable receipt | 用户气泡展示本次真实内容，局部“待提交”语义及取消；Composer 可编辑 | 在后台预检/准备前建立；不得进入历史成功、计费、导航 durable owner 或持久化消息计数 |
| Preparing | 后台处理该 attempt | 与 LocalPending 同一气泡；不加常驻“正在准备”栏、占位动画或 Composer 高度变化 | 有明确 owner 的取消 token；任何回执均需 key + attempt + thread/stream 校验 |
| FailedUncommitted | 有明确证据事务未提交 | 保留原气泡/图片，真实错误及“重试”；新草稿独立 | 重试只用旧快照，原位换 attempt；不把当前 Composer 的文字/附件冒作重试内容 |
| CancelledUncommitted | 取消已确认且未提交 | 保留可恢复内容和重试入口；不显示通用错误 | 迟到旧 attempt 不得再清草稿、重新启动或生成第二份气泡 |
| Reconciling | 通道中断/取消竞态导致提交结果未知 | 内容保持；明确核实中/核实失败与可恢复入口；在结果不明时禁用重投，原因可见 | 通过 conversation 层查询冻结 user ID/receipt，不能以超时猜测未提交；可重试核实，不盲重试写入 |
| Committed | 真实事务提交 receipt 匹配 | 同一对象转为正式用户消息；移除待提交标志，不叠加“发送成功”提示 | 保持稳定本地 key/MessageCopy/位置，补充 durable message ID/seq；重复 receipt 幂等；运行启动与否另行表达 |
| CommittedStartFailed / Stopped | 已提交后启动失败/主动停止 | 真实历史保留；沿用运行失败/恢复或停止语义，不能标成未提交 | 若提供重试，必须复用已提交用户身份并验证其 thread/内容/附件一致，不再插用户消息；重启运行是新 attempt 而非重发文字 |

正常窗口内切线程保留提交快照及其 owner；视图缓存被移除不能静默释放未提交内容，可由控制器持有并在重开同线程时恢复。不是新增崩溃恢复持久化 outbox：进程退出后未落库内容的恢复不在本卡承诺；普通关闭/切换不得假装 durable，现有未保存内容保护不得削弱。

### 3.3 草稿、重试与持久化边界

- **不能比较文本相等清草稿。** 清空只发生于原子捕获时仍归属该 revision 的 Native 普通提交；确认/失败/取消不再清当前输入。附件按提交 ID/附件 ID 精确转移所有权，保留引用/字节至终态；纯图片不能生成一条无意义空文字气泡。
- pending 期间继续编辑，但尚未转运行的同一提交不允许再次普通启动；后续真正进入 running 后仍按既有队列能力处理新草稿，不把本卡 guard 扩大成全程禁发。
- `MessageStarted` 保持既有运行事件契约，不前移以假装 durable，也不代替 primary 首次驱动事实。普通 receipt（含 user ID/seq、assistant ID/seq、submission/attempt 及 accepted payload 归属）须在 `pipeline.rs:1130` 实际 commit 成功后立即单调写入共享 handle，早于 `:1131–1138` title launch、`PreparedRun` 返回和后续 skill/actor 准备；UI 通知可迟到，事实不能迟记。失败/丢失/锁毒按真实已提交或 Unknown 核实，不伪报 NotCommitted；title 与 primary provider 计数分域。
- 普通提交的 user ID 必须在尝试前稳定分配并传入 conversation 写入链；重试不能每次生成无关联的新用户 ID。复用现有主键/事务及已存在用户校验能力，不默认新增 schema/outbox；“相同 ID 已存在”须核对 thread、角色、payload/附件，不直接当成功。具体接线经主 Agent 审核后才冻结，不能临时复用 `pending_message_id` 冒充 #535 队列语义。
- 不确定提交结果是恢复状态，不是可随意重试的普通错误。已提交的用户消息在取消与启动失败后仍属真实历史；失败提示不得吞掉权限请求或原有恢复入口。

### 3.4 主 Agent 续接审查：普通提交身份不能借用旧分支

续接工作树已 fetch 并快进到 `2ba904f7074115de5ee89cc18c376c083f803357`；相对上文 `871f91f246e90753743a256cee1eec315e74effc` 只新增 #554 文档，生产不变。以下决定收敛已有 A1–A12，不新增队列/outbox/崩溃恢复范围，测试仍未编写或冻结。

- 当前 `agent/entry.rs:637–642` 的 ID 选择依次是 credential continuation、persisted instruction、pending queue、最后新建 ID。普通提交必须有独立的可选 typed owner，不挤入 `pending_message_id` 或 approved-plan `persisted_user_message_id`；无普通 owner 的旧调用保持原路由。
- `pipeline.rs:730–765` 的 `uses_existing_user` 实际要求计划批准固定正文、Execute、有效 approval 及未消费的相邻 seq；它**不是任意用户消息的通用重试接口**。普通重试需单独的 conversation 层校验入口，原计划/委派/凭据检查逐字保全，不能放宽该分支来复用普通消息。
- 普通逻辑提交在 UI 捕获时稳定分配 user ID；attempt 在真正开始每次准备时更新。真实事务回执至少关联 user ID、该次 assistant ID/seq 和 attempt。重复回执仅归并本地状态；重复 prepare 先查对应身份及真实事务终态，匹配 thread/role/payload/图片后方可恢复，不能只因 ID 存在就接受。图片比对在 conversation/store 边界执行，不为此将正文或图片放进日志。
- 初次事务仍原子写 user、图片与 assistant 占位；当前 `pipeline.rs:1130` commit 后先单调记录普通 typed durable receipt，再允许 `:1131–1138` title launch，最后才返回 `PreparedRun`。不能等 entry 收到返回才记录。`MessageStarted` 不前移、不替代 receipt；primary Started 须在首次驱动 future 前写入。skill/actor 后续失败必须携已提交归属，不能再走未提交重发。
- 未提交失败可原位重试旧快照；已提交但未启动时，恢复先核实持久状态。只有原 assistant 为匹配的启动前失败、没有后续模型/工具副作用且该线程没有新运行/新历史时，才可用同一 user 的显式恢复接口启动新 attempt，不能再插 user/图片。不能证明上述条件时提供核实/现有运行恢复动作并拒绝盲重发，不把拒绝伪装成“重试成功”。该恢复校验及相反样例必须由 T4/T5 对应的风险验证覆盖，不要求先行 RED。
- 取消不回滚已提交历史；提交结果未知时核实先于重投。单一普通 owner 在控制器中保持到已提交或可恢复状态被明确处理，流视图释放不能丢未提交快照。保存位置与条件释放须按候选 v2 的 app-global claim、worker quiescence 和密封事实闭合；窗口释放不是证明，不扩展磁盘 outbox。

本审查确认需要独立普通 receipt/recovery 契约，但未声称其 API 已存在或安全性已验证。#535 实际最终接口未取得时仍不得抢写共享符号；父先关闭候选 v2 的 P1 接口/集成缺口，再决定实现授权。后续按风险覆盖 T1–T6、身份冲突、已消费用户、图片不匹配及后续历史等必要对照，保留实现独审与云端验证，不以测试先行或有效 RED 为前置。

### 3.5 v2 独立审查修订：恢复资格、载荷与历史

审查绑定 v2 文件 SHA256 `af78eab2fd74691b9c909ead59531cf56211830fcea374a07d3be96446b210c7`；主 Agent 已定点复读原始证据。下列是接口闭合所需的必要澄清，不是已实现机制或测试先行门禁。当前树已保留 #547 合并基线 `9b901a34ee00611f73fbd58275c7087282af5e2c`，图片预算/首次输入取消逻辑不回退。

1. **空 failed 不能证明主运行未启动。** `finish_prepared_failure` 只写空 failed 且调用方可能未成功清理；运行后错误也写 failed，`recover_thread` 还可能将 streaming 改成 interrupted。恢复资格必须依赖存活普通 owner 中的 attempt→assistant 准备/主运行启动事实，而非从空正文、无工具或 failed 反推。丢失证明、结果未知或清理失败时保持已提交/待核实，禁盲恢复；不暗加跨进程恢复承诺。
2. **资格核实与占用不能有并发窗口。** 同一普通 owner 下先取得互斥恢复 attempt，再在 conversation/store 原子边界核对 thread、user/图片、旧 assistant、尾序及无后续历史，并登记新 attempt 对应 assistant；重复/迟到恢复请求只观察原结果，不重复插入。检查后允许别的写入再无条件启动不合格。具体 owner 类型、事务接线与取消返回的 typed 终态仍须先冻结，不能以纯 mutex 或一次查询冒充完整事务不变量。
3. **冻结三个不同载荷域。** UI 原始提交快照用于展示/草稿恢复；一次被接受的文件引用展开载荷用于该提交持久化/主模型请求；`safe_live_text` 是展示投影，不作为身份比对。`app_agent.rs:1154–1179` 会读取并展开引用；已提交恢复必须复用首次接受的展开载荷，不重新读取可能已改变的文件。原文不同但脱敏展示相同不得合并；图片验证数量、顺序及实际字节，不重复 insert。不将任一载荷/凭据放进诊断日志。
4. **副作用范围明确为主运行。** commit 后、`PreparedRun` 返回前可能启动自动标题辅助请求，其 token 与主运行分开。“主运行尚未启动”不表示所有模型请求为零。保留自动标题唯一归属与既有计费，不重复触发 title；恢复准入检查的是主运行/工具副作用，不靠标题完成与否推断。typed 结果分别表达提交、主运行启动、取消/未知；receipt 投递失败或准备 join 丢失都不能改写成未提交。
5. **旧助手占位不静默删除。** 启动前失败的旧 assistant 保留准确失败终态和原 seq；若最终冻结方案允许显式恢复，新 attempt 使用新 assistant ID/后续 seq，但不新增 user/图片。测试同时核对旧/新 assistant、刷新 UI 历史和实际请求历史，不能只断 user 数量或在 fixture 中隐藏旧失败项。是否需排除启动前空失败占位进入后续模型历史，必须先按现有 history/context 契约冻结；当前无该产品改动授权，不在实现时顺带过滤。

T4/T5 对应风险验证还须区分：启动前失败、主运行已启动但空输出、清理失败、准备/receipt 丢失、重复并发恢复、引用文件变化、脱敏展示碰撞、图片不匹配、后续历史变更，以及旧/新 assistant 完整顺序。原 A1–A12 不削弱。上述具体 owner/事务/历史决策尚未齐备，未决见候选 v2/调用点图；不得只实现即时气泡 happy path 而遗漏这些边界。验证顺序按当前政策，不强制先写测试。

## 4. 下一阶段实现动作（当前不执行）

| 顺序 | 具体文件 / 符号 | 拟动作与边界 |
|---|---|---|
| I1 | `conversation_stream/composer.rs::{submit_message,accept_composer_submission,reject_composer_submission}`、`core.rs::ConversationStream`、`mod.rs::ComposerSubmitted` | 捕获每次普通提交 revision/快照与身份；新增普通 pending 展示入口；拆开本地展示、durable 确认、typed 拒绝；保留非普通分支行为，不能提前调用旧 accept |
| I2 | `model.rs::{StreamEntry,StreamEntryIdentity,MessageCopy}`、`render.rs` / `render_rows.rs` | 用真实列表行承载 pending 状态与最少操作；保持气泡样式/复制/图片展示；确认只更新状态/归属，保持本地 key 与 MessageCopy；键盘可达、名称清楚，错误不藏 hover |
| I3 | `window/agent.rs::{submit_composer,finish_agent_preflight,start_agent_run_with_reasoning,stop_composer}`、`window/session.rs::materialize_draft` | 普通分支同步建立 pending，随后异步 preflight/启动；把 draft 落库、open_thread、workspace 路径解析/目录创建移至 worker/conversation 边界；UI 只捕获不可变请求与必要 owner，返回后重验 lease、thread/model、权限/route 与取消 |
| I4 | `app_agent.rs::{UserSubmission,ActiveAgentRun,AgentController::accept_durable_start}`（impl 名按现有定义）、`window/artifact.rs::apply_agent_batch_ingress` | 普通提交 key/attempt 跟随全链；保持旧 generation/thread/stream 校验；真实 receipt 幂等归并；不改 queue_delivery / steering / continuation 决策；迟到结果不能影响新 owner |
| I5 | `vega_conversation::types`、`agent/entry.rs`、`agent/pipeline.rs::prepare_run_with_images_and_reasoning` | 携稳定用户身份与真实提交结果，覆盖提交前失败、提交后 actor 失败、通道错误、重试核实；保持事务、权限、图片写入、provider-before-durable 禁止顺序；不让 UI 直读 SQLite |
| I6 | 同链错误与取消出口、`core.rs` 列表/滚动接口 | 所有早退释放对应 busy/lease，保留快照和准确恢复动作；row identity 转正式与 #545 缓存 revision 接口协调；不改变尾随、运行中队列或 Codex 支持范围 |

“移出 UI”不能仅把函数声明改 async 后仍在 UI poll 执行同步工作。云端应观测实际 I/O 边界所在执行上下文与 UI 可继续 draw，保留原权限/路径围栏和取消后的 owner 复核；不能先做好永久权限判断再无条件执行。

## 5. 真实 API、可行观测与接线缺口

### 5.1 已核实的测试基础

- workspace 实际使用 `gpui-kit = 0.6.1`，解析到 `gpui-pre = 0.3.4`；旧设计文档提及的 0.6.0 不作当前 API 依据。`gpui-kit/Cargo.toml:56–60` 的 `test-support` 传递到 gpui、platform、base、component；facade 导出 `#[gpui_kit::test]`。
- registry 只读核实：`gpui-pre/src/app/test_context.rs` 的 `TestAppContext::update:207`、`add_window:220`、`add_window_view:288`、`VisualTestContext::from_window:756`、`simulate_keystrokes:794`、`simulate_click:850`、`debug_bounds:888`、`draw:893`、`simulate_event:925`；真实 `Window::painted_quads:2618` 在 test-support 下可读。`draw` 执行 element layout/prepaint/paint，不等于 OS present。
- GPUI `ListState::{logical_scroll_top,bounds_for_item,viewport_bounds,is_following_tail}` 以及生产 `scroll_anchor_snapshot` 可读实际列表状态；MessageCopy/生产选择路径可读复制文本，测试平台可读 clipboard。quads 只有形状，不证明文字内容；需同时断言真实行内容/状态、bounds、clip intersection。quad 坐标为 scaled pixels，bounds 为逻辑像素，按 `window.scale_factor()` 换算，不做截图推断焦点。
- 当前 `vega_ui` / `vega` manifest 没有测试接线；静态清点 workspace `crates` 与 `xtask` 未发现 Rust 测试属性，不能引用历史测试名当作可运行回归。如风险验证需要新开发期测试，只建必要接线，不整目录恢复旧集合；合并前按当前政策留痕并清理测试及独占支持。

### 5.2 后续按风险选择的开发期支持（当前不添加）

1. 如需开发期 UI 测试，在有测试的 `vega_ui`、`vega` manifest 增加 dev-only `gpui_kit = { workspace = true, features = ["test-support"] }`，锁文件只处理必要测试依赖，不升级生产依赖；conversation 存储夹具只用已批准的 tempfile/现有依赖。是否需要额外依赖由父审批准，不自行引入 mock 框架。
2. crate 内 `cfg(test)` 模块能调用 pub(crate) 生产路径；初始化真实主题、组件、键位和必要 globals，创建合成 Thread、真实 ConversationStream，必要时真实 VegaWindow/controller。不启动桌面 app、不运行外部模型/进程、不接真实凭据。
3. 对 provider preflight、worker 启动、存储 fault 边界增加最小 dev-only latch/fault seam；它们只能延迟/故障外部边界，不能替代路由、owner/权限判断、UI 插入与事务决定。测试要调用生产 controller + event ingress，不是手写一个 fake controller 然后在里面调用 accept。
4. 原始 ID 不自动是 debug selector。新增可删除的 test-only selector/只读 paint observer，把 pending/user/image 行、按钮与真实 row bounds 关联；不得靠源码 regex 判“已显示”。同一 UI 实体的录制计数必须 per-instance，避免 Nextest 默认并发污染。
5. 先以**已有正式历史用户行**作阳性对照：真实视图能找到内容与非零 clipped bounds；在相同 harness 中操作发送入口能收到真实 `ComposerSubmitted`，异步 latch 确实没有释放。阳性对照失败是接线故障，不证明本卡产品缺陷。

可行性限制：静态 API 证明测试平台有进程内调度、布局/绘制与列表状态的观察口，但尚未证明仓库 globals/renderer 接线可云端启动。真实窗口焦点、IME、OS 屏幕呈现、外部 provider/文件导入与实际阻塞时长只能用户手测；无 renderer 的 `capture_screenshot` 不能作为必要断言。当前没有“普通提交 durable receipt”及 request/attempt API，这是目标产品缺口；静态接口缺失或编译接线失败不代表行为验证通过，未运行断言明确标未运行。若阳性对照无法工作，先修接线/取得 Actions 原始日志，不降验收到 toy helper。

## 6. 必要风险验证组（未来云端，不强制 TDD）

以下 T1–T6 保留全部必要行为与失败边界，按影响和风险选择验证接线，不以仅 T1 的 happy path 代替验收。按[当前政策](../vega-rust-test-delivery-lifecycle.md)可先实现后补必要测试，不强制一次写齐、测试冻结、前置测试独审或有效 RED；实现独审、安全断言及必要回归不取消。若写开发测试，使用行为命名，合并前留痕并清理独占支持。

| 组 | 真实被测入口与夹具 | 必要可观察结果 / 不变量 |
|---|---|---|
| T1 延迟下的原生本地显示与 guard | 真实 ConversationStream 的按钮和发送动作；预检/准备回执 latch；普通 Native 与纯图片变体 | 第一可绘制周期存在唯一 pending 内容行、可编辑新草稿；回执未释放；快速重复输入不会多启动。现基线缺气泡是待验证的行为缺口，不以虚构 API 编译失败代证 |
| T2 控制器到持久化确认 | 真实 `submit_composer` → 预检完成 → worker 边界 → `apply_agent_batch_ingress`；生产事务/受控 provider | pending 在任何 I/O 完成前可绘制；I/O 不在 UI 同步执行；真实提交 receipt 才转正式；同 ID 重放仍一份。receipt/owner 接口及其断言未运行时如实记录，不能以静态接口缺失冒充执行了全部断言 |
| T3 revision/附件/稳定位置 | 等待时编辑不同文字及同文新 revision、新附件；确认与重复确认；已有长历史/选区 | 当前输入/附件不变，原快照内容与图片准确；key、MessageCopy、相邻顺序和选区不变；列表只在初始 pending 插入，确认不再 splice |
| T4 失败与存储真实性 | 每独立阶段一个注入：preflight、启动、事务、actor-start-after-commit；实际临时 file-backed Store | 未提交零消息/零 provider 请求；提交后失败仍一条用户+图片；typed 恢复状态正确；busy 释放；重试复用原气泡/快照且最多一份 durable user。storage seam 不绕过真实事务 |
| T5 取消、迟到、切换与核实 | cancel-before-commit / commit-before-cancel 两种受控顺序；通道丢失；A→B→A；新 attempt 与旧事件交错 | 旧事件不能修改 B/新 attempt；取消不谎称撤销已提交；未知结果不重复写；恢复后仍唯一消息，原图片不丢；stop token 与 controller 终态正确 |
| T6 路由与安全回归 | Native running Queue/Send-now/SteeringAccepted/continuation、Codex、自动任务/批准计划/凭据续跑；model/permission gates | 不套普通 pending 逻辑，不双 echo，不抢运行 owner；权限/模型校验仍执行，拒绝不调用 provider；与 #535 冻结契约兼容；使用实际合法事件序列，不以 Proposed→Success 或缺 Started 的伪序列制造结论 |

断言时序：先 draw 真实视图建立输入命中；发真实操作；保留外部 latch；只推进由该操作安排的下一次有效 draw，检查 pending；随后 release latch、按生产事件收敛。不能为了得到气泡随意反复 refresh，不能用 sleep 等自然网络；UI 阻塞测试用 bounded observer/watchdog 防止主线程自锁，I/O context 与 UI 响应两项都检查。

云端仅复用 `.github/workflows/pr-check.yml`：`cargo fmt --all -- --check`；`cargo clippy --workspace --all-targets -- -D warnings`；三组既有 Python 检查；`cargo nextest list --workspace --message-format json`；开发非空集执行 `cargo nextest run --workspace`。macos-latest，required check 为 `check (fmt, clippy, test)`。这些是**未来 Actions 命令，本轮未执行**，不新增 workflow/强制 E2E 阶段。

按风险记录实际基线、实现独审、必要验证及云端 run/job；使用开发 Rust 测试时固定可恢复源码/patch、完整 SHA、原始结果与计数，合并前清理测试及独占支持。最终无 Rust 测试的 head 仍须完整 required CI，空集记 0 发现/0 执行、`no Rust tests`，不是行为测试通过。不要求 RED/GREEN 时序或冻结后禁止补测；不得按实现反推预期、放宽断言、ignore 或盲目重试换绿。产品、接线、验收变化及外部环境失败分层诊断；验收变化先更新 spec。本轮不触发 CI 或改 workflow。

## 7. 共享写入冲突、顺序与未决项

- **#535 活跃，不领取、不写其 worktree/源码。** 既有[历史评论](https://github.com/puzige/vega/issues/535#issuecomment-6345089275)不是本轮最新接口证据；父检查点确认独立工作仍活跃，实际最终签名/集成点未取得。其范围是 `pending_messages.rs` echo/delivery、SteeringAccepted、continuation 及对应 conversation 持久化。普通发送使用独立 submission owner，不能借 `pending_id` 混入队列。
- 符号接口隔离：本卡主改普通 `submit_composer` 分支、普通 accept/reject 和 `UserSubmission`；`enqueue_composer`、queue_delivery、pending echo reconciling、SteeringAccepted 消费归 #535。`app_agent.rs`、`window/artifact.rs`、`content.rs`/`model.rs`/`core.rs` 为共享文件，父明确一位写入者和逐符号接入顺序；不因文件不同分支就并行改同一接口。
- **当前接入前提**：父审候选 v2 的 claim/receipt/恢复围栏及调用点图，协调 #535 实际最终接口与共享符号顺序后决定下一实现授权。后续最小实现、必要风险验证与实现独审按当前政策，不强制先行测试/有效 RED。若 #535 先合并，保留其生产增量并针对实际最终接口验证兼容，不用旧 ref 覆盖文件或恢复历史测试集合。
- #545 会消费稳定 entry key/revision；本卡确认必须保持 key 并使 display/durable-owner revision 正确失效。两卡 `core/model/render` 的代码阶段串行集成，先确认该接口再组织 #545 验证；当前仅各自 docs，不混写。
- #549 的 Motion/lifecycle 归另一层，本卡无需新增动画/token，不复制公共机制。

| 未决事项 | 已有证据 / 需要的下一步 | 是否降低验收 |
|---|---|---|
| receipt、用户 ID 和已提交重试的最小接线 | 当前 `UserSubmission` 只有 payload，entry 在后台生成 ID，事务早于 actor；父审核候选 v2 的跨层 typed receipt、sealed proof 与恢复围栏，按 T4/T5 风险覆盖 | 否；解决前不得先实现 happy path |
| globals/云端进程内 UI 与 I/O seam | registry 有真实 API，仓库无现成测试接线；下一阶段以 T1 阳性对照和同轮 Actions 证明 | 否；接线故障独立诊断 |
| #535 共享写入和新基线 | 他卡仍活跃；需要父的符号级接口冻结/集成通知 | 否；当前规格可审，代码不得抢写 |
| 未提交内容跨正常切换/视图释放的 owner 寿命 | 候选 v2 的存活 owner/claim 保存位置与条件释放仍须闭合；要求保留内容，不新增磁盘 outbox | 否；T5 不能只测两个始终存活的 view |
| 真实 UI 延迟/安装版本 | 尚未观测；合并后用户核对版本，慢准备/长会话/图片/IME/切线程手测并分阶段记录 | 否；不以云端进程内 draw 宣称真实流畅度 |

## 8. 错误、安全、回滚与阶段交付

保留权限模式、危险确认、skill/model reasoning 身份、路径围栏、持久化先于 provider、副作用取消及凭据脱敏。pending 正文/预览沿用 `safe_live_text`；日志不写 payload/图片/凭据，不为了观测永久存正文。测试仅合成数据与 owned tempfile，不能使用用户工作区/真实配置。

失败提示必须说明失败对象及下一动作；待提交/取消/已提交未启动不混用“发送失败”。重试/取消入口支持键盘，不仅颜色/hover。错误与待处理事项持续可获取，普通确认不叠成功提示；不恢复 #174 冗余常驻准备栏，不重做 #427 之外的全局错误产品。

回滚由主 Agent 在云端验证后按本卡生产提交回退；先固定测试证据。优先无 schema 方案，不删历史消息/图片、不回滚或覆盖 #535/#529。已落库数据继续由原 conversation history 读取，运行停止仍用 CancellationToken。若发现需要 schema/新恢复产品决策，先报告与修订规格，不能在实现中暗增范围。

本轮交付含主规格最小修正、候选 v2 及调用点图；v1 字节不变。未测试、未 CI、未运行/编译/打包/bench/profile/CUA，无源码/Git/外部操作。静态核对、文件 SHA256 和无 Git 的文档 diff SHA256 随 READY-doc 返回；只表示文档可审，不表示 P1/P2 已闭合或授权生产。父拥有独审/集成/下一决定；本轮 READY-doc 后停止全部写入。

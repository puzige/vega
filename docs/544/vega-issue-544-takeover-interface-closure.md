# #544 接手后的接口闭合记录

## 1. 身份与版本

- 用户先确认选择 #544，再明确原执行者已停止；认领标识 `vega-544-takeover-20261025T020430Z`，[认领及范围](https://github.com/puzige/vega/issues/544#issuecomment-6392255832)已经发布并读回。
- 复用 `~/Workspace/worktrees/vega-544-send-echo`、`feat/544-send-echo`。旧工作树干净，没有未提交生产/测试修改。
- 旧产物提交 `f757d0ebf502e1e479504c00817d429c7d1e2f7e` 保留；v1 SHA-256 仍为 `6c6eaadbbd46d8579ffa29528d183b100c8003fba331fb1012fb336648e9a2cd`。
- 已 fetch 并正常 merge master `420a0eb9a06594173b658f7e3a4437e3d2b5432d`，集成 HEAD `4caa21cdfc82287323a09ea033e2978d5d2e09d1`。相对 master 只有原五份 `docs/544/` 文档，生产一致。
- #535 PR #603 已合并；保留 `RunContinued`、predecessor 终态隔离及真实续接计时/间距。等待 #535 已不是阻碍。本卡不将普通提交身份塞入 pending queue ID。

本文保存本轮已经独立复读的事实和有界决策，不把历史分析或调查中的建议写成已实现机制。主规格 A1–A12 是验收基准，v1/v2 和旧检查点保留为历史证据；实现许可须以具体闭合方案为准，不能仅凭本记录启动 happy path。

## 2. 本轮复核的事实

行号对应上述集成版本，实施时按符号定位：

| 来源 | 事实 | 对设计的约束 |
|---|---|---|
| `conversation_stream/composer.rs::submit_message`（269）、`accept_composer_submission`（305） | 前者只冻结附件、置 guard 并发事件；后者收到 durable start 后才追加 User/UserImages，且按文字相等清草稿 | 本地展示与持久化确认必须拆开；不得提前调用旧 accept；普通提交按 revision 转移一次，确认不清新草稿 |
| `window/agent.rs::submit_composer`（1141） | automation、running queue、Codex 独立分派，Native 普通发送才进入 preflight | 先做无 I/O 路由判断，再建立普通提交；非普通沿原路由，不误造本卡 pending |
| 同文件 `finish_agent_preflight`（1713）、`start_agent_run_with_reasoning`（680） | release 预检 lease 后同步 materialize；再同步 open_thread/workspace，之后才 begin/spawn | 本卡普通路径需把物化/DB/工作目录读取移到后台；preflight→active 不能出现失去排他权或 self-busy 误拒绝 |
| `app_agent.rs::run_agent_worker_with_mcp`（913、1184–1306） | 原文经文件引用展开后才传 conversation；配置、凭据、MCP 均在后台，启动前检查 cancel | 原始快照、accepted 展开正文和安全展示投影必须分域；已提交恢复不得重新读取改变的引用文件 |
| `agent/entry.rs`（668–713） | 目前按凭据/批准计划/pending queue/新 ID 选择 user ID，再 spawn_blocking prepare | 普通身份通过独立可选 handle 传递；不放宽原分支 |
| `agent/pipeline.rs`（1130–1149） | transaction commit 后可能 launch 自动标题，随后返回 PreparedRun | commit receipt 必须在 commit 成功之后、title 之前写共享事实；title 是独立 provider 副作用，不等于 primary Started |
| `agent/entry.rs`（768、867、897、915） | skill/actor/事件投递失败出口均有清理 Result 被忽略的路径 | 普通 handle 必须记录实际 cleanup Result；失败或未知不给“未启动恢复”资格 |
| 同文件（986–1045） | runtime future 在 join/supervisor 驱动时才开始执行 | 在首次驱动前成功写 Started；创建 future、Started UI 事件和空输出都不能反推主运行是否开始 |
| `agent/pipeline.rs::finish_prepared_failure`（2076） | 写 failed 且验证更新了对应 assistant，join/数据库失败均可能返回错误 | 不把调用了清理当清理成功；旧 failed assistant 不删除或过滤 |
| `window/mod.rs::window_terminal_cleanup`、`AutomationGlobal::close_window` | 取消并删除本窗口 active/weak owner，不能证明 worker 已停止 | 快照和执行权不能只挂 weak window；worker drain 后按精确 token 释放，不因视图销毁开放重启 |
| `conversation_stream/core.rs::durable_entry_identity`、`set_entry_identity` | key 和 durable metadata 同结构；替换整个 identity 会 retire 高度/text cache | pending 首次就用稳定 user ID 构造 row key；确认只补 seq/归属，不换 key、MessageCopy 或行位置 |
| `conversation_stream/content.rs::hydrate_history_entries` | 历史逐条创建 User/图片和新的 MessageCopy | 同一存活视图的确认/分页必须按 user ID 合并；新视图可以重建视觉对象，但不能重复原 pending 或丢失快照 |
| `vega_store::write_coordination` | dev/inode 识别数据库别名，permit 是连接/OS线程绑定的短写入权 | 可复用数据库身份原则，不能跨 async 持此 permit 或称其为启动/恢复围栏 |

## 3. 需要冻结的最小接缝

### C1：进程内执行权和快照寿命

采用 thread + 稳定数据库身份的窄准入，不引入队列调度器或跨进程产品。数据库身份在 Store 打开时获得，UI 点击时只读缓存身份，不为加锁做 metadata/canonicalize I/O。进程内记录应强持不可变原文、图片对象和共享 attempt facts；UI 只是观察者，不是 worker 完成证明。

单一 owner 从普通 preflight 开始，经过 materialize/skill-pin、active generation、准备和主运行，直到真实 worker 收敛。不同窗口同 thread 的第二 owner 必须被同步拒绝；相同 submission 的重复事件只观察原记录，不将首份提交标失败。checked upgrade 只绕过本 owner 的 busy，不绕过模型/权限/路径/其他 owner 门禁。

取消请求只撤销后续发布/驱动能力。视图切换保留记录；窗口关闭取消但不释放尚可提交/驱动的 worker。旧 owner 的 compare-and-release 不能释放新 generation。是否实现此窄准入还需与 C3 的所有参与点一起审定；不能只加一个全局 map 就声称闭合。

### C2：可选 ordinary handle，不改非普通产品路由

通过 `PersistenceActorConfig` 的可选普通句柄将同一 submission/attempt、user ID、accepted payload、commit receipt 和主运行事实送入 prepare/entry；没有句柄时保留旧行为。初次事务仍原子写 user/images/assistant，commit 后立即记录 receipt，再启动唯一自动标题。

事实至少分开表示 commit（未开始/进行中/已提交/结果未知）、primary（尚未驱动/已开始）、cleanup（不需要/待执行/成功/失败/未知）及登记 worker 是否全部结束。每次更新验证 owner/attempt，单调前进；poison、join 失败或通道丢失均不能生成 NotCommitted/NotStarted 证明。UI 通道只唤醒，状态从共享事实读取。

已提交但主运行未启动的恢复只有在原 owner 密封 NotStarted、cleanup 成功、worker 已收敛后允许；新 attempt 用新 assistant，不再插 user/images、不重复 title。主运行已经 Started 即使零输出也拒绝此类恢复。原 assistant 的 ID、seq、failed 和既有模型历史规则保留。

### C3：事务资格到 launch 的同一围栏

在已提交恢复期间，原 receipt 与 accepted payload/images、原 user/assistant、expected tail/next_seq 必须在 immediate transaction 内精确比较；仅插新 assistant。资格保留、事务提交及后续异步准备到 primary launch admission 始终属于同一进程内 owner，SQLite transaction 不跨网络或 await。

必须与删除、plan-review 持久化、hydrate orphan repair、manual compaction/context 设置及非普通 fresh start 有确定的互斥关系。合法 primary/cleanup/自动标题/#535 steering 不能一概禁用，也不能无条件绕过；接线须区分 owner 内部写入和外部冲突修改。必须明确 metadata/title 与权限/model/skill 修改的不同分类。此处仍等有界调查的精确参与点，不以“再查一次”冒充并发隔离。

### C5：展示与恢复操作

- 普通路由接受后、任何 preflight/I/O/worker 发布前，将原始安全展示正文和图片加入真实列表，立即 notify；纯图片不造空文字行。
- 原 revision 的草稿/附件只转移一次；后续 confirm/failure/cancel 一律不从 Composer 取内容、不清新 revision。相同文字不等于相同草稿。
- pending 与 durable 使用同一 row key；确认不追加 User/UserImages，不换 MessageCopy，不额外滚到底部。
- 明确未提交失败可重试旧快照；提交或启动未知先核实，按钮说明具体不能重投的原因；已提交启动失败按 C2/C3 安全恢复，不伪报“未保存”。错误和恢复入口就近可见、支持键盘；不加常驻实现说明。
- 重开视图先加载真实历史，再按稳定 user ID 合并进程内未决记录；迟到事件只更新对应 thread/submission/attempt。B 的草稿/列表/附件不被 A 的事件改变。

## 4. 验证与当前阶段

本轮已执行 `git diff --check`、Git 差异/状态检查和五份文档 SHA-256；没有本地运行 Vega、编译/check/Clippy、构建、测试、profile 或 CUA。没有本卡生产变更、新 PR 或云端行为结果。

下一步先把 C1/C3 的有界调查结论与以上事实合并为精确到文件/符号的单 writer 实现包，不能留下“由实现者决定”的安全空白。保留 A1–A12；按风险用 Actions 验证真实生产 controller/存储/UI 对象，尤其双窗口、commit/取消竞态、unknown 禁重投、同文新草稿、图片/引用与原位确认。原正式历史行是 UI harness 阳性对照；未执行不得写 PASS。开发 Rust 测试若使用，留源码/patch/完整 SHA 与真实结果，合并前清理独占支持，最终 head 完整 required CI 确认 Rust 0 发现/0 执行。真实 UI 呈现时延、输入法和系统集成交用户手测。

# 数据库级写入 actor（#919）

> 最新整合快照见文末。原基线、18 项阶段证据及“跨模块边界分析与未完成项”保留为历史实施记录，不作当前缺口清单；验收与安全约束不因追加状态而撤销。

## 基线与依赖

基线 `0b3dcd3603a8e97e0ef74ae25a26f17fc3a094e5`，继承 #918。用户反馈“已经使用 下一步”，已登记第一阶段使用反馈。当前本卡 In progress。运行命令共享线程已有开发实现和本地验证，尚未独立审查、CI、合并或安装；不是全卡完成。

## 验收

- 同数据库身份（device/inode，非路径字符串）主/子运行共享一个写入线程与有界 FIFO；不同库独立。明确非运行写入口，不假称全仓库已迁移。
- 暂停锁外准备时，父任务与两个子任务在恢复前提交消息、完整工具终态；继承 #918 权限、输入、来源、恢复、冲突和预算断言。
- 命令提交后回执；同稳定操作身份重复交付不重复写用量或执行工具。结果未知先核对，不自动重放事务体或外部动作。
- 队列有界、公平，发送等待可取消；已入队的必要终态与审计 drain 后再关闭，不因调用者丢弃回执静默丢失。
- 排队后重新核对执行 admission、owner 与权限；不能缓存启动时的授权代替提交端校验。错误无部分安装。
- 故障和退出有真实区分，独立故障侧录不得占 actor 内耗时 I/O。
- 本地行为/安全/并发回归、独立审查与无 Rust 测试最终 head required CI，合并后用户手测。

## 生产入口盘点与实施顺序

1. `agent/persistence.rs` 当前每次运行各自 spawn_blocking 并持有 Store；Snapshot、Event（含用量、工具生命周期、steering）、Diagnostic 三类具名命令。先把其写连接、命令调度、运行状态收敛到同库 actor；保留每运行有界发送端与生命周期注册，不能把任意闭包交给 actor。
2. `agent/pipeline.rs` 的快照/上下文/Skill/凭据准备保留锁外。最终原子安装为独立短事务，不把整个 prepare_run 搬入 actor。
3. 非运行直接入口包括权限/重置、pending message、线程/sidebar、automation、MCP 设置、委派/recovery、compaction；初始化/迁移和独立故障侧录也有专用边界。现有 API 大量接受借用的 Connection/Transaction，部分权限路径先取全局权限 Mutex；不能用同步等待 actor 的任意闭包适配，否则可能产生权限锁反转和 UI 阻塞。逐类界定短提交命令再迁移，未迁移入口必须登记，不能把运行迁移冒称“所有写入已统一”。
4. 不叠加新的全库 permit；actor 仍使用现有短事务协调以兼容上述迁移中的入口与外部进程。最终目标为盘点范围内具名写命令收敛，并明确初始化/迁移等保留例外。

## 安全设计约束

actor 不执行历史投影、文件解析、模型/工具或用户回调。运行本地发送端负责非存储回调和故障侧录；actor 只接收准备完成的数据。保留既有权限→writer 锁序；同步持权限锁等待 actor 是禁止的接线。对包含自带事务的事件，不套 BEGIN 造成嵌套事务；统一原子回执需先明确实际事务边界。

## 证据

本轮运行层实现已通过编译、18 项开发行为回归和受影响模块全目标 Clippy；随后新增满队列取消/跨库隔离测试通过（仍需在最终冻结版本整组复验）。第一阶段结果不替代本轮结果。开发测试按风险添加并留痕，合并前清理；不修改用户数据库、安装、机器配置或 NSH 工作树。

## 跨模块边界分析与未完成项

只读独立盘点确认：权限变更/在线重置、pending/steering、线程/sidebar/project、automation、委派 checkpoint/resume/terminal、compaction/precompute、MCP/Skill 配置、shell facts/recovery、隐私与计划、惰性 reasoning、标题/预测用量、独立 SQL 诊断及 Codex 均有在线写入。不能把这些列作永久旁路。

允许例外只限 actor 启用前初始化/migration、有生命周期隔离的启动恢复、内存准备副本和独立文件故障侧录。相同 helper 的在线调用不因启动例外自动豁免。

三个必须解决的结构点：

1. `vega_store/preparation.rs::PreparedDelegatedInstall::validate` 绑定原连接指针；需要 owned 安装 DTO 携带数据库身份/schema/revision/序号/来源，在 actor 自己的事务中复核，不能删断言直接跨连接。
2. MCP/default permission/draft 的文件锁与 SQL 组合：必须沿用或扩充 durable intent → 锁外文件操作 → actor 确认安装协议；不能把配置解析、保存或凭据删除放入写线程。
3. `LifecycleOwnership::Drop` 无 async 等待上下文；必要终态需要预持有的 drain lease/通道及故障策略，不能用可能满的 try_send 冒充持久释放。

运行层当前仍须处理：事件内部自带事务导致部分事件未与回执原子提交；重复事件元数据与消息/seq 状态恢复；独立故障侧录仍在共享线程；注册的全局容量/关闭与 app 生命周期；跨运行公平动态证明；排队后的权限检查必须与敏感写入原子化；外部忙时可取消有限恢复。现有测试没有证明这些未完成契约。

先保存开发检查点，不创建可合并 PR，不提前清理测试，不降低卡片验收。

事件原子化：共享线程持有短 `WriteTransaction`，保存命令回执与业务变更处于同一事务。存储 helper 提供明确的 `_in_transaction` 入口；不改变全局 `WriteTransaction` 的嵌套拒绝语义。涉及截图控制发布或配置文件权限的事件，需要将非 SQL 预检及提交后副作用留外，不能靠外包整个旧方法掩盖。

## 退出与最后生产者生命周期细化

- 最后一个生产者释放后，同物理库旧线程排空完成前不能另开第二个 actor；不能只靠弱引用存活数代表线程已退出。注册表持有稳定 actor 生命周期，退出时关闭新注册/普通发送，并允许已经预留的必要终态投递。
- 应用退出后台任务先关闭预计算、结算 Shell，排空已接受的 MCP/诊断外部协调任务，再关闭所有数据库 actor；等待已注册运行发送端断开、普通队列及必要租约结算，最后排空独立文件侧录。所有等待在后台，不同步阻塞 UI。超时保留未结算计数并报告，不能声称已持久化。
- 有界后台协调池区分接受前拒绝、接受后失败和未结算；文件侧录同样保留接受计数、失败记录及关闭拒绝。实际 SQL/文件失败不能只记录日志后让排空返回成功。
- 开发回归覆盖工作池容量/关闭拒绝/继续排空、侧录锁等待与数据库隔离、必要终态回执丢失仍提交、旧 actor 最后生产者释放后重开仍为同一实例，以及关闭等待在途运行/必要租约。

## 上游 Shell 并发集成

上游 `2a29adf9b2ea78eaf59d591838531b45adc7373e` 已发布第 63 个 Shell concurrency migration。保留其原编号、顺序和内容；本卡未发布的 receipts/configuration/source migrations 顺延为 0064/0065/0066，所有生产 include 引用同步更新。只操作开发临时库，不迁移或修改用户库。

验收：从已发布 63 版临时库升级后，Shell concurrent 列仍存在且本卡表与来源触发器齐全；空库全链迁移结果相同；两条路径再次 migrate 均无变化。Shell 在线命令继续经过共享 actor，并保留上游 facts 单调合并、终态幂等核对及锁外待提交文件日志语义。

## UI 调用端细化

- 持久任务模式切换及计划审阅在后台等待具名命令；UI 持精确可信动作 lease 并展示已有忙状态。回执只释放本次 lease，不能覆盖新路由或新请求。草稿模式切换保持纯内存。
- 计划审阅保留 first-wins、已提交审批 instruction capability 和提交未知后的权威重载；不自动重试审批。后台等待期间重复提交不另开写入，晚到回执不启动失去路由归属的执行，已提交指令仍可按既有恢复路径处理。
- 本轮不新增说明文案；保留真实失败反馈，完成后解除忙状态，原动作可发现性不变。

## 当前整合快照（非独审／非最终验收）

基线由主会话提供：HEAD `7c3ee7c97db83fbec4ce099313661f9cb7325cb3` + 未提交，MERGE_HEAD `2a29adf9b2ea78eaf59d591838531b45adc7373e`；本轮 fetch 已完成，origin/master 与该上游相同。本次仅核对已接收结果对应的相关源码并整合四份文档，未运行 Git/Cargo、未修改 Rust 或用户库；不扩大为其他迁移审查，也不替代最终全卡独审。

### 已核对的当前接线

- **来源迁移正式为 0066**：`vega_store/src/lib.rs` 注册 0064 receipts／0065 configuration intents／0066 source validation；`context_compaction/source_version.rs::verify_schema` 引用正式 `migrations/0066_source_validation.sql` 并保留精确表/触发器定义校验、物理数据库身份、schema 和 thread revision。旧 `docs/919/0065_source_validation.sql` 仅历史 draft，不再是未注册清单；历史编号/hash 不批量改写。详见 [delegation-background.md 当前整合状态](delegation-background.md#当前整合状态文档整合非全卡独审)。
- **privacy 已接线**：`privacy.rs` 锁外 `prepare_clean`，以 `PermanentlyCleanPrivacy` + `PreparedCleanup` 原子安装；`RedactionCommitToken` 无 policy/Fn，pipeline 安装持 owned token 并保留前后校验，不再携带完整 authority 的回调。store 安装保留来源 fence、revisions、permission、epoch、exceptions、idle 及原值 CAS。不是“删除校验换锁外准备”。
- **pending 提交端 SQL-only**：`pending_messages.rs::verify_commit_fence` 用 `capture_fence_for_commit` 从已注册 workspace/permission SQL 复核，不探测 workspace 目录；锁外准备仍进行必要文件检查。此描述不包含数据库自身 I/O 或公共身份检查。
- **automation draft owned 协议**：`permission_contracts.rs::DraftCreationGuard` 仅持 owned 字段；配置锁／权限锁在入队前释放，`intents::PrepareDraft` 记录 durable intent；`automation/mod.rs::DraftAutomation` 在短事务内以 epoch/generation/source/operation/state 复核 materialize 并创建任务，不把 ConfigEdit 或文件读取带入 actor。
- **后台池及关闭**：预测诊断、MCP reconciliation、submission journal 共用 `submission_diagnostics/background.rs` 的 2 workers、1024 queued-job 有界池，闭包只在 actor 外协调。容量／关闭为接受前显式拒绝；接受后 SQL/actor 失败或 panic 进入 failures，继续 drain 后仍报错，超时报未结算数量，不冒充成功。`vega/src/app_agent.rs` 后台退出依次处理 precompute、Shell、协调池、database actors、sidecar。详见 [auxiliary.md](auxiliary.md#当前整合状态源码核对非独审最终验收)。

以上是指定接口的当前事实，不意味着已重新证明所有旧结构点或全库写入口穷尽；一次性全库迁移的交付范围不变，不允许据历史实现分段单独合并。

### 慢文件系统：已接收风险与契约边界

原验收是**慢锁外准备暂停时，父任务和两个子任务能在恢复前提交消息与完整工具终态**。它不承诺任意慢盘／任意同步 syscall 下同库 actor 都能推进。不能把 metadata 操作数有限称为耗时有界，也不能把该风险条件推成未经用户确认的新验收。

当前 `pipeline/install.rs::InstallPreparation::apply_with_admission` 的两次 `SkillRun::validate_prepared_sources` 和 privacy `RedactionCommitToken::validate` 均在 actor、permission lock、SQL 写事务内同步执行：Skill 链路路径 canonicalize/open/openat/目录与文件 metadata，credential `CredentialStorageVersion::capture` 为 canonicalize + 三对象 symlink_metadata；**不读取或解析文件 body、不取得文件锁，但没有耗时保证**。慢调用会阻塞同库 actor，安装场景还延长 permission lock／SQL 写事务。queue/BEGIN budget 不抢占已开始的 syscall。

`Store::verify_database_identity` 保留 SQLite HAS_MOVED 和连接路径 metadata；公共执行入口在 apply 前，privacy 等事务内调用则仍持锁/事务。数据库自身 SQL I/O、metadata 和物理身份安全不得为了性能删掉；本次不声称 VFS 内部耗时已核实。细节及必要回归见 [preparation.md](preparation.md#来源末检与慢文件系统边界)。

**需主会话确认的文字冲突**：`auxiliary.md` 历史范围段“文件操作……留在 actor 外”以及 `delegation-background.md` 的概括若按一切文件 syscall 理解，比上述原承诺更强，与仍保留的 metadata 安全末检不一致。本次保留原文并显式指出，不擅改验收；actor 禁文件 body 解析、文件锁和任意用户回调的边界不因 metadata 说明而放松。

### 中央验证与尚未完成门禁

用户／主会话本次提供：中央 conversation **78/78**、store **27/27**、app **7/7** 通过（含 privacy/pending/diagnostics）；store **14** 项 lint 已修且编译通过；conversation **9 项生产 + 1 项 test lint** 修复中。本文档整合者未重跑或独立复验，精确工具链、命令、日志、耗时及源码版本对应由主会话维护 `validation.md`，不覆盖该文件。

**Clippy、最终冻结审查、CI 均待完成**。旧阶段 Clippy 通过不替代当前整合版本；中央测试总数也不等于每条历史回归已逐项证明。最终相关回归核对、Rust 开发测试源码/结果留痕及合并前清理、无 Rust 测试最终 head required CI、非实现者独立审查和用户手测仍为独立门禁。当前是已接线且有中央测试报告的整合状态，不是完整验收、可合并、已合并或已发布。

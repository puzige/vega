# 数据库级写入 actor（#919）

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

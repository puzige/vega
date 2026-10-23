# #576 同库写入协调与有界恢复

版本：v1.2，2026-10-23。初始实现基线：`5a1ec8ad26fa53fd6210a8e58b6ad85e739dd8bb`；继续实现前快进至 `6e490aefc18fd0e8a263cbcd6b67570bcbf9348e`，无存储层上游冲突。

## 目标与证据

#570 修正了四处纯读 `BEGIN IMMEDIATE`，但之后只读诊断仍记录到 event_pipeline 的 SQLite 5/5。历史记录没有锁持有者和事件子类，不能把所有失败归因于某个任务。已有每连接 2 秒 busy timeout，不等于没有重试，也不是整个事件的截止时间。

SQLite WAL 同时只有一个 writer。参考 SQLite busy handler / isolation 文档，以及 Zed 合并提交 `e9d2934edfa5560e5efd8253aaff0d9df39a7c70` 的每数据库写队列与 5 秒 busy timeout，采用同库排队、真实写事务尽早取得 writer、短且明确的锁作用域、有界原生等待。既不替换数据库，也不循环重放事件。

## 存储边界与覆盖清单

保留 rusqlite Connection 及 Transaction 的读取接口，在 `vega_store::write_coordination` 提供共享写许可和持有许可的 WriteTransaction：

- 文件数据库以文件设备号 / inode 为键；同一文件的相对路径、绝对路径和符号链接共享队列。内存 / 临时数据库按连接区分。不把路径或文件身份输出到日志。
- Store 初始化的 WAL pragma 和迁移也协调。busy timeout 在 pragma 之前设置。
- Store::immediate_transaction 和 store 内实际写事务统一使用协调事务；仍为 deferred 的纯读快照不改变。
- store 中具有 SQL mutation 的函数在完整函数范围持有许可：messages、assistant responses、tool_calls、token_usage、run_diagnostics、threads、projects、sidebar_organization、pending_messages、context_compaction、credential_privacy、credential_continuation、skills、permissions、subagents、delegated_stages、automation、codex_tasks、mcp_servers、image_attachments、computer_use_delivery、palette（如存在写入）。这覆盖任务 Actor、启动 / 结束、子任务阶段恢复、自动化、侧栏与设置服务。
- conversation 唯一额外直接写事务 ToolRoundCompleted 改用 Store::immediate_transaction。`threads::open_thread` 同样使用协调事务，许可覆盖更新时间和提交，而不只覆盖内部 helper。
- 原始 rusqlite Connection 仍能被其他消费者直接写入；绕开存储 API 的连接、其他进程及 SQLite 自身 WAL 清理不在进程内队列保障内，靠原生等待和真实错误兜底，不宣称全局无锁冲突。

## 协调协议

- 许可由 RAII 释放，按 FIFO 取得；队列最多 128 个等待者，超出立即以 SQLITE_BUSY 返回，不增加无限任务队列。
- 队列等待最多 10 秒；超时移除自身票据并唤醒后继，返回 SQLITE_BUSY。队列等待不占 SQLite writer。
- 同线程、同一 Connection 的嵌套存储函数复用许可。相同线程尝试持有许可后写同库的另一个 Connection，立即返回 SQLITE_LOCKED，避免同步重入死锁。其他线程的连接正常排队。
- 可取消获取每 25 毫秒检查调用者 guard；检查点安装在校验输入后直接使用可取消协调事务，不先经过不可取消外层许可，排队或取得 SQLite writer 后取消均返回 ContextCheckpointError::Cancelled，不提交。不丢弃必须保存的终态 / 用量来让取消更快；其他同步 API 的等待上限仍然有限。
- WriteTransaction 先取得许可再 BEGIN IMMEDIATE，直至 commit / rollback / drop 完成后才释放。读取函数与 WAL 快照不取得许可。嵌套事务仍按 SQLite 原行为报错，不伪造成成功。
- 所有 read-then-write 的 deferred 事务改成协调的 immediate 事务，在读取业务状态前申请 writer，避免 BUSY_SNAPSHOT 升级失败。已有外层事务的叶子写函数只复用许可，不再打开事务。
- 不把许可跨 await、provider、工具执行或外部凭据回调持有。不得等待依赖同库写入的回调。保留已有原子 admission、usage attribution、audit、steering、隐私清理和 source/checkpoint CAS 边界。

## 锁作用域与有限恢复

普通连接和只读连接的原生 busy timeout 保持 2 秒。PersistenceActor 使用 5 秒，在初始化前生效。此值借鉴已有应用且作为有限策略，不宣称对所有机器最优；观察数据再调整。

队列等待上限和 SQLite handler 累积等待上限分别计算；SQLite 可以在有死锁风险时跳过 handler。5 秒不是整个多 SQL 事件的总耗时承诺，也不能修复冻结的外部 writer。SQLITE_BUSY_SNAPSHOT、SQLITE_LOCKED、constraint / permission / corrupt / IO 错误保留原始分类，不进行应用层重放。

实际写事务不包围 provider / 外部凭据读取。准备新回合仍需要在同一事务内验证完整来源、附件、恢复阶段、隐私清理、tool audit 和插入回合，否则会产生 TOCTOU 或孤立 streaming 行；不通过把校验移到 commit 后来缩短事务。实现时可把已冻结输入上的纯 CPU 投影移到 writer 之前，但必须在提交前复核同一来源和 checkpoint，不能降低这些不变量。检查点 CAS 的来源复核、工具终态审计、usage 和归属写入继续原子。

文本 snapshot、用量、诊断等单语句只在保存该结果时持有许可；不会为整个运行持有，也不会将全事件包装为新重试循环。原有有限 Actor channel（64）和每个请求确认继续提供背压。关闭后不再接受新事件，已接受请求正常完成 / 返回真实错误，再释放连接和许可；不 abort SQLite 调用。

## 可观察性

使用现有白名单依赖 tracing，记录枚举操作类别、queue_wait_ms、hold_ms、transaction_ms、完成 / 回滚 / 失败 / 超时 / 取消，以及真实 SQLite primary / extended code。PersistenceActor 补充固定事件类别和保存耗时，关联既有 RuntimeFailureStage。禁止 SQL、数据库路径、文件 inode、线程 / 消息 ID、正文、参数、凭据或原始错误 Display。

SQLite 锁持有者不可知时仍为未知；队列等待和 SQLite 失败不能混称。队列释放日志不等于数据成功提交。写入未确认时不向 UI 假报保存成功。

应用当前未配置 tracing subscriber，不能只添加不可收集的日志。存储层另保留进程内最近 256 条的有界诊断环，按数据库文件身份隔离查询；不写 SQLite、不增加落库锁，不含内容或身份。既有运行诊断 JSON 导出版本升为 2，新增 `recent_process_storage`，明确为当前进程同库近期记录而非该 run 的历史归因；重启会丢失、容量会覆盖，不能冒充持久化证据。记录操作类别、许可等待/持有时间、事务 begin/commit/rollback 结果及真实代码；Actor 的固定事件子类也记录到此环，以枚举限制值域；因此没有 tracing subscriber 时，仍能导出事件子类、保存耗时、结果和错误码。

## 变更记录

- v1.2：确认应用未配置 tracing subscriber，增加有界进程诊断及既有导出入口，不改变存储行为验收。
- v1.1：静态复核补齐 `open_thread` 的事务许可生命周期、检查点排队取消分类和 Actor 事件类别诊断。保留原验收，不以编译或静态结果代替并发行为验证。

## 开发验证与交付

本地只格式 / diff / 静态检查，不运行或编译 Vega，不用 CUA，不使用子 agent，不修改 live DB。Rust 开发测试仅在 draft PR 交现有 Actions 执行。测试覆盖：

1. 多连接同库 FIFO 互斥，另一文件不受阻，同连接嵌套可用；同线程另一连接拒绝，失败 / drop 后许可释放。
2. writer 持有时读取快照仍读到最后提交数据，#570 四个纯读不变。
3. 短阻塞释放后 snapshot、tool terminal/audit、response、usage 正确保存一次且顺序正确，不重跑模型 / 工具。
4. 有限排队与取消、外部真实 writer 超时及后续显式保存恢复；constraint 等非 BUSY 不重放。
5. read-then-write 事务取锁早于状态读取；source / model / checkpoint 失配和取消仍 fail closed，隐私和新 delegated recovery 不变量不变。
6. 原生 timeout 初始化策略与 allowlisted 诊断。

真实外部应用压力及安装版本验收由用户完成。云端验证成功后固定可恢复的完整测试 SHA / 源码 / 实际结果，再移除 Rust 测试及独占接线，最终 head required CI 证实 0 发现 / 0 执行（no Rust tests）。不改 CI，不删失败测试换绿，不把静态检查或空集当功能通过。合并后 #576 保持 In review，用户手测通过才 Done。

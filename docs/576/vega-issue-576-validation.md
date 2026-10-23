# #576 实现与验证记录

## 当前阶段

基于 `6e490aefc18fd0e8a263cbcd6b67570bcbf9348e`，主 Agent 直接实现。遵守本次用户要求：不使用子 Agent / CUA，不修改运行中应用或 live DB，不进行本地编译或执行 Vega 测试。未取得非实现者独立审查，不将自审标作独立审查。

## 实现范围

- `vega_store::write_coordination`：文件 dev/inode 分库 FIFO 许可、128 等待者背压、10 秒排队上限、同连接嵌套、跨连接同步重入拒绝、可取消等待，以及提交/回滚完成后释放许可的事务包装。
- 所有现存 store SQL mutation 入口持有许可；真实写事务改为协调的 BEGIN IMMEDIATE。保留纯读 deferred 快照和 #570 四处读事务。
- `Store::open_with_busy_timeout` 在 pragma 前配置 native timeout；普通与只读 2 秒，PersistenceActor 5 秒。
- ToolRoundCompleted 使用协调事务。无应用层全事件重放，不重新调用模型或工具，不拆分原子 admission、隐私、审计、usage attribution、delegated recovery 和 checkpoint CAS。
- 只记录 allowlisted 类别、等待/持有/事务/事件耗时、结果及数值错误码；进程内最近 256 条有界诊断，现有导出 JSON v2 新增 `recent_process_storage`，按数据库隔离，无路径、正文、SQL、身份。此部分是当前进程同库近期记录，非该运行的历史归因，也不在重启后保留。
- 依赖仅新增现有白名单 tracing 的 store 使用边，无版本升级；不改 schema、CI 或现有 Python 测试。

## 本地静态验证

2026-10-23T06:59:07Z：

- `cargo fmt --all -- --check`：退出码 0。
- `git diff --check`：退出码 0。
- `cargo metadata --no-deps --offline --locked --format-version 1`：退出码 0；只检查清单元数据，不执行构建。
- mutation 入口静态清点：未发现未接入的 store SQL 写函数；conversation 唯一直接写 ToolRoundCompleted 已接入。剩余 unchecked/deferred 事务为纯读。
- `.github/`、`scripts/`、`.config/`、store migrations 与基线无差异。

以上不等于类型、借用或运行验证；没有在本地编译、运行 Vega、Clippy、Rust/Python 测试或打包。

## 开发回归

新增 18 个开发 Rust 测试，尚待既有 Actions 实际运行：

- `crates/vega_store/src/write_coordination/tests.rs`：14 个，真实临时文件 WAL、多连接；FIFO、分库、别名、嵌套、WAL 读取、失败/drop/rollback、取消/超时、队列满、外部短锁/持续锁、BUSY_SNAPSHOT/constraint 分类、checkpoint cancel/source/model/predecessor、privacy source fence、诊断容量及现有导出。队列满仅构造队列状态，不将此单项称作 128 线程压力测试。
- `crates/vega_conversation/src/agent/persistence/storage_tests.rs`：4 个，三个并发 Actor 的文本、工具终态/audit、round/response、用量一条一次；超过普通 2 秒的外部锁恢复；持续锁 5 秒后真实失败及后续显式保存；2 秒/5 秒连接策略和只读失败。
- 不执行真实 provider、工具、外部进程或 UI。原 #570 测试集合不恢复；其四处生产读事务逐字节保持。

待云端成功后固定完整开发 SHA、原始测试源码/patch 和真实发现/执行/通过/失败/跳过计数，再移除测试及独占接线。最终无测试 head 必须通过完整 required CI 并确认 Rust 0 发现/0 执行（no Rust tests），不得用删测掩盖失败。

## 未决与边界

- 云端类型检查、Clippy、测试：待 CI。
- 不承诺冻结的外部 writer 会自行恢复；native timeout 是每 SQLite lock episode 的累计等待，不是整个事件总截止时间。
- 绕开 store API 的原始 SQL、其他进程及 SQLite WAL 清理不受进程内许可保障。
- 仍保留需要完整验证的原子启动事务，不为缩短持锁时间引入 TOCTOU。后续按诊断定位具体长期 holder，不把历史未知 holder 当作已确定。
- 自审不等于独立审查；本次用户要求不使用子 Agent，不能声称取得独立审查。
- 安装/压力/真实系统集成由用户手测；Issue 保持 OPEN，当前 In progress。

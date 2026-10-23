# #576 实现与验证记录

## 当前阶段

基于 `6e490aefc18fd0e8a263cbcd6b67570bcbf9348e`，主 Agent 直接实现。遵守本次用户要求：不使用子 Agent / CUA，不修改运行中应用或 live DB，不进行本地编译或执行 Vega 测试。未取得非实现者独立审查，不将自审标作独立审查。

## 实现范围

- `vega_store::write_coordination`：文件 dev/inode 分库 FIFO 许可、128 等待者背压、10 秒排队上限、同连接嵌套、跨连接同步重入拒绝、可取消等待，以及提交/回滚完成后释放许可的事务包装。
- 所有现存 store SQL mutation 入口持有许可；真实写事务改为协调的 BEGIN IMMEDIATE。保留纯读 deferred 快照和 #570 四处读事务。
- `Store::open_with_busy_timeout` 在 pragma 前配置 native timeout；普通与只读 2 秒，PersistenceActor 初始化 5 秒，成功后替换为安全 busy handler，按单调时钟限制每个等锁 episode 为 5 秒。
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

新增 18 个开发 Rust 测试，第二轮 Actions 已实际执行，17 通过、1 失败：

- `crates/vega_store/src/write_coordination/tests.rs`：14 个，真实临时文件 WAL、多连接；FIFO、分库、别名、嵌套、WAL 读取、失败/drop/rollback、取消/超时、队列满、外部短锁/持续锁、BUSY_SNAPSHOT/constraint 分类、checkpoint cancel/source/model/predecessor、privacy source fence、诊断容量及现有导出。队列满仅构造队列状态，不将此单项称作 128 线程压力测试。
- `crates/vega_conversation/src/agent/persistence/storage_tests.rs`：4 个，三个并发 Actor 的文本、工具终态/audit、round/response、用量一条一次；超过普通 2 秒的外部锁恢复；持续锁 5 秒后真实失败及后续显式保存；2 秒/5 秒连接策略和只读失败。
- 不执行真实 provider、工具、外部进程或 UI。原 #570 测试集合不恢复；其四处生产读事务逐字节保持。

待云端成功后固定完整开发 SHA、原始测试源码/patch 和真实发现/执行/通过/失败/跳过计数，再移除测试及独占接线。最终无测试 head 必须通过完整 required CI 并确认 Rust 0 发现/0 执行（no Rust tests），不得用删测掩盖失败。

## 第一轮云端及修正

- 开发 head `57662d27e65362c5ad006b69f760e724f2ac285d`；draft PR #577；[Actions 39915128775](https://github.com/puzige/vega/actions/runs/39915128775)。Clippy job `120286873590`、Nextest job `120286873513`、required `120288622787` 均失败；已确认 Format 成功、Clippy 执行退出 101。测试发现/执行计数尚未取得，不声称任何用例通过。
- 原始日志包 403，job 日志 503，返回原始 XML `ServerBusy / Egress is over the account limit`，网页 502；注解仅有退出码。没有重跑同 head，也没有依据不完整日志删除测试。
- 静态复核定位明确类型缺口：原 `DiagnosticExport` 派生 PartialEq/Eq，而新 Vec<StorageObservation> 及其枚举未派生对应 trait；补齐相等比较支持，修实现层而非测试断言。后续恢复原始 Clippy 日志，确认 E0369（Vec<StorageObservation> 的 ==）及 E0277（StorageObservation: Eq），与静态诊断一致。首轮是编译失败，不是行为 RED；日志提取故障是另一项外部故障。
- 同期自审发现新开普通读取连接也被无条件初始化写许可阻塞。调整为仅非 WAL 模式变更才协调，连接局部 pragma 不排队；迁移无待执行项直接返回，有待执行项取得许可后复读版本。增强既有读写并行测试，在 writer 持有期间新开普通连接并运行无待执行项 migrate，仍读取已提交快照。
- 以上修正保持全部 18 个测试和原验收；新 head 重新跑完整现有 Actions。当前仍未取得成功结果。

## 第二轮云端与持续锁诊断

- 精确 head `20c31d7166f092546a9d23636a65ebecd7630e4e`；[Actions 39916893692](https://github.com/puzige/vega/actions/runs/39916893692)。Clippy job `120292287485` 成功，含格式和 workspace/all-targets/-D warnings；Nextest job `120292287697` 失败，required `120293515502` 失败。
- Python 实际执行 5 + 15 + 40 = 60 个，全部通过。Rust 发现 18、执行 18、通过 17、失败 1、跳过 0；所有 14 个 store 回归和 3 个 Actor 回归通过。Nextest run ID `8530ccf9-0f69-4163-81b2-38318aa7f22a`。
- 唯一失败 `actor_reports_continuous_lock_and_can_save_after_an_explicit_retry`：8 秒外层期限返回 Elapsed，整个用例 8.193 秒，原行 314。不能从此证据认定排队、SQLite 等锁或回执哪层阻塞，也不能判为测试偶发；没有删除、ignore、放宽时限或重跑相同 head。
- 静态核对 Snapshot 只执行一次 autocommit UPDATE，无应用层重放；已验证连接 timeout 策略的另一用例通过。SQLite bundled 3.53.2 默认 handler 按请求睡眠量计数，不以单调墙钟计时；这仅是候选原因，尚非失败归因。
- v1.3 先补观测：取得许可时记录无内容枚举及排队耗时；失败用例保留 8 秒期限及全部原断言，在失败分支保存截止时记录，释放原始夹具锁、正常关闭 Actor 后输出前后记录，仍报告失败。此轮不改变生产等待或业务验收。
- 诊断轮提交前 fetch 并同步上游 `b032d0d2ab9ee057137f3e5ad2c0e517011ea1ae`，先保存诊断补丁，再 rebase 两个本卡提交，无冲突；上游两项改动不触及本卡存储路径。旧 SHA 的 CI 证据不变，新基线必须重新完整验证。当前 `cargo fmt --all -- --check`、`git diff --check` 均退出 0，无本地构建/运行。

## 第三轮证据与有界等待修复

- 精确 head `d5a92a58714c324baec0b6af3c690b6f514fb64e`；[Actions 39921307834](https://github.com/puzige/vega/actions/runs/39921307834)。Clippy job `120306927287` 成功；Nextest `120306927647`、required `120309215334` 失败。Python 60 通过；Rust 发现/执行 18，通过 17、失败 1、跳过 0，持续锁用例 8.226 秒失败。
- 从原始 job 日志提取：截止 `elapsed_ms=8002`，最近一条为 Messages/PermitAcquired、queue_wait_ms=0，无后续释放/回执；释放夹具外部锁并正常关闭 Actor 后，新增 Messages/PermitReleased 及 Snapshot/Persistence，elapsed_ms=8104、保存成功。故此失败位于已取得许可后的 SQLite 调用，不是进程内排队或调用方回执延迟。不把释放之后的成功算为通过，也不声称证明 OS 调度延迟的具体成因。
- v1.4 先更新 spec，再用 rusqlite 的安全 busy_handler 函数指针安装 Actor 专用回调：thread-local Cell 保存首次 busy 回调的 Instant，按剩余时间最多睡 25ms，睡后复查，超过 5 秒返回 false。每次 count=0 重置 episode；只重试 SQLite 的取锁动作，不重放事件、不 abort 或留后台请求。
- 保留全部 18 个测试及持续锁每次原 8 秒期限、至少 4 秒等待、Snapshot/5/5 分类、未保存、释放后显式恢复断言。该用例连续执行两次受锁保存，新增验证同一 Actor 下一请求拥有新的等待预算，不降低原验收。
- 修复前 fetch，干净分支 rebase 到 `988e1e37271c666f05ff8eae46d662324e91afd6`。新增上游仅 `pending_messages::mark_accepted` 是未覆盖 mutation，补完整函数许可，保留原 WHERE/continuation 和外层事务；新 message page 查询为纯读不改，0034 迁移原样保留。不得把同步成功当作该新基线的功能验证。
- 本轮 `2026-10-23T08:40:56Z`：`cargo fmt --all -- --check`、`git diff --check`、`cargo metadata --no-deps --offline --locked --format-version 1` 均退出 0；测试源码静态 14 + 4 = 18；相对最新 master，`.github/`、`scripts/`、`.config/`、migrations 无差异，无新增生产注释、unwrap/expect 或 unsafe。本地未运行/编译 Vega 或 Rust/Python 测试。

## 未决与边界

- v1.4 修复待完整 Actions；既有失败未消解，不移除任何测试。先前已通过的检查不冒充新 head 已验证。
- 不承诺冻结的外部 writer 会自行恢复；普通 native timeout 累计请求睡眠，Actor handler 限定单 episode 的实际等待，均不承诺整个多 SQL 事件的总截止时间或硬实时调度。
- 绕开 store API 的原始 SQL、其他进程及 SQLite WAL 清理不受进程内许可保障。
- 仍保留需要完整验证的原子启动事务，不为缩短持锁时间引入 TOCTOU。后续按诊断定位具体长期 holder，不把历史未知 holder 当作已确定。
- 自审不等于独立审查；本次用户要求不使用子 Agent，不能声称取得独立审查。
- 安装/压力/真实系统集成由用户手测；Issue 保持 OPEN，当前 In progress。

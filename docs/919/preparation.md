# #919 prepare_run 生产写入迁移检查点

## 状态与范围

**READY：本授权范围停止写入，供主 Agent 统一编译、补齐接口和回归；不是完整验收或可合并声明。**

共享 worktree：`/Users/puzige/Workspace/worktrees/vega-919-database-writer`。
基线 `origin/master`：`0b3dcd3603a8e97e0ef74ae25a26f17fc3a094e5`；本轮 HEAD：`555b0c0b3c00031ad22b69c7a5622e3c1dc4116b`。已检查该 master 是 HEAD 祖先。使用主 Agent 本轮已完成的 fetch；未 fetch/rebase/reset、未改 index、未 commit/push/PR、未运行 Cargo、未访问用户数据库。

仅修改以下文件；`concurrency_tests.rs` 的共享未提交变化归主 Agent，本 agent 未编辑或格式化它，既有暂停点与断言未删：

| 文件 | 职责 |
|---|---|
| `crates/vega_conversation/src/agent/pipeline.rs` | 调用侧锁外准备、具名命令 enqueue、有限 revision 重算、提交后动作 |
| `crates/vega_conversation/src/agent/pipeline/authority.rs` | 精简 owned CommitAuthority 与提交专用 fence 捕获 |
| `crates/vega_conversation/src/agent/pipeline/install.rs`（新增） | InitializePreparation / InstallPreparation 命名 DatabaseCommand |
| `crates/vega_store/src/preparation.rs` | 普通/委派共用内存投影及 owned 安装起始 next_seq 验证 |
| 本文档 | 范围、证据、阻碍与下一步 |

## 已实现迁移（首阶段记录，本轮追加证据见末节）

- 原 recovery、初始 authority 事务走 `InitializePreparation`；原最终原子写入走 `InstallPreparation`。调用端释放 permission coordination 后 enqueue，actor 内重新按 permission → writer 次序取锁；未传事务/权限/config guard，未 actor 自 enqueue。
- 普通与 delegated 均捕获 bounded 内存快照，锁外完成 legacy/payload redaction、安装前历史、最终 ContextSource/history、完整 terminal tool projection（带 mutation origins）、foreign tool IDs 预筛、Skill 文件准备与 explicit preflight。writer 不调用完整历史投影、工具终态恢复或 Skill preflight。
- 最终短事务保留 pending 来源与交付、continuation claim/attach、delegated instruction/parent/execution、approved-plan、普通 recovery、消息/图片、run fence、标题 claim、Skill activation audit 及 ordinary receipt 原子安装。
- actor 在 BEGIN 前后重新验证 admission/reset/owner；BEGIN 后重验完整 fence、automation/pending/approved/interrupted 来源、model context policy、exact rules、Skill generations/预准备文件身份、credential authority。原安全校验保留，权限捕获使用本轮共享新增的 `capture_fence_for_commit` / `capture_delegated_fence_for_commit`。
- `OwnedDelegatedInstall` 继续验证物理数据库身份/schema/revision/origins；新增安装前 next_seq，而非用普通路径新增 user 后的 assistant seq 误作安装前序号。borrowed 接口继续保留原连接指针绑定。
- snapshot 增加 `permission_contract_state`，用于普通 pending 内存模拟。纯内存写仅作用于准备副本，不是生产旁路。普通/委派生产安装统一走命令，无初始化永久旁路。
- revision 冲突最多两次重算，只在任何安装体写入前返回 Conflict；不重放事务体或提交后动作。每次重算从未激活 SkillRun 副本重做预算 preflight，避免 already_loaded 跳过检查/审计。replay lease、凭据 activate、自动标题 launch 在成功安装后执行。
- 首阶段仅累计初始化及重算间 `immediate_transaction_with_budget` 的 FIFO/BEGIN 等待。本轮已增加 actor 队列等待累计，未扩大 10 秒预算；当前接口及证据见末节。

## 未完成的共享接口阻碍（必须补齐，不能作为永久限制接受）

1. **命令队列取消与预算：本轮实现已接线，待中央编译及行为验证**。首阶段的阻塞 send/recv 缺口由末节的 owned admission 状态和 FIFO/Condvar 队列替换；不能用首阶段或旧 26 项结果证明新代码。
2. **凭据无闭包提交令牌**：`privacy.rs::PreparedRedactionAuthority` 私有字段 `policy: CredentialExceptionFilter` 是 `Arc<dyn Fn...>`，目前只有完整 authority 的 `validate`，没有 callback-free 校验 DTO。本实现为了不删 credential 提交检查，暂时将 `Arc<PreparedRedactionAuthority>` 传给 InstallPreparation，仅调用 validate、不调用 policy，但这仍不满足严格的“禁止闭包传输”。
   最小下一步：由持有 privacy 范围的 agent/主 Agent 拆出不含 policy 的 owned commit token，保留 root/database/revision/storage/epoch/exceptions/rule 校验；policy 仅留调用侧 redaction。将 InstallPreparation 字段换成该 token，并验证凭据/exception 更新竞态。不得用锁外 validate 缓存替代提交复核。
3. **编译与风险回归尚未运行**：用户要求 Cargo 由主 Agent 集中执行。本检查点未证明 Rust 类型/借用/Send 编译、Clippy 或行为结果。共享范围仍在并行变动，以上 SHA 仅冻结本范围字节。

## 首阶段静态验证与历史内容标识（非本轮新代码验证）

工具：`rustc 1.98.0 (88d9e12ae 2026-08-18)`；`rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`。仅查询 rustc 版本，没有编译；Cargo 版本本轮未取得。

实际已执行：

- `rustfmt --edition 2024 --config skip_children=true --check`，显式指定上表四个 Rust 文件：退出码 0；不递归格式化其他 agent 文件。
- `git diff --check -- <三个已跟踪授权 Rust 文件>`：退出码 0。
- 静态清点：pipeline 直接 `.immediate_transaction` 0；install 模块完整 history/source 投影、terminal recovery、explicit Skill 加载/preflight 调用 0；新增代码注释、unwrap/expect 0。
- 没有新增测试函数或测试模块。diff 内的一处 `#[cfg(test)]` 是搬迁并保留原 `foreign_ids_boundary` 接线，不是新测试；既有 `concurrency_tests.rs` 不在本 agent 写范围。

四个代码文件按上述顺序，以 `relative-path + NUL + file-bytes + NUL` 拼接 SHA256：

`a901c3964f0d1bf04203205c538a2329a1613da5d82276ae9b31b427170118f9`

| 文件 | SHA256 |
|---|---|
| pipeline.rs | `9d8a1b053b9b700436fcb60c2f5b41c9099eed79f06cf8c7df9357ec7f5a8138` |
| pipeline/authority.rs | `64a986301a990301406e57d531555561c00a43811a8d0e9b062b8690cf1b8b36` |
| pipeline/install.rs | `bf7afd37cd7d50aab5adf14c6b0cdb2da1359d5e2d29913257b0db65445ed27b` |
| store/preparation.rs | `c101bc4b64183e2b28c5c4f84d974c3beafdd625080eed0ee2886d30b613e0ce` |

## 主 Agent 下一步

共享 queue 接口本轮已实现，credential token 待 privacy 产物接入；统一 Cargo 编译/Clippy/任务相关回归仍须执行，不把静态检查当行为通过。必要验证包括普通与 delegated/pending/approved/interrupted/credential continuation/recovery、输入/权限/owner/Skill/凭据变化拒绝、revision 最多两次重算、零部分写入、提交后动作一次性、满队列与 BEGIN 取消及累计 admission budget。

核心 #918 回归仍必须真实执行：暂停锁外准备期间，父任务与两个子任务真实消息及完整工具终态回执先提交，再恢复准备；检查历史、工具回执与顺序不丢失、不重放。保留当前 concurrency_tests 所有断言。测量 writer 短事务耗时，不因数据规模放宽断言或预算。

NOT RUN：Cargo check/build/test/nextest、Clippy、核心并发回归及计时、required CI、非实现者独立审查、用户手测。未合并、未发布，任务完整验收仍未完成。

## 本轮共享 queue cancel / admission budget 检查点

**READY：本轮授权文件已停止写入；仅完成队列实现和静态检查，不是全卡完成。**

用户指定源码 HEAD 仍为 `555b0c0b3c00031ad22b69c7a5622e3c1dc4116b`；本轮遵照限制未执行任何 Git/Cargo 命令，未改 index/commit，也未全局格式化。本轮实际写入范围仅以下五个 Rust 文件与本文档；`pipeline/authority.rs`、`store/preparation.rs` 原 diff 保留且本轮未编辑，两个测试文件未编辑。其他 agent 的工作未覆盖。

### 接口与语义

- 保留 `commit(path, identity, command) -> Result<C::Output, String>` 和 `DatabaseWriter::submit`，普通 service 不必迁移签名；未新增 UI 同步调用。
- 新增 `commit_with_admission(path, identity, command, CommitCancellation, budget) -> Result<C::Output, CommitError>`。取消状态仅持 owned CancellationToken / OrdinarySubmission，不包含回调、SQL 或任何权限/config/事务 guard。
- admission 计时由 FIFO submit 入口 `start_wait()` 启动，扣除队列状态锁、容量等待、ticket 等待及已入队等待，actor `begin()` 冻结余额；不扣 Store 打开、命令构造、锁外准备、permission 取锁或事务体。
- `Waiting / Started / Rejected(error)` 由同一 Mutex 仲裁。入队前取消/零预算返回明确 not-enqueued；入队后尚未开始时，调用者或 actor 将状态置 Rejected，actor 必须跳过 apply。调用者可及时返回，不存在返回取消后该命令仍成功执行的隐瞒。
- actor 一旦将状态置 Started，调用者不再因取消/排队超时提前返回，必须接收实际 apply 结果。事务 BEGIN 等待仍由现有预算/取消检查负责；无盲重试、事务体重放或外部动作重放。回执断开报 OutcomeUnknown，要求先核对；未伪称取消或成功。
- 一般命令队列仍最多 128 个排队 command；提交者按 ticket FIFO，满队列 Condvar wait_timeout（25 ms）且 pop/取消/关闭 notify_all，无 busy-spin。等待提交者仍持自己的 command，和原 bounded channel 一样不计入已入队 128 槽。各 run 及 necessary 既有调度方式保留。
- actor 退出 guard 关闭队列、唤醒提交者，并释放未开始请求的 reply，避免新 Arc 队列使接收方永久等待。正常退出仍依既有 wakeup sender 生命周期与 drain；NecessaryLease 128-slot semaphore、预留单槽 try_send、非阻塞 Drop/wake 路径不变。
- `DatabaseCommand` 新增有默认实现的 `apply_with_admission(store, remaining)`，不要求普通 command 修改 apply；Initialize/Install 覆盖该入口，传入 actor 扣队列后的余额。
- `WriteTransaction::admission_remaining() -> Option<Duration>` 暴露 budget 构造在 BEGIN 成功时冻结的余额；未改变现有 FIFO/BEGIN deadline/retry/取消行为。非 budget 构造返回 None。prepare 初始化返回余额，Install revision conflict 返回进一步扣除后的余额，最多两次锁外重算仍保留。
- physical database verify、权限→writer 次序、提交端 authority/admission/owner/fence/source/Skill/credential 检查均保留；运行旧事件回执与水位恢复代码未动。

### 未决和最小接线

- 本轮最后读取 `privacy.rs:292-303, 360-389` 时仍只有 `PreparedRedactionAuthority`，私有 `policy: CredentialExceptionFilter` 回调尚在，没有 callback-free token。`pipeline/install.rs::InstallPreparation.redaction_authority` 仍为 `Option<Arc<PreparedRedactionAuthority>>`；`pipeline.rs` 仍 clone 完整 authority。这是已知未完成边界，不把禁止闭包传输称为已满足。
- privacy agent 交付 token 后，将 Install 字段换为不含 policy 的 owned commit token；调用侧从原 authority 提取 token，提交的两处 validate 保留。token 必须验证 root/database/revision/storage/epoch/exceptions/rule，而非缓存锁外授权；本轮没有编辑 privacy。
- 尚未证明 Rust 类型/Send、并发或预算时序。普通 API 和 NecessaryLease 的兼容性目前仅静态核对，必须中央验证；未扩超时或删断言取得通过。

### 本轮实际验证

- `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`；查询 `rustc 1.98.0 (88d9e12ae 2026-08-18)`，没有编译；Cargo 版本未取得。
- `rustfmt --edition 2024 --config skip_children=true --check` 显式五个下表 Rust 文件：最终版本退出码 0。定向 rustfmt 不递归子模块，未编辑 tests.rs/concurrency_tests.rs。
- 内联 Python 静态核对退出码 0：五文件无行尾空白/unwrap()/expect()；128 command capacity、NecessaryLease semaphore 128、FIFO ticket/Condvar、退出释放 pending、两个 preparation commit_with_admission 与两个事务余额读取均存在。这是接线清点，不是行为测试或编译通过；脚本未保存到源码。
- **NOT RUN 本轮**：Git diff/check、Cargo check/build/test/nextest、Clippy、CI、独立审查、用户手测。旧 26 项结果是历史证据，未列作本轮结果。

五个 Rust 文件按下表顺序以 `relative-path + NUL + file-bytes + NUL` 拼接的 SHA256：

`973eb28442980fce3f0e602bfd04cf392618d984db17fd47ab4ae65b4d1c31e7`

| 文件（仓库相对路径） | SHA256 |
|---|---|
| crates/vega_conversation/src/database_writer.rs | `7ed115710a4636a6c7a9a0d65490d5d6958c2d901b5b82a6497401b3f6e8cfb4` |
| crates/vega_conversation/src/agent/persistence/database_writer.rs | `61d008d45d2d14111eee58820d644d529a35ce0a88ff0a619a0d278e851aa635` |
| crates/vega_conversation/src/agent/pipeline.rs | `818f4d2494d22725fe19ee0a2ae46934d59d30a2e58ce98d44394abe898d4c26` |
| crates/vega_conversation/src/agent/pipeline/install.rs | `ad3f27eb5083300a409d9afde504ee4ddb798b36c07957a3be0792e43ae9c84c` |
| crates/vega_store/src/write_coordination.rs | `65d74fac6555b2e09b9e36152388ec55fe81f778e54c56661a88daa92195311f` |

本轮未编辑文件的读取标识：authority.rs `64a986301a990301406e57d531555561c00a43811a8d0e9b062b8690cf1b8b36`；store/preparation.rs `c101bc4b64183e2b28c5c4f84d974c3beafdd625080eed0ee2886d30b613e0ce`。

### 中央建议回归（非已执行）

1. 预取消、满 128 槽容量等待取消、已入队但 apply 未开始取消/超时：快速回执且恢复 writer 后绝无业务写入；后续 FIFO ticket 可推进。
2. actor begin 与取消竞争：若 begin 赢必须等真实结果，成功不能报取消；若 rejection 赢 apply 不执行。回执断开区分 unknown，无自动重试。
3. 分别累加初始化队列等待 + FIFO/外部 BEGIN busy + 安装队列等待 + 两次 conflict BEGIN，合计受同一 10 秒约束；长锁外暂停不耗预算。长事务体本身不触发队列超时假回执。
4. 同库 alias 共 actor、异库独立、普通 service commit 兼容、NecessaryLease 128 容量耗尽与 Drop/drain、丢弃回执仍 drain、旧事件回执不退水位及 physical DB 替换拒绝。
5. 继续运行原全部权限/owner/source/Skill/凭据/输入变化回归及零部分写入断言，保留 #918 父+两子真实消息和完整工具终态先提交再恢复锁外准备的完整断言；不以新队列取消替代安全检查。


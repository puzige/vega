# #919 开发检查点（未完成，不可合并）

## 内容与验证身份

基线 `0b3dcd3603a8e97e0ef74ae25a26f17fc3a094e5`。当前检查点包含开发 Rust 测试；不得进 master。任务分支 `feat/919-database-writer`，worktree `~/Workspace/worktrees/vega-919-database-writer`。

实际工具链：`cargo 1.101.0 (e611c7a87 2026-11-04) (huacnlee/cargo 0.102.0-9)`；`rustc 1.98.0 (88d9e12ae 2026-08-18)`。所有 Cargo 命令设置 `CARGO_FORK_AUTO_UPDATE=0`、`--locked --offline -j 2`，未改机器配置或 CI。

## 当前实现

运行 persistence 的具名 Snapshot/Event/Diagnostic 共享数据库身份注册的写线程，各运行保留有界发送端和轮转调度，事件唤醒无定时轮询。关闭先断开发送端并唤醒，再等待 drain；调用方丢弃回执不丢已接收命令。快照/用量带操作 ID，回执同事务保存；其他事件暂有 pending 状态防盲重放，**并未满足完整原子回执与恢复验收**。工具提案/批准/运行增加权限 revision 校验，但尚须与最终写入原子化。自动化 missing_usage 回调已移出共享线程。

## 实际执行

- `cargo check -p vega_conversation --locked --offline -j 2`：通过（阶段性编译）。
- `cargo test -p vega_conversation --lib --locked --offline -j 2 -- --nocapture`：检查点 **19 发现、19 执行、19 通过**，执行 16.19 秒；编译日志 `/tmp/vega-919-checkpoint-tests.log`。这些临时路径非永久证据，原测试源码保留在本检查点提交中。
- `cargo clippy -p vega_conversation -p vega_store --all-targets --locked --offline -j 2 -- -D warnings`：通过，2.07 秒；日志 `/tmp/vega-919-checkpoint-clippy.log`。现有 manifest `unused workspace dependency tracing-subscriber` warning 未修改。
- `cargo fmt --all`、`git diff --check`：通过；未运行最终无测试版 fmt/CI。

19 项含第一阶段本任务相关回归及共享线程新回归：慢准备暂停时父任务及两子任务完整终态先提交；followup/recovery；权限撤销；快照冲突重算；取消与预算；晚到工具 ID；大历史计时；同库别名共享/不同库隔离；回执丢失仍提交及 drain；单运行关闭不影响其他运行；重复用量操作只一行；pending 操作不重放；权限 revision 变化拒绝旧提案；满队列取消不入队，另一库可提交。

## 失败与责任层

- 新权限末检使旧并发夹具失败：夹具仅插消息、无 run fence；补生产权限前提后原隔离断言通过，未删除校验。
- 首版满队列回归超时：消费者已取走一条，队列仍余一格，测试写入已经入队。改为实际确认队列 Full，取消保持 200ms 断言，跨库提交保持 200ms；通过。不是放宽时限。
- 临时接口/编译与 Clippy 错误已修；最终此检查点的上述验证已重跑。

## 未决与下一步

完整清单及三个结构调整见 `database-writer.md`。19 项通过**不证明**全库入口迁移、完整事件原子性、提交未知恢复元数据、全局容量/公平性、独立侧录外移、app shutdown 或权限原子复核完成。

上述为旧检查点的状态。用户随后明确选择一次性全库迁移，任务已继续；主 Agent 与具名互不重叠的范围协作者持续实现，不做 native-only 部分合并。尚未最终实现独立审查、云端 required CI、合并、正式打包、安装或用户第二阶段手测。独立子 Agent 的入口盘点不是实现审查；不关闭 #919，不标 In review/Done。

## 后续集成阶段：后台关闭与准入

- 当前基础 HEAD `555b0c0b3c00031ad22b69c7a5622e3c1dc4116b` 加未提交全库迁移差异；协作者仍在写 Codex、隐私、来源校验与 fork，以下不是最终冻结身份。
- `cargo check -p vega --all-targets --locked --offline -j 2`（`CARGO_FORK_AUTO_UPDATE=0`）：成功；11.67 秒，剩余两处迁移后的 dead-code warning，非最终 Clippy 证据。此前的 Request move 错误已在责任层修复；Codex 私有方法、fork 模块缺失均曾阻断中间态编译，不列作通过。
- `cargo test -p vega_conversation --lib --locked --offline -j 2`：30 发现/30 执行/30 通过，执行18.74秒；日志 `/tmp/vega-919-tests-current.log`。含慢准备时父任务与两个子任务完整终态提交、receipt 原子性/恢复、权限、跨库、后台池容量/失败/排空、同库最后生产者释放后重开仍同 actor、关闭等待运行和必要租约且丢回执仍提交。
- `cargo test -p vega_store persistence_failures::tests --lib --locked --offline -j 2`：2 发现/2 执行/2 通过，执行0.37秒；日志 `/tmp/vega-919-sidecar-current.log`。覆盖文件锁不阻塞数据库、物理库替换拒绝、关闭超时不冒充落盘/后续排空/关闭拒绝。
- 上述之后补充服务准入排队取消、排队预算、开始后取消仍等真实结果，以及必要终态失败不冒充关闭成功。首次专项编译被并行 fork 测试文件尚未创建阻断，尚无这些新增测试的通过证据。全库新版本仍需集中复验和最终门禁。
- 本阶段最初两次中央命令沿用了本机默认选项（未显式 locked/offline/auto-update 环境），未安装或修改机器配置；随后恢复前述固定命令约束。实际 rustc/Cargo 版本与本文件前文相同。

## 上游 Shell 集成阶段（开发树，尚未冻结）

内容身份为 HEAD `7c3ee7c97db83fbec4ce099313661f9cb7325cb3`，正在集成 `2a29adf9b2ea78eaf59d591838531b45adc7373e`，加主线及并行范围的未提交差异。当前三个文本冲突已解决并登记，仍未提交合并；隐私、pending 和手动压缩诊断的后续差异不由以下历史结果证明。

工具链本轮实际为 `cargo 1.101.0 (e611c7a87 2026-11-04) (huacnlee/cargo 0.102.0-9)` / `rustc 1.98.0 (88d9e12ae 2026-08-18)`；Cargo 均使用 `CARGO_FORK_AUTO_UPDATE=0`、`--locked --offline -j 2`。

| 实际命令 | 实际结果 | 本地原始日志 |
|---|---|---|
| `cargo test -p vega_store --lib --locked --offline -j 2` | 20 发现/20 执行/20 通过，0.89 秒 | `/tmp/vega-919-store-integration.log` |
| `cargo test -p vega_conversation --lib --locked --offline -j 2` | 62 发现/62 执行/62 通过，18.68 秒 | `/tmp/vega-919-conversation-integration.log` |
| `cargo test -p vega codex_agent::behavior_tests --locked --offline -j 2` | 5 执行/5 通过，0.00 秒；另 0 匹配的二进制不是测试通过 | `/tmp/vega-919-codex-integration.log` |
| `cargo clippy --workspace --all-targets --locked --offline -j 2 -- -D warnings` | 失败，14 个 store lint，尚未通过；责任层修复中 | `/tmp/vega-919-clippy-integration.log` |
| `python3 scripts/release_tests.py` | 5/5 通过 | `/tmp/vega-919-python-release.log` |
| `python3 scripts/prepare_computer_use_tests.py` | 15/15 通过 | `/tmp/vega-919-python-computer.log` |
| `python3 -m unittest discover -s scripts/tests -v` | 40/40 通过 | `/tmp/vega-919-python-suite.log` |

迁移回归保留已发布的 0063 Shell schema，本卡顺延为 0064 receipts、0065 configuration、0066 source validation。真实 Store 回归从已发布第 63 版升级，与空库全链 schema 逐项相同，Shell concurrent 列、各新表/触发器均存在，重复 migrate 无变化，foreign-key check 为空。

会话回归覆盖 MCP 五项协议、Codex 启动与丢回执不重放、委派来源/权限变化、fork、Shell concurrent facts/cleanup/generation 三项、必要终态/关闭、队列及 BEGIN 组合预算和慢准备父任务/两子任务终态先提交。仍不代表真实模型/外部 ACP 或 UI 手测通过。

失败诊断留痕：迁移夹具最初使用不存在的内存 Store 方法及 usize SQLite 参数，已按实际 API 修正；委派测试最初模块路径和直接 rusqlite 依赖错误，修正接线且未新增依赖；Codex 丢回执夹具在意图创建前取确认时间，真实 SQL 触发器拒绝时间回退，改为创建完成后取时间，保留原始回执与不重放断言。首次会话实际 62 执行、61 通过/1 失败，修正后上述全套 62/62 通过。没有通过放宽断言、删除失败测试或修改生产时间单调规则换绿。

临时 `/tmp` 日志不是远端可恢复证据；后续须将冻结源码及结果摘要提交发布、独立实现审查、开发 Rust 测试留痕清理，并验证最终无测试 head 的完整 required CI，当前均未完成。

## 隐私、pending 与诊断整合复验（仍为开发树）

本轮 HEAD `7c3ee7c97db83fbec4ce099313661f9cb7325cb3`、MERGE_HEAD `2a29adf9b2ea78eaf59d591838531b45adc7373e` 加未提交差异；已 fetch，origin/master 与 MERGE_HEAD 相同。尚非冻结提交。工具链仍为前述 cargo 1.101.0 / huacnlee 0.102.0-9、rustc 1.98.0，Cargo 设置 `CARGO_FORK_AUTO_UPDATE=0`、`--locked --offline -j 2`。

| 实际命令 | 实际执行结果 | 本地日志 |
|---|---|---|
| `cargo test -p vega_conversation --lib --locked --offline -j 2 agent::diagnostics::tests:: -- --nocapture` | 5 执行/5 通过，1.07 秒 | `/tmp/vega-919-diagnostics-current.log` |
| `cargo test -p vega_conversation --lib --locked --offline -j 2` | 78 执行/78 通过，42.40 秒 | `/tmp/vega-919-conversation-revalidation.log` |
| `cargo test -p vega_store --lib --locked --offline -j 2` | 27 执行/27 通过，12.89 秒 | `/tmp/vega-919-store-revalidation.log` |
| `cargo test -p vega --bin vega --locked --offline -j 2` | 7 执行/7 通过，1.00 秒 | `/tmp/vega-919-app-revalidation.log` |
| `cargo clippy --workspace --all-targets --locked --offline -j 2 -- -D warnings` | store 原 14 lint 修后已编译；当前失败于 conversation lib 9 项、lib test 10 项（重叠，不相加），修复中 | `/tmp/vega-919-clippy-revalidation.log` |

78 项包含慢锁外准备暂停时父及两子消息/工具完整终态、fork 暂停隔离、pending 四项、privacy token 四项及压缩诊断八项；27 项包含永久清理七项、迁移链和 sidecar；应用七项包含 Codex 五项与可信动作 lease 两项。专项与全套计数重叠，不能累加。测试发生时存储 lint 范围仍在变动，修后仍须冻结版本复验，不拿本表替代最终 head。

失败责任层：永久清理 offset 夹具先后违反 generated notice 必须 streaming、response start 必须位于当时内容尾部的真实 API 契约；按真实生命周期修正夹具后，七项全部通过，生产断言未放宽。会话首次 78 执行/77 通过/1 失败来自 Failed 诊断夹具未提供 failure_code；`root_event` 原已补 Cancelled/Interrupted 码，无生产取消缺陷，未改生产分类。夹具仅在 Failed 提供 RuntimeFailure 并增加落库失败码断言，专项及全套如上通过。新诊断模块引用 undeclared rusqlite、旧 helper 名与 Debug 比较要求均按现有 API 修正，未加依赖。集中命令首次被宿主 shell 将字符串实参视为单个 Cargo 子命令拒绝，未执行测试；改用 bash 数组显式参数后才取得本表结果。

来源边界另见 database-writer.md：Skill/凭据末检的同步路径及 metadata 仍可停顿，不读 body/解析文件或取得文件锁，但无耗时保证；本轮慢准备回归不证明任意慢盘场景。保留来源替换和数据库身份安全检查，不把 metadata 宣称有界。独立全卡审查、Rust 测试留痕清理、最终 required CI、合并均未完成。

## 停写前全工作区复验

上述 lint 修复已完成；统一 `cargo fmt --all` 后，`cargo fmt --all -- --check`、`git diff --check` 与 `cargo clippy --workspace --all-targets --locked --offline -j 2 -- -D warnings` 均退出 0（Clippy 15.35 秒，日志 `/tmp/vega-919-clippy-final-development.log`）。保留既有 manifest unused dependency 提示，未删依赖、加 allow 或改 CI 求绿。

`cargo test --workspace --all-targets --locked --offline -j 2` 首次在 store JSON 碰撞夹具失败：26 通过/1 失败；其他已执行分组不是失败总数。`cargo tree --workspace --locked --offline -e features -i serde_json` 证实全工作区 GPUI 启用 `preserve_order`，单包默认则排序。原清理算法按实际 JSON map 遍历顺序分配碰撞后缀，夹具误将另一构建特性的遍历次序写成契约。仅将原始 JSON 输入的两个字段顺序调整为两种特性均一致；原两键、两值及历史身份断言未改，生产算法未改。

修后单包该专项 1 执行/1 通过（0.19 秒，`/tmp/vega-919-json-default.log`）；同一全工作区命令完整执行 **112 项：app 7/7（1.09 秒）、conversation 78/78（43.50 秒）、store 27/27（13.83 秒），0 失败/0 忽略/0 过滤**，其他目标是 0 测试，不称行为通过。日志 `/tmp/vega-919-workspace-development-tests.log`，编译阶段 11.82 秒。命令仍设置 `CARGO_FORK_AUTO_UPDATE=0`，工具链未变。

同版本三组 Python 实际命令分别 `python3 scripts/release_tests.py`、`python3 scripts/prepare_computer_use_tests.py`、`python3 -m unittest discover -s scripts/tests -v`，分别 5/5、15/15、40/40，退出 0；日志 `/tmp/vega-919-python-{release,computer,suite}-finaldev.log`。suite 中 Unsupported bundle path 是失败路径夹具的预期输出，不是命令失败。

当前源码停止写入，随后保存开发提交（包含测试，仅用于可恢复证据及独立实现审查，不得直接合并）。最终移除开发 Rust 测试及独占支持后，仍须核对清理差异、最终 head 全套 required CI；本地行为通过不代表真实 UI/外部服务或用户验收。全库迁移不分批合并，不安装新应用，不改用户库。

## 开发独审结果与修复中状态

三份非实现者审查绑定已发布开发提交 `32af3eb9d5eb19d25d878b495f2b330066a406ea`，比较基线 `2a29adf9b2ea78eaf59d591838531b45adc7373e`。均已完成，但均有阻断，不代表通过：

- 核心 writer：P1，全库只限制待注册队列，已注册运行可无限增长，且无界排空 registration 可饿死已有工作。主线新增覆盖待注册+活跃直到 drain 的 128 个运行 permit、每轮一个注册、满额立即明确拒绝（非新增等待），并检查 queued cancel/abandoned startup receipt，生产编译已通过，独立范围正在补饱和/持续注册公平性开发回归。
- 服务安全：P1，外部直接修改默认权限 YAML 不推进 SQL generation。Guard 现在锁外读取前后捕获文件身份/版本，并在安装前后仅作 metadata 校验，保留所有 epoch/generation/intent/source CAS；专项实际 `cargo test -p vega_conversation --lib --locked --offline -j 2 permission_contracts::intents::tests:: -- --nocapture` 4 执行/4 通过，1.19 秒，日志 `/tmp/vega-919-draft-source-tests.log`。真实 actor 安装前暂停，直接原地写、原子替换、删除配置均拒绝，线程不创建、intent 保持 prepared、SQL generation 不变。未把 YAML body/配置锁搬进 actor；metadata 仍无耗时保证。
- 服务安全：P2，MCP metadata 预检与实际 SQL 写入不在同一事务，外进程登记 import 后可破坏 source 恢复。独立实现范围已交付八类命令同事务检查+写入、五项开发测试源码，尚待中央运行。
- 应用/委派：P2，CodexWrites Drop 同步等待普通 actor 回执。独立实现范围正在移至显式 worker 收尾，Drop 仅保留预留 necessary 通道；开发回归待交付。

以上均为本卡范围内具体缺陷；修后需集中复验并交非实现者增量审查，旧 112/112 与旧 Clippy 不证明新字节。开发 Rust 测试尚未清理，最终 required CI、合并、用户手测均未发生。

## 四项独审修复中央复验

全部实现范围已停写，原三审阻断均已实现修正，尚待非实现者增量结论。

- 核心专项 `cargo test -p vega_conversation --lib --locked --offline -j 1 agent::persistence::database_writer::tests:: -- --nocapture`：21 执行/21 通过，7.98 秒；日志 `/tmp/vega-919-capacity-review-tests.log`。含128运行上限、真 drain 释放、持续注册公平、排队取消、丢 ready、关闭结算、既有 necessary 失败不报成功。
- MCP 专项 `cargo test -p vega_conversation --lib --locked --offline -j 1 mcp_settings::protocol_tests:: -- --nocapture`：10/10，8.96 秒；日志 `/tmp/vega-919-mcp-review-tests.log`。八类 metadata 同实际事务复核 pending/projection，独立连接 import 交错拒绝后恢复成功。
- Codex 应用专项 `cargo test -p vega --bin vega --locked --offline -j 1 codex_agent::behavior_tests:: -- --nocapture`：8/8，4.55 秒；日志 `/tmp/vega-919-codex-settlement-app.log`。会话首次错用 codex_tasks::persistence_tests 过滤器，0 执行不算通过；核对实际模块后 `cargo test -p vega_conversation --lib --locked --offline -j 1 codex_persistence::persistence_tests:: -- --nocapture`：5/5，2.10 秒；日志 `/tmp/vega-919-codex-settlement-conversation.log`。
- 全工作区 `cargo clippy --workspace --all-targets --locked --offline -j 1 -- -D warnings`：通过，24.43 秒；日志 `/tmp/vega-919-clippy-review-fixes.log`。首次新增 MCP 测试 tuple type_complexity 失败，仅提取 ConfigState 类型别名后重跑，未放宽 lint 或断言。现有 manifest unused dependency 提示未修改。
- 同一源码 `cargo test --workspace --all-targets --locked --offline -j 1`：**128 执行/128 通过，0 失败/0 忽略/0 过滤**；app 10（4.06 秒）、conversation 91（68.25 秒）、store 27（15.36 秒）。编译55.98秒，日志 `/tmp/vega-919-workspace-review-fixes.log`，其他目标0测试不称行为通过。
- `cargo fmt --all -- --check`、`git diff --check`：通过。本轮仍固定工具链及 `CARGO_FORK_AUTO_UPDATE=0`，因其他任务构建使用 -j1 协调负载；未干预其他任务进程。Python前轮5/15/40为未改脚本版本证据，最终无Rust测试head仍须重跑。

保存新的修复开发提交后停止写入交增量独审；保留完整开发测试，不先清理换绿。来源metadata末检风险及用户真实外部/UI手测限制不变。未开最终PR、未合并、未安装。

## 独审通过与最终测试清理

三位非实现者已在精确开发 head `f8ec21230d7a720612819bc9ede9a65553f74cdb` 对原发现的修复增量分别给出 PASS：core（全库容量/注册公平）、services（默认权限来源/MCP事务）、app（Codex非阻塞Drop/显式收尾）。继承未变范围的原审查结论；静态限制与真实UI/外部服务待用户手测不变。

开发回归源码和128/128证据保留在已推送的上述提交。清理只删除17个外部开发测试模块、4个文件尾部内联测试模块、模块接线及 pipeline 单一测试暂停钩子；Rust清理差异38文件、7361行删除、0行新增。未新增/删除生产接口、依赖、feature、Python fixture或通用CI。首次清理预检误将lib.rs同级模块解析为子目录，尚未执行删除即失败；修正模块路径规则，所有目标核对完成后统一删除。该工具问题不作产品缺陷。

最终无测试源码的 `cargo fmt --all -- --check`、`git diff --check`、`cargo clippy --workspace --all-targets --locked --offline -j 1 -- -D warnings` 均通过，Clippy27.85秒，日志 `/tmp/vega-919-clippy-no-tests.log`。静态Rust测试属性/cfg接线/暂停钩子扫描为0；三份manifest与Cargo.lock相对基线无差异，无新增可执行doctest。

本机 Nextest **0.9.151**（不冒充CI固定0.9.146），`CARGO_FORK_AUTO_UPDATE=0 CARGO_BUILD_JOBS=1 cargo nextest list --workspace --locked --offline --message-format json`：**0发现**；`cargo nextest run --workspace --locked --offline --no-tests=pass`：**0执行**，16 binaries，明确记录 **no Rust tests**，不是行为测试通过。首次错误传-j被CLI拒绝，未执行清点，改用CARGO_BUILD_JOBS控制编译，不改变Nextest测试默认并发。日志 `/tmp/vega-919-final-nextest-list.{json,log}`、`/tmp/vega-919-final-nextest.log`。

最终源码三组Python实际重跑 `python3 scripts/release_tests.py` 5/5、`python3 scripts/prepare_computer_use_tests.py` 15/15、`python3 -m unittest discover -s scripts/tests -v` 40/40，退出0，日志 `/tmp/vega-919-final-python-{release,computer,suite}.log`。

接下来保存最终无Rust测试head，清理delta独审与该head required CI并行；尚未取得最终CI、未合并、未安装、未用户验收。保留两处本轮Python生成但尚未删除的__pycache__，不作未知用户数据删除。

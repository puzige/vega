# Codex 在线持久化迁移（#919）

## 范围与规格

共享基线 `555b0c0b3c00031ad22b69c7a5622e3c1dc4116b` + 未提交集成。续派授权：Codex worker/service、window/agent.rs 的 Codex 启动函数、codex_tasks.rs 及子模块、store Codex 必要事务 helper 与本文。未改 lib.rs/shared writer/app_agent.rs/权限/MCP/Skill/Git index；未 Cargo、Git 写操作、全局格式化、用户数据操作或委派。前阶段 lib.rs export 和 tool_calls 通用 helper 差异保留，不冒充本轮新增。

目标仍为一次性全库迁移，不分批合并。本文仅本独占范围交接，不代表整库完成/可合并 READY。

- UI 不同步等待 actor。预检结束后，后台用只读连接验证文件快照、取得线程 admission，复用 DraftCreationGuard durable intent；具名 actor 短事务按 permission → writer 锁序重验 authority/default generation、owner、项目和初始 session 来源，原子安装 draft、Codex snapshot 与 session intent。失败回滚不得有线程/快照/session 半成品。
- 不跨 commit 持 ConfigEdit/permission/source mutex。文件、目录检查、快照序列化、配置/凭据、ACP、权限队列、UI 回调留 actor 外。store `PreparedCodexSnapshot` 只携 owned 数据；事务内 bind 不 BEGIN，不改变通用 SQLite 事务语义。
- 启动回执携同一 admission 的 persistence service，不在 worker 再 acquire。异步 UI 回调重验可信动作令牌、stream 归属、完整 expected draft、Settings 状态；仍经 controller begin 取得 generation/cancel。晚到/取消/stream 替换/worker spawn 失败不得启动 ACP。
- service 预留 NecessaryLease；丢弃启动结果/窗口关闭/无法 spawn 时，用具名 AbandonedStartup 核对原 snapshot/intent/owner 后结算。未开始外部进程为 definitive failure；开始后仍是 intent 则 uncertain；已 confirmed/终结不覆盖。worker 后台退出前显式等待该结算，Drop 是未回执时的必要通道后备。查询用既有 codex_task_identity，不强制读排队。重验生产连接身份由共享 commit/necessary request 执行，不改其接口。
- 新增/敏感 worker 写重验权限来源，已发生必要终态不因权限变化消失；精确 task/message/tool/input/display 来源仍必须匹配。消息 user+assistant 和 tool seq 在 actor 事务内分配。此 service 仅新任务首对消息，数据库已存在消息时拒绝换新 ID 重放，未知必须先查询核对。
- prompt 所有收尾分支先取消未决选择、等待本轮已派发权限任务结束，再消费真实 resolution 并提交终态。有效真实允许且 ACP 回复成功才记用户批准；未选择/无效选项不伪造用户批准。真实终态早于用户拒绝时，专用 Codex SQL helper 保存执行终态与 deny/user 双事实，不补造自动批准，不修改通用状态机断言。
- 不明工具写标记 persistence_uncertain，prompt finalization 不盲重放。失败健康状态阻止正常完成回执；不把日志/已入队/外部请求当持久化成功。

## 原入口覆盖与关键接口

| 原入口/场景 | 新边界 |
|---|---|
| UI materialize_codex_draft + begin_codex_session_creation | `codex_tasks::prepare_codex_startup(path, draft, snapshot)`，后台准备 + `PreparedStartup` 原子 actor 安装 |
| UI database path/controller begin/spawn 失败标记 | 路径先检查；`CodexPersistence` 的必要租约与 `AbandonedStartup`，UI 不写 SQLite |
| worker admission 失败窗口 | 返回 service 已持同一 owner；不再二次 acquire；启动前 `validate_external_start` 执行端重验 |
| worker runtime/process/initialize/mode failure，session unknown/confirm | `CodexWrites::session` → owned `CodexSessionOutcome`；不回放外部操作 |
| persist_codex_messages | `create_messages`，连续 seq、原子消息对、数据库首对防重放 |
| streaming/finish messages | `stream` / `finish_message`，精确 assistant/streaming 来源和变更数断言 |
| proposal | `propose_tool`，owned identity/preview，事务内 tool seq 与 display metadata |
| approval/running/rejected/finish | `change_tool`，真实 observed in-progress / terminal 事实与审批审计同事务 |
| permission/prompt 交错 | `select_codex_permission` 校验真实队列可选项；`settle_codex_permissions` 等待 task 并消费 resolution |
| 真实终态早于拒绝选择 | `Finish.rejected_by_user` → `finish_codex_observed_after_denial_in_transaction`，严格终态配对和 deny/user 校验 |
| 不明/取消/收尾 | 必要真实事实先落库，未知标记不自动重放；无成功回执不报告正常完成 |

删除 conversation 中已无调用的旧直接写 API：materialize/bind/begin/confirm/mark uncertain/mark failed；全 crates 搜索零调用。只读 codex_task_identity/codex_prompt_binding 保留。

公开语义接口：`CodexStartup { thread, snapshot, intent, persistence }`；`prepare_codex_startup`；`CodexPersistence::{validate_external_start,mark_external_started,settle_abandoned_startup,session_outcome,create_messages,stream,finish_message,propose_tool,change_tool}`。不接受 SQL/closure。内部 sealed `PreparedStartup` / `AbandonedStartup` / `CodexCommand` 仍使用兼容 `commit(path, identity, command)`。

store 新增最小 helper：`PreparedCodexSnapshot`、锁外 `prepare_snapshot`、`bind_prepared_in_transaction`。旧 store bind wrapper 委托同一 helper，原有 standalone Codex SQL/初始 session trigger 语义保留。tool_calls 本轮只新增拒绝+真实终态专用 helper；其余 diff 含前阶段三个 Codex 事务内 helper 与其他已有通用 helper。

## 开发行为回归（未运行）

新增 15 项开发 Rust 测试，真实临时 Store/migration 与同库 actor；外部 ACP 回复边界隔离，不替代权限队列/选择决策。未新增依赖。

- `codex_tasks/startup/tests.rs`：9 项，原子新 draft+snapshot+intent/唯一 owner；session SQL 失败全回滚；排队 authority/default generation/pending 及错误 owner 拒绝；默认文件变化拒绝；abandon 前/后外部启动 definitive/unknown；权限/session 来源变化拒绝；消息/tool 序号和真实终态合法路径；消息 SQL 失败不自动重试；startup 回执丢弃后同一 intent 结算且不重放。
- `codex_tasks/persistence_tests.rs`：1 项，真实 actor 提交消息后丢弃回执，核对消息只安装一对，换新 ID 也不得重放。
- `codex_agent/behavior_tests.rs`：5 项，真实权限队列有效选择延迟 resolution 与 prompt 收尾；无选择不能伪造批准；未提供选项拒绝；用户拒绝+真实终态并存 deny/user；工具 SQL 失败后 finalization 不重放。

中央建议命令（未执行，不是结果）：

```text
cargo test -p vega_conversation codex_tasks::startup::tests -- --test-threads=1
cargo test -p vega_conversation codex_persistence::persistence_tests -- --test-threads=1
cargo test -p vega codex_agent::behavior_tests -- --test-threads=1
cargo check -p vega --all-targets
```

测试只留开发工作树，合并前由主 Agent 固定源码证据后清理三文件及三个 cfg(test) 接线。未恢复旧测试集、无 ignore/放宽断言/重试换绿。真实 GPUI 晚到结果/停止交互尚无本轮自动执行证据，需中央回归或用户手测；测试名数量不代表发现/执行/通过。

## 静态证据与精确身份

最终定向 `rustfmt --edition 2024 --config skip_children=true --check`：10 个授权 Rust 文件退出 0；window/agent.rs 只格式化两个 Codex 函数，提取 wrapper 格式检查通过，不格式化其他函数。`rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`。

`git diff --check --` 对 5 个 tracked 授权代码文件退出 0。新文件由 rustfmt 检查。全 crates `rg` 查 materialize_codex_draft/bind_codex_task/begin_codex_session_creation/confirm_codex_session_creation/mark_codex_session_/CodexPersistence::acquire/CodexWrites::acquire 无匹配。app worker SQL 写调用搜索无残留；actor 无文件/ACP/凭据/await 回调。仅静态格式与搜索，不证明类型编译或行为通过。

以下表顺序 `path + NUL + bytes + NUL` SHA-256：

- 生产 8 文件：`418be10bde7c593620e9b5127924e07146f091699209a70dc94e220796631d6f`
- 全部 11 文件含开发测试：`9458ebeb39bc9130008e7cbe72686ddc853429a68fd9198330ac7932bd738366`
- 差异 identity：`bff28049d391310ef53d71ffe5b1d5cf68f443a6dd88ae9457f0635f531b8129`，同表顺序 `path + NUL + (git diff --no-ext-diff HEAD -- path；未跟踪则原bytes) + NUL`。含既有授权文件 diff，不是本轮单独 patch，也不是提交 SHA。

| 文件 | SHA-256 |
|---|---|
| crates/vega/src/window/agent.rs | fdb16d7a0d0a00f12eb2914af39c02ce2d12c34e27a337fb48545d0349b361d3 |
| crates/vega/src/codex_agent.rs | f5a23d82d5f7d898b95024d80b056391f96f921d2e4ece659dc7b86e44479b22 |
| crates/vega/src/codex_agent/writes.rs | f963803af7bdb2339855b6b3cb98bf001263478f3f530f51752b393a87c16234 |
| crates/vega_conversation/src/codex_tasks.rs | 7e3bed282ea15f42bac0462a686dc50d5d358ceb2f07d885d6286d8bf7f24a2e |
| crates/vega_conversation/src/codex_tasks/startup.rs | eb528c67631ece1af3d4c5020a9e1a74930953035efb1224c1f3a97e1b6c8ce3 |
| crates/vega_conversation/src/codex_persistence.rs | 9f22d4e9aeecc96508fa097c261383bd26c1cbf4fdf380a59692f124fe1ecb47 |
| crates/vega_store/src/codex_tasks.rs | d4f7b2785f3bda467a3cae53dec14dcd558328fd8dd5155e7518fa61a03ee2dc |
| crates/vega_store/src/tool_calls.rs | 430ee502ac927c663f46ea24812e43c67551ae50d3e480561e0935985f2d1eac |
| crates/vega/src/codex_agent/behavior_tests.rs | 220ec4a56a522cb08815a9d34b4b202b5d8bdc79fa17e6229e49ea44452af27d |
| crates/vega_conversation/src/codex_tasks/startup/tests.rs | 5103e7190dbd2ade95506943943661796dc25646f1925eef75264abe8f1fa739 |
| crates/vega_conversation/src/codex_tasks/persistence_tests.rs | e20fbdf48d3e10e161cf35f22bbc612417c63d415f84e04d754878093df0851c |

## 未决验证与边界

1. 未运行 Cargo/check/Clippy/15 项开发测试/CI/非实现者独立审查/GPUI 手测。用户提供的此前 app 编译和 conversation 39/39 不覆盖本轮新字节；必须中央对冻结字节验证。测试可能暴露产品或接线问题，停写后由主 Agent 接管，不自动认为通过。
2. NecessaryLease/actor shutdown/source migration 依赖主线既有 shared writer；本轮不改接口。租约注册失败在业务安装前返回；已排队必要结算失败由 shared execute_terminal(false) 保留失败事实。异常进程退出仍需依据 durable session intent 查询恢复，不承诺崩溃后已执行外部动作可重放。
3. 错误仍为 String，保留 commit/SQL 真实错误，未知不冒充没写。没有提供任意 SQL/closure 公共入口或把在线 migrate 当例外。未知工具提交阻止本轮重试，但没有新增自动恢复 UI；用真实数据库事实核对，不能无证据重放。
4. 真实 ACP 进程/传输、系统退出/drain 和 GPUI generation/late-result/cancel 仍需中央集成/手测。新测试隔离外部 ACP 回复，不能冒充真实外部协议验收；跨库替换/关闭并发需共享 actor 回归覆盖。
5. 本轮没有发现必须扩大授权才能实现的接口阻碍；未保留本 scope 在线直接写旁路。停写交主 Agent 中央验证，不做部分合并。

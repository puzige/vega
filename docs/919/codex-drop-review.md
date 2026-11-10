# #919 Codex 非阻塞 Drop 修复

## 范围与身份

- 本轮是独审 P2 的实现交付，不是非实现者独立审查。
- 起点 HEAD：`32af3eb9d5eb19d25d878b495f2b330066a406ea`；用户指定 baseline：`2a29adf9b2ea78eaf59d591838531b45adc7373e`。
- 用户已完成 fetch，origin 不变；本轮禁止 Cargo、Git 变更、桌面操作及用户数据库访问。仅修改指定 Codex 文件与本文，不修改主线 core 容量/权限或 MCP。

## 验收（实现前固定）

1. `CodexWrites` 析构不得调用同步普通 commit 或等待数据库回执。取消后未进入 worker、线程创建失败导致 request 析构、启动后未确认的异常析构，均只由 `CodexPersistence` 使用启动时预留的 necessary lease 非阻塞投递。
2. worker 的全部正常返回与早退必须汇入显式收尾，显式等待真实 necessary ack，之后才计算返回值；调用方只能在 worker 返回后发 `AgentUpdate::Finished`。不能仅删除 Drop 而遗漏显式收尾，也不能因为业务失败而短路跳过收尾。
3. 外部启动前的未结算 intent 变为 `DefinitivelyFailed(ProcessStartFailed)`；外部启动后未确认变为 `Uncertain(OutcomeUnknown)`。持久化 confirmed/terminal 不得被 abandoned 结算覆盖；结果未知不能当成未执行，不能自动重试外部动作。
4. explicit settlement 使用预留 necessary 通道，不依赖 ordinary 队列容量/接纳状态。提交后消费 lease；丢回执不丢必要写，不能因 ack 未知再次投递/重放外部动作。执行失败必须使 worker 返回失败，并由 shared writer 保留 drain 失败事实。
5. 回归使用隔离临时数据库、真实 SQL 和 sealed actor gate，无新增依赖或生产 callback。暂停 actor 时，启动前和启动后未确认的析构须在恢复 actor 前完成；恢复后核对必要结算。显式收尾在恢复前不得返回，恢复后核对 ack、业务结果和 Finished 顺序；覆盖 confirmed/terminal、丢 ack、必要结算 SQL 失败。

## 验证与交付边界

- 开发测试仅供主 Agent 集中验证，本轮不执行 Cargo；未运行不计通过。Rust 开发测试合并前按仓库政策清理并留痕。
- 本轮允许定向 `rustfmt --edition 2024 --config skip_children=true` 和静态 diff/调用路径核对。
- 最终记录实际命令、结果、文件 SHA-256、未运行项及停写状态；不提交、不推送、不修改 Git 状态，不把实现者自查称为独审。


## 实际修复与出口核对

- 删除 `CodexWrites::drop`；隐式字段析构继续调用既有 `CodexPersistence::drop`，仅 `get_mut` 取出预留 lease 并 `try_send`，不建立普通 commit、不等待回执。
- 新增消费式 `CodexWrites::finish_worker(self, success)`；先执行 settlement 并记录真实错误，再计算 `success && settled && healthy`，失败结果/已有 unhealthy 均不会短路收尾。
- `run_codex_agent_worker` 业务体收进一次调用的闭包，全部业务 return 退出闭包后必经 `finish_worker`。不增加线程、外部重试、依赖或生产 callback。
- `settle_abandoned_startup` 将普通 commit 改为消费预留 necessary lease，显式 `receipt.recv()` 等真实 ack。提交后无论成功、SQL 失败、ack 未知均不恢复 lease，Drop 不二次投递；重复显式调用返回需对账错误。confirmed/terminal 的本地状态直接返回；本地状态滞后时仍由既有 `AbandonedStartup` 在 SQL 内核对同一 intent 并保留 durable confirmed/terminal。

| 路径 | 顺序 / 结果 |
|---|---|
| worker 尚未开始、线程创建失败 request 被丢弃 | persistence Drop 必要投递，不等待；外层失败 Finished 不是已结算 ack 的证明 |
| 入口已取消、runtime 创建失败、验证前再次取消、validate 失败/随后取消 | 闭包 false → 显式 finish → 必要 ack（仍为 intent 时 definitive）→ worker false |
| spawn 失败 | 保留既有 definitive outcome 写；闭包 false → 显式 finish → worker false；未知 ack 时必要 SQL 只核对事实，不重试外部 |
| initialize / new_session / set_mode 失败或取消、session 确认失败 | 既有显式 outcome 逻辑不变；async false → shutdown → 显式 finish；剩余 intent 且 external_started 时 uncertain |
| 消息/流/工具路径早退、prompt 正常/失败/取消结果 | async bool → shutdown → 显式 finish → 结合 persistence healthy 计算 worker bool |
| `AgentUpdate::Finished` | 未改 window/agent.rs：其成功线程分支先调用 worker，再发送 Finished，故等待在事件前；线程创建失败分支不等待数据库，不声称已结算 |
| panic / 未经显式收尾直接丢弃 writes | 不承诺返回或 Finished；仅既有 persistence Drop 非阻塞 necessary 投递 |

外部启动边界仍是既有 `mark_external_started()`，不改变 ACP 初始化/会话协议。未修改 `AbandonedStartup` 的同 intent / snapshot / owner 检查、confirmed/terminal 保护或真实 SQL 状态转移。

## 开发回归（新增 7 项，均未运行）

conversation `codex_persistence::persistence_tests`：

- `abandoned_drop_returns_while_actor_paused_and_necessary_write_survives_lost_ack`：sealed command 暂停真实 actor；两种启动边界的 Drop 在恢复前返回；Drop 丢弃 ack 后必要写仍落库。
- `explicit_settlement_waits_for_real_necessary_ack_and_consumes_lease_once`：恢复前无返回、仍为 intent；恢复后得到真实 ack，第二次调用拒绝重投。
- `abandoned_settlement_preserves_durable_confirmed_and_terminal_when_local_ack_is_stale`：通过真实命令写 confirmed / uncertain / definitive 而保留本地旧 intent；必要结算不覆盖 durable 状态。
- `failed_necessary_settlement_reports_error_and_is_not_retried_on_drop`：SQL trigger 使结算失败；去掉 trigger 后也不二次提交，Drop 后仍为 intent。

app `codex_agent::behavior_tests`：

- `cancelled_request_and_started_writes_drop_do_not_wait_for_sql_writer`：真实 SQLite 写事务阻塞 actor；启动前取消 request / 启动后 writes 析构在释放 SQL 写锁前返回，之后核对状态。
- `cancelled_worker_returns_and_publishes_finished_only_after_settlement_ack`：入口取消的真实 worker 在 SQL 写锁释放前不发 Finished，释放后确认 definitive 与失败 Finished。
- `explicit_worker_finish_preserves_business_failure_and_reports_settlement_failure`：业务成功/失败、已有 unhealthy、SQL 失败四种组合，核对实际 worker finish bool 与数据库状态。

集中验证还须运行既有 actor 回归 `shutdown_waits_for_active_run_and_reserved_terminal_even_without_receipt`、`necessary_terminal_failure_is_not_reported_as_successful_shutdown`，覆盖丢 ack 不丢 drain、necessary 执行失败不报 drain 成功。本轮没有改 actor 文件；其 per-writer close/wait 接口为私有，未扩大生产接口，也未调用会污染全局注册表的 shutdown_all 来代替隔离测试。新增 Codex 失败回归验证必要命令真实 SQL 失败与 lease 不重投；shared drain 失败事实由既有 `NecessaryRequest::execute_terminal` 及上述 actor 回归承担。主线 actor 并行修改，最终集成版本必须由主 Agent 集中运行，不以历史结果代替。

## 实际静态检查与停写身份

执行工具：`rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`。未调用 Cargo / rustc，未运行测试、CI 或应用，不访问用户库，不作提交/推送/rebase/fetch 等 Git 变更。

- 定向 `rustfmt --edition 2024 --config skip_children=true`：下表 5 个 Rust 文件，退出 0。
- 定向 `rustfmt --edition 2024 --config skip_children=true --check`：下表 5 个 Rust 文件，退出 0。
- `git diff --check -- <下表 5 文件> docs/919/codex-drop-review.md`：退出 0；新文档另做尾随空白检查。
- 静态核对 worker 全部 return 汇入显式 finish、window 的 Finished 在 worker 返回之后；普通 commit 已从 startup settlement 消除、Drop 未增加等待或重试。以上不证明类型编译或行为通过。

下表顺序 `path + NUL + bytes + NUL` 的联合 SHA-256：`61bc2e338bf361ce1906d305e39c184f9f3d126c8cf4978baf3bc64b98d41375`。它是未提交内容身份，不是 Git 提交 hash；文档自身 hash 在最终交付消息记录，避免自引用。

| 文件 | SHA-256 |
|---|---|
| crates/vega/src/codex_agent.rs | 4969fdd6afa8cf464496cbe7de2d8256b35fbfb02a5a1abe034e5c8367bd27b3 |
| crates/vega/src/codex_agent/writes.rs | e9a301a2f78ca664d8c5909fa193615e1dd93ebcd419ba63ea94377a325bf95a |
| crates/vega/src/codex_agent/behavior_tests.rs | 3b858ab8d0f8cb55054e82eb931188f749d260ac640465b0528ae313c0949705 |
| crates/vega_conversation/src/codex_persistence.rs | f41ff8a44118926000c7ccb20a39026ce55c4e7aa003fcf45e850ec3c10f5902 |
| crates/vega_conversation/src/codex_tasks/persistence_tests.rs | d81321fab484f0ae2811ddeb40d01df9da6d9693ca37fe5c22947d38d0f8f419 |

未修改 startup/tests.rs，无新测试专用文件/接线。只写上表 5 文件与本文。工作区另有主线 core/权限、MCP 等其他 agent 的改动，均不清理、不暂存、不覆盖。交接时停止本范围写入；主 Agent 接手集中编译/回归、最终版本 CI 与非实现者独审。本报告不是独审结论，未运行项不得算通过；合并前仍需按仓库政策留痕并清理 Rust 开发测试。

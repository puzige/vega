# MCP metadata P2 修复契约与交付记录

## 范围与版本

- 工作树：`/Users/puzige/Workspace/worktrees/vega-919-database-writer`。
- 起始 HEAD：`32af3eb9d5eb19d25d878b495f2b330066a406ea`；用户指定 baseline：`2a29adf9b2ea78eaf59d591838531b45adc7373e`。
- 本轮 fetch 已由主会话完成，origin/master 不变；本实现不执行 Git 写操作。
- 独占写范围：`crates/vega_conversation/src/mcp_settings/writer.rs`、`protocol_tests.rs`、`crates/vega_store/src/mcp_servers.rs` 及本文件。core capacity 与 Codex scope 不在本次范围。

## 修复前证据

`SaveMcpMetadata::apply` 在真实 SQL BEGIN 之前读取 `CleanupProjection`。进程内 write permit 不能阻止另一进程在检查后、BEGIN 前登记 file-import pending。尤其 LastError 只按 id/revision 更新，可能更改已被 import intent 捕获的 rows，使恢复时 `Source` 比较永久冲突。其他命令即使被 `stage` 拒绝，也不满足来源复核与写入同一事务的契约。

## 实现前验收契约

1. 所有 metadata command 的 expected projection（accepted digest、generation、pending declarations）比较、pending 拒绝、目标 revision CAS、写入、cleanup outbox 和必要的 `stage` 均位于同一个真实 `BEGIN IMMEDIATE` SQL 事务；LastError 不 stage，但仍受相同来源检查保护。
2. 提取以 `&WriteTransaction` 接收事务的 `*_in_transaction` helpers，helpers 不 BEGIN、不 commit、不调用旧自带事务 API。旧的 Connection API 签名、返回值和业务校验保留，并由 wrapper 建立/提交事务；不改变全局事务实现或 nested BEGIN 拒绝，不用进程 permit 代替 SQL atomicity。
3. 逐类覆盖 Create、LocalRotation、OAuthInstall、LastError、Replace、Enable、Disconnect、Remove。来源或业务校验失败回滚，不改变 rows、identities、digest 或 pending；commit 异常沿用 `commit_with_rollback` 既有错误语义，不把未知提交结果称为成功。enable 的 registry suspended 清理仅在 commit 成功后进行。
4. 保持 registry → write coordination → SQL 事务锁序。actor 不做配置文件/YAML/body I/O，不接收任意回调；reconciliation 来源和数据库身份校验不削弱。
5. 必要开发回归使用独立 SQLite 连接等价重现交错：metadata 捕获旧来源后，独立连接登记 import pending；旧 metadata 拒绝且 SQL 状态不变；丢失 intent 后可以重新准备、保存和确认。各类正常路径及既有 rows / identity / digest 来源拒绝仍覆盖。

## 本轮验证边界

用户禁止本轮 Cargo、Git 变更、用户库访问、CI 改动、依赖或 allow。仅修改授权文件，允许定向 rustfmt 和只读静态检查。开发 Rust 测试按当前授权保留在工作树交付给主会话集中执行，本轮不称测试通过；合并前测试留痕/清理、required CI、非实现者独立审查与用户手测由主会话处理。

## 实施与静态结果

### 已实现

- `SaveMcpMetadata` 仅建立一个真实 `WriteTransaction`，在事务内复核 `CleanupProjection` 并拒绝 pending，八分支全部调用 typed transaction helpers，最后统一 `commit_with_rollback`。
- Create、LocalRotation、OAuthInstall、Replace、Enable、Disconnect、Remove 的原 SQL、校验、cleanup 和 stage 提取到同名 `_in_transaction` helper；LastError 提取其原校验与 UPDATE，仍不 stage。
- 八个 Connection API 保持签名/返回类型，wrapper 建立事务、调用 helper、提交。LastError wrapper 现在也明确建立自己的事务；嵌套调用必须使用 helper，不让全局 nested BEGIN 自动退化或复用。
- registry 锁序不变，Enable 的 suspended 清理位于 SQL 提交成功之后。无 actor 文件/YAML/body 操作或用户 callback。

### 开发回归源码（未执行）

保留原有 5 项协议测试，新增 5 项：

1. 八类 metadata 捕获旧 projection 后，由独立 Store/SQLite 连接通过真实 prepared reconciliation 登记 import pending；旧命令和已捕获 pending 的命令均 Conflict，rows、cleanup、identities、accepted digest、generation、pending base、pending declarations 完整快照不变；丢弃 intent 后重新 begin/save/confirm 成功。
2. 八类正常命令检查持久化 row、revision、slot/outbox、启用/删除状态及后续 pending 恢复；Disconnect 先安装 OAuth slot 再确认，验证真实旧 ref 入 outbox；LastError 不更改状态/identity/outbox。
3. 独立连接分别改 rows、identities、digest 后，confirmation 拒绝且无部分写入；原数据库身份及 generation 拒绝测试保留。
4. typed helper 在外层事务内写入，对独立连接不可见且可回滚；全局 nested BEGIN 和外部 wrapper nested 调用仍拒绝；独立连接直接 BEGIN IMMEDIATE 在外层写事务期间失败、回滚后成功。
5. 除 Create 外七类命令 stale revision 拒绝，SQL 快照和 registry suspended 保持不变。

这是独立连接的等价顺序交错，不是多进程端到端执行证据；事务持锁期间的独立连接原始 SQL 锁断言用于排除仅靠进程 permit 的解释。没有测试调度 callback 或生产 test hook。

### 实际执行的静态检查

- 源码版本：HEAD 仍为 `32af3eb9d5eb19d25d878b495f2b330066a406ea`，本轮产物为未提交工作树文件。
- 工具：`rustc 1.98.0 (88d9e12ae 2026-08-18)`；`rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`。未调用 Cargo，因此本轮 Cargo/fork 版本未取得。
- 定向 `rustfmt --edition 2024 --config skip_children=true`，随后相同三文件 `--check`：退出 0。
- `git diff --check -- <writer.rs> <protocol_tests.rs> <mcp_servers.rs>`：退出 0；仅 Git 只读操作。
- Python 只读源码核对：八个外部 API 签名一致；去除旧 permit/BEGIN/commit 并规范化 tx 引用后的原函数体，与新 helper 的词法 token 一致；八个 helper 不自建事务/commit；writer 单一 BEGIN → projection → 八分支 → commit → registry 更新顺序满足；原测试全文保留；新增代码无注释/allow，writer 无新增文件/YAML/callback 调用。全部断言成功。此为实现者静态自检，不是运行回归或独立审查。
- 未执行：Cargo 编译、Clippy、Rust 测试、CI、用户库或真实 MCP/凭据操作。主会话须集中编译/运行并处理结果，不能将本轮静态结果写作行为测试通过。

### 源码 SHA-256

| 文件 | SHA-256 |
|---|---|
| `crates/vega_conversation/src/mcp_settings/writer.rs` | `b7034968f0e53dc15d111ace1ba91e7c1e6a13fa105f7d80c6425cff85e4366b` |
| `crates/vega_conversation/src/mcp_settings/protocol_tests.rs` | `81da5e7a54e01d449c660aa287bf16241517673f536a438545da63316ee59d7a` |
| `crates/vega_store/src/mcp_servers.rs` | `63e6d69b05d3d59a4193113d735ee23dc36d71b5a8f6011777d51ec22e9496f4` |

本文件自身 hash 在交付消息单列，避免自引用。交付后停写，不提交、不推送、不清理其他 scope 或开发测试；本文件不是独立审查通过结论。

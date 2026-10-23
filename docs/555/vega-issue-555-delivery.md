# #555 修复与验证

## 版本与范围

- 规格 [v1.2](vega-issue-555-storage-contention.md)。用户本次明确要求主 Agent 直接执行，不再委派子 Agent；主 Agent 已接手并直接复核，不声称有完整独立审查结论。
- 初始基线 `4c0d225daf47e66e559752ee78ba067520bea080`；测试基线 `a39badd7cb79dd535157c886a485fbbcad22ca6d`。实现前同步政策基线 `d649b490ebdf831c13fb4d8a770eb2b77f1f4239`，集成提交 `492c031834b5b777628332e0518939f4ff58008d`；这轮同步仅改文档/技能，没有改变生产或测试。
- 生产只改 `crates/vega_conversation/src/agent/compaction.rs`：用量投影、手动准备、自动源捕获及失败源校验使用 deferred transaction；手动 source/checkpoint 捕获后立即结束读事务，再作凭据检查和历史投影。
- 真实写事务、checkpoint 安装 CAS、source/model/fingerprint/predecessor 校验、凭据 fail-closed、2 秒 timeout、审计、schema、依赖与 CI 不变。没有修改运行时数据库、安装或重启应用。

## 根因证据与限制

只读的上下文路径误用 `BEGIN IMMEDIATE`，预占 WAL 全库唯一写锁；其他会话正文/工具持久化因此可能在 2 秒后返回 BUSY。本机历史 content-free 诊断出现 SQLite `5/5`，但当时未记录持锁者，不能逐条归因。以下云端真实双连接回归确认本轮具体代码缺陷，不冒充用户真实模型/UI 验收，也不承诺消除外部占锁或所有长写事务。

## 原始测试与有效 RED

- 测试提交 `25222357ed242765d3c3f9606366c07fbed270df`；[原始源码](https://github.com/puzige/vega/blob/25222357ed242765d3c3f9606366c07fbed270df/crates/vega_conversation/src/agent/compaction/storage_contention_tests.rs)、[原始 patch](https://github.com/puzige/vega/commit/25222357ed242765d3c3f9606366c07fbed270df.patch)。SHA256 `4737fd0a60da1c392b51501aaadfb1b342ab1416904ea7b713ef23903115d94a`，实现期间未变。
- [Actions 39879602360](https://github.com/puzige/vega/actions/runs/39879602360)，[Nextest job 120173519994](https://github.com/puzige/vega/actions/runs/39879602360/job/120173519994)。现有 `cargo nextest list --workspace --message-format json` / `cargo nextest run --workspace`：10 发现/执行，4 通过、6 失败、0 跳过；fmt 和全目标 Clippy 成功。
- 原始失败：投影在 writer 活跃时返回 `Store("database is locked")`；手动凭据回调的真实 snapshot 保存得到 `Some(Err(Some(5)))` 而非 `Some(Ok(1))`，导致 `CredentialCheckUnavailable`；自动源捕获/失败源校验得到 Store，而非正确 provider 503 或 SourceChanged。无编译/fixture/runner 故障。
- 正向对照：凭据不可用与泄露仍拒绝 provider/checkpoint，真实写冲突仍 BUSY 且释放后可保存，只读连接投影正常。这四项 baseline 已通过，不伪造为目标缺失失败。

## 直接复核与本地静态检查

- 主 Agent 已逐处确认四个事务仅 SELECT；source/checkpoint/model/diagnostic/policy 仍各自从同一快照读取；没有读后升级写事务。手动提前 commit 不提交任何业务写入，所有隐私检查仍在 provider 之前执行；最终 checkpoint 安装保留原有原子源校验。
- `cargo fmt --all -- --check`：退出码 0。
- `git diff --check`：退出码 0。
- 对测试提交比较测试源码无 diff，SHA256 不变。没有本地编译、运行 Vega、Clippy 或执行 Rust/Python 测试。

## 最终云端验证与交付

待同一 10 个回归及完整 required check 验证。按最新仓库政策保留必要新测试，不删除测试或制造空集门禁。通过后主 Agent 复核最终 SHA 和 CI，再 squash merge；Issue 保持开放、In review，等待用户新版多任务与长会话/压缩手测。未取得结果不标通过。

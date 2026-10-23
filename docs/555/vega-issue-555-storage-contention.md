# #555 — 上下文读取不应阻塞会话保存

## 版本与证据

- 规格 v1.3，2026-10-23；初始生产基线 `4c0d225daf47e66e559752ee78ba067520bea080`，先行测试提交前 fast-forward 到 `a39badd7cb79dd535157c886a485fbbcad22ca6d`；上游仅改 Composer UI/文档，本卡生产文件与初始基线相同。
- v1.1 仅更新执行方式：用户明确要求不再使用子 Agent，后续测试复核、实现、审查与集成均由主 Agent 直接完成。没有取得已中断独立审查的完整结论，不将自查标作独立审查；验收及断言不变。
- v1.2 历史安排：同步 `d649b490ebdf831c13fb4d8a770eb2b77f1f4239` 后保留 10 个回归及接线；该保留决定现由 v1.3 取代。集成提交 `492c031834b5b777628332e0518939f4ff58008d` 仅同步上游文档，生产与测试文件内容未变。
- v1.3 用户纠正：取消强制 TDD 不改变 Rust 测试不得进入 master 的交付边界。测试可用于开发验证和留痕；清理 [PR #570](https://github.com/puzige/vega/pull/570) 带入的 `storage_contention_tests.rs`（10 个测试、914 行）及 `compaction.rs` 末尾独占的 `#[cfg(test)] mod storage_contention_tests;`，不改任何生产逻辑。保留原始源码/patch、带测试 GREEN 证据；最终删测 head 重新过完整 required CI 并确认 Rust 0 发现/0 执行。现存 Python 测试、fixture、CI 与依赖不变。
- Issue 仅包含 `Conversation storage is busy. Wait for other tasks to finish, then retry.` 截图，无复现步骤。
- 只读查询本机 `run_diagnostic_events`：24 条 SQLite primary/extended `5/5` 记录，分布为 snapshot 13、event_pipeline 5、tool_proposal 2、tool_terminal 4；没有 `SQLITE_BUSY_SNAPSHOT` / `SQLITE_LOCKED` 证据。只报告分类与数量，不公开会话内容、身份或路径。
- `Store::open` 的 busy timeout 为 2 秒。每个运行的 PersistenceActor 使用独立连接；因此另一连接持有写锁时，正文/工具生命周期保存会在超时后 fail closed。
- 已确认的代码缺陷：`read_context_projection_with_provider`、手动压缩准备、自动压缩 source/checkpoint 捕获、失败后的 source guard 检查，均只读却调用 `Store::immediate_transaction`。SQLite WAL 的 `BEGIN IMMEDIATE` 预占全库单一写锁，历史读取/投影/估算不必要地阻止别的会话写入。手动压缩还在此写锁内调用凭据读取/检查。
- 窗口 `refresh_context_projection` 在后台调用上述 projection，正常查看/刷新用量即可进入这条路径。历史每次事故的具体持锁者未记录，不能声称已逐条追溯；云端确定性回归将验证本轮缺陷的因果链。

## 验收矩阵

1. **读取与已持有写锁并行**：文件 SQLite/WAL，连接 A 已开始写事务并有未提交更改，连接 B 经真实 `read_context_projection_with_provider` 返回正确的已提交 source/model/policy/diagnostic/usage 投影。读不到 A 的未提交内容；A 提交后下次读取能看到新状态。不得把错误降为默认或空结果。
2. **只读数据库可投影**：经 `Store::open_read_only` 调用同一 projection 成功，结果与同一已提交源一致，不发生写入。
3. **手动准备不阻塞保存，凭据检查仍 fail closed**：真实手动压缩 API 读取有合格 bash 输出的历史；在其凭据读取回调内通过独立连接执行真实 streaming snapshot 保存，必须成功且读回一致。回调失败仍返回 `CredentialCheckUnavailable`，检测到凭据仍返回 `CredentialExposureBlocked`，两者都不能调用 provider 或安装 checkpoint。不能以延长超时/重试隐藏持锁。
4. **自动压缩只读捕获/失败源校验并行**：通过真实 compaction hook（必要时同模块测试访问现有私有 guard 检查），在另一连接持有写锁时能完成源捕获/校验并返回与无写锁相同的正确语义结果，而不是 SQLite busy。模型、source fingerprint 和 predecessor 不匹配仍拒绝；同一快照内 source/checkpoint 关系不放宽。
5. **真实写冲突仍可见**：另一写事务持续占锁时，真实 streaming snapshot 写入仍返回 SQLite busy；调用方将其分类为安全 `RuntimeStorageBusy`，既有失败消息不被吞掉。测试连接可显式降低 timeout 以有界执行，生产 2 秒值不改。

冻结前由主 Agent 按本次用户裁决直接复核必要行为和失败边界，测试使用进程内真实 SQLite/存储/会话逻辑；仅 provider 与凭据读取外部边界替代，不运行外部模型、网络或应用，不以源码匹配冒充行为回归。新测试本轮独立编写，不恢复历史测试集合。

## 最小实现范围

- 主 Agent 仅修改 `crates/vega_conversation/src/agent/compaction.rs` 中上述四处纯读取事务为 deferred、一致性快照；可复用已有 `Connection::unchecked_transaction`，不新增 public API、依赖、schema 或 CI 设施。
- 手动准备在取得 source/checkpoint 的同一快照后尽早结束读事务，再做不需要连接的凭据检查及 history projection；保持原顺序的校验语义与安全结果。
- 真正修改状态、安装 checkpoint、claim 或 run preparation 的 immediate transaction 保持原样；尤其不改变 `prepare_run_with_images_and_reasoning` 的原子性、隐私清理和工具审计。
- 不扩大为全局写入调度器，不延长 busy timeout、不加自动重试、不吞存储错误、不修改用户数据、UI 或错误文案。

## 开发与交付顺序

步骤 1–3 记录 #570 已完成的历史开发过程，不是当前任务的 TDD 前置要求；本次 v1.3 仅执行步骤 4 的清理与最终门禁，不重做既有验证。

1. 原测试写入者只写开发 Rust 测试/必要测试接线，生产行为不改。用户要求停用子 Agent 后，主 Agent 接手固定产物并直接复核，不再委派。
2. 主 Agent 提交测试/spec 并建立 draft PR，使用现有 `pr-check` Actions 获取目标行为失败的有效 RED；无关编译/接线/runner 故障不算 RED。
3. 冻结测试源码与预期；主 Agent 最小修改生产代码，测试不得追改。主 Agent 核对改动，再由同一 Actions 验证 GREEN。
4. PR 保留已取得的完整 SHA、原始测试源码/patch 与云端实际计数；测试源码在 `25222357ed242765d3c3f9606366c07fbed270df`，带测试 GREEN 在 `2068f26a8b93e8b399deb5458a83922a9442e0af` / [Actions 39880510836](https://github.com/puzige/vega/actions/runs/39880510836)。按 v1.3 清理 Rust 测试和独占模块接线，生产文件去除该接线后须与 #570 完全相同；对删测最终 head 重新运行完整 required CI，确认 Rust 0 发现/0 执行。测试历史和证据继续可访问，不恢复强制测试先行或 RED 流程。
5. 最新 required check 成功且主 Agent 完成最终内容复核后转正式 PR、squash merge；Issue 保持开放、看板 In review，等待用户新版真实会话手测。未安装或重启应用不称已安装。

本地只做快速差异/格式/静态检查；不运行 Vega、编译或执行测试。CI 启动后每 5 分钟查询，不 watch/短轮询。原生用户验收：多任务运行时切换/刷新长会话上下文用量及手动压缩，确认其他会话正文与工具继续保存；持续真实存储冲突仍应给出可恢复失败。

## 回滚与残余

生产只涉及读取事务模式及手动准备的只读计算边界；可通过 revert 合并提交回滚，不涉及数据迁移。此次去除已确认的无谓写锁，不承诺消除外部占锁或所有长写事务；若新版仍出现 busy，保留安全阶段/错误码以继续定位，不能将未知持锁者归因于本轮已修路径。

# D4：运行级凭据读取与不可变权限接线

## 范围与依据（2026-10-26）

落实主规格“严格凭据读取与脱敏”，复用 `vega_store::keystore::lookup_existing_key`（已在 `decccd52a675de7dbe8ebba0ffbea8285eadd512` 引入），不新增存储后端、不改 Provider/MCP readiness、命名工具保护或 Bash 执行实现。本阶段不是完整 D4 或 native cleanup 验收。

当前统一 reader 在权限 fence 校验前读取全部 binding，且 Provider、steering、子任务预先持有它。`privacy::refresh_authority` 和 `exception_filter` 会写同一数据库，并可能初始化 fingerprint key；不能把旧闭包直接移入 SQLite 写事务，也不能先初始化存储再判断严格 lookup 的 StoreAbsent。

## 接线契约

1. 每次主运行构造无 I/O 的 `RunCredentialReader`，保存配置根、binding refs、必需 Provider/MCP reader 与隐私上下文。公开的运行 reader 在接纳前一律失败；没有 UI mode setter，也不能由调用者直接启用。运行异常放行过滤器预先构造为惰性转发，在准备完成前不放行、不初始化存储。
2. pipeline 在已有 admission/pending/ordinary/delegated-owner 门禁之后，用短事务取得一致的真实权限快照。复用同一校验函数检查 ordinary immutable fence、delegated durable parent/child fence、automation claim、interrupted tail/fence/seq、线程身份和 FullAccess grant。Ask/Plan 即便 permission 字段为 FullAccess，也不属于 P2 可选 binding 豁免。
3. 释放该短事务后，在 permission coordination 和 admission 仍持有时读取凭据。先逐项严格 lookup bindings，再读取必需 Provider/MCP 值，再刷新 privacy authority/建立原有 exception filter。只有已校验 Execute + FullAccess 可忽略 `Ok(None)`；所有其他策略的 None、StoreAbsent、Unsafe/IO/Malformed/Limit/Unsupported 都失败。相同 ref 的必需角色不会因为可选查找而免除；Some 值始终参与脱敏，privacy 刷新返回本次实际汇总值（含首次生成的 fingerprint key），读取失败撤销内存 privacy authority/live 可用状态。空 binding 集合不为 binding 目的探测或创建存储；原有必需凭据和 privacy 初始化职责不被取消。
4. 在最终写事务中重新执行相同权限校验，并与凭据读取所依据的 fence、线程 model/backend/status、interrupted user 身份核对。漂移则失败，不提交用户/助手新消息，不启用 reader；不持有 SQLite 写事务调用 privacy 刷新。保留既有普通提交 rollback/unknown/receipt 语义。
5. 最终事务提交、普通提交回执记录后才一次性激活共享 reader。它绑定完整 fence 和主 assistant message id，不允许切换到更新的线程权限或另一次主运行。激活失败不把已提交事实改成未提交。后续每次读取重新检查真实存储并刷新脱敏/隐私 authority，不缓存密钥值作为读取成功替代。
6. Provider 外层与内部重试 guard、steering、runtime 输出脱敏和辅助模型调用使用同一 reader。子任务继承此对象；仅完整 delegated parent fence 与主 message id 相等时可准备，不在子线程重新推导 P2，不重新绑定父 reader。子任务既有独立 admission、durable fence、required MCP 和无继承例外策略保持不变。

## 必要开发验证

仅在既有 Actions 执行，保留现有 reset/startup/真实 GPUI 回归：

- 接纳前 reader fail closed；提交后 Provider、runtime 与 ChildSettings 共享已接纳策略和存在值脱敏。
- Execute + 有效 FullAccess 对安全现存文档内缺失的可选 ref 放行；Ask/Plan、Execute ReadOnly/Confirm/Auto、失效 grant 拒绝。
- 必需 Provider/MCP 失败（包括与可选 ref 同名）仍拒绝；真实存储缺失/损坏/不安全不被隐私 fingerprint 初始化掩盖。
- reader 在准备阶段可通过另一 Store 写同一 DB，证明未嵌套持有写事务；若改变权限或线程身份，最终复核拒绝且运行 reader 未激活。
- pause/pending/失效 interrupted authority 在读凭据前拒绝。现有 post-pause 正向对照及全部断言不削弱。
- 父 reader 不接受不同主 run 或不同 delegated parent；同一不可变父权限合法继承。

这些是 draft 分支开发辅助 Rust 测试；合并前固定源码 SHA/实际云端结果并清除测试及独占接线，最终 required CI 必须明确报告 no Rust tests、0 发现/0 执行。本地不编译、不执行 Vega 或测试；不访问真实配置、密钥或原生进程。

## 验证状态

实现及七项开发回归保存在 `38c99f38164c10dab616dd157cd60b6c4e45a32e`；非写入者对该提交相对 `21342c5107ccabdcc8f0c5638d981127733cd1b7` 的 14 文件增量审查无发现。回归使用真实临时文件 Store、keystore、pipeline、Provider guard、runtime reader 和 ChildSettings；不执行真实 Provider 请求、MCP 服务或原生进程。本地仅执行 Rust 1.98.0 rustfmt 与 diff-whitespace 静态检查。

该 head 的 [run 40421469864](https://github.com/puzige/vega/actions/runs/40421469864) 在编译阶段失败：Clippy `121916683817`、Nextest `121916684229`、汇总 `121917714624` 均失败；`pipeline.rs:784,1380` 两个 E0308 来自回滚 helper 接收 `Option<&CommittedReceipt>`，调用却提供 `Option<&Arc<CommittedReceipt>>`。测试发现/执行计数未取得，不能写成测试通过或 no Rust tests。修正两个调用为 `as_deref()`，不改变回滚语义、权限逻辑或测试断言；修正提交 `e877e7e76f589ecac00b07ce17f04c2ba62c7bfc` 的增量非写入者审查无发现；其 [run 40422933356](https://github.com/puzige/vega/actions/runs/40422933356) 已成功（Clippy `121921526673`、Nextest `121921526851`、汇总 `121922962155`）。实际日志发现 77 项／21 binaries，执行 77、通过 77、跳过 0；七项新凭据回归、两项真实 GPUI 导航及历史交接保留回归均明确通过。该完整 SHA 保存了可恢复的原始开发测试，不是最终 no Rust tests 交付 head，也不构成原生或全 D4 验收。基线 `21342c5107ccabdcc8f0c5638d981127733cd1b7` 的 required CI 已成功（70 执行/通过、0 跳过），不验证后续差异。

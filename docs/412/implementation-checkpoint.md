# 会话式 Shell 实施检查点（未完成，禁止合并）

## 第二轮安全增量（当前待复核）

- UI 非写入者对 `a353ab927dfc8f211a52c3f41cf1c26e54f83614` 关闭原 1 P1/3 P2，无新增发现；原生验收限制不变。安全复核仍发现首次自动收尾调用一次无预算 discover、重新出现别名的准入/恢复范围不一致，未批准。
- 本轮将首次自动收尾也接入共同截止时间；新增首次扫描→耗尽预算→settled/TERM/KILL 全序列观察计数回归。store 的恢复/命令投影范围与 runtime 一致，词法或当前可解析别名重叠均可见；恢复 epoch 汇总重叠范围的持久世代，解除仍逐项校验原 operation 与资源状态，并精确按原 operation/workspace 删除和推进受影响世代，不批量清除。
- 别名回归覆盖原目录缺失、重新出现 symlink、原 blocker/命令可见、追加观察使旧确认失败、原决定精确读回、重放不清除新 blocker。没有改变冻结合同或已接受风险。
- 同一官方 1.98.0 工具链与 `-j 2`，五包命令同下：3+14+8+19+3=47 运行/47 通过/0 失败/0 忽略，doctest 0；完整 workspace/all-target strict Clippy、fmt、diff 退出 0。上一 head 三组 Python 5/15/40 通过；本增量不改 Python。待取得最新精确版本独审/云端，开发测试尚未清理，未合并。

## 安全与 UI 首轮增量修复（历史版本，结论见上）

- 安全独审对 `689b2abb7402ba0c4e1805e176956207549c8193` 确认下节 3 项 P1 已解决；新增 2 项 P2：历史工作区不可访问阻断全局启动、预算耗尽后仍发起进程观察。本轮保留历史绝对规范路径 blocker，不跳过隔离；路径重新出现为别名时也比较可解析身份。共同截止时间贯穿扫描、匹配与 settled，每个外部观察前检查；不承诺中断已在途 OS syscall。
- UI 独审对同一旧 head 返回 1 项 P1、3 项 P2。本轮关闭面板保留已提交但未知的同一确认身份；首次读取失败提供就地重新检查；按 operation/call/thread 关联持久命令终态，原始工具回执和输出不改写；恢复行展示安全命令及具体原因/下一步，移除不存在的详情入口提示。
- 已同步 master `4b604cb8c2a8c33ae8e5ed3a1ec9f941bbd19b01`，只解决迁移登记冲突，保留主线 0060/0061，Shell 迁移顺延 `0062_shell_operations.sql`。冻结合同 blob 仍为 `de5da7970c4686b392c4ef784d679b642abd83d9`，与用户冻结提交一致。
- 本轮实际工具链：官方 Cargo `1.98.0 (797e8a9bc 2026-08-05)`、rustc `1.98.0 (88d9e12ae 2026-08-18)`。命令 `rustup run 1.98.0 cargo test --locked -j 2 -p vega_ui -p vega_conversation -p vega_store -p vega_tools -p vega_runtime`：3+14+7+18+3=45 运行/45 通过/0 失败/0 忽略，doctest 0 执行。`rustup run 1.98.0 cargo clippy --workspace --all-targets --locked -j 2 -- -D warnings`、`cargo fmt --all -- --check`、`git diff --check` 均退出 0。验证对应本节所在提交的实现及开发测试内容；文档更新不改行为。
- 开发过程失败如实记录：预算计数回归预期错写 2 次枚举，实际调用链为 3 次；保留“到期后观察计数不再增加”断言后校正。新 UI 回归耗时预期错写 4s，既有 formatter 对 4200ms 为 4.2s，核对未改生产 formatter 后修正。SQLite u64 读取、身份字符串比较及测试模块位置的编译/lint 失败均修责任层，无 lint 豁免。一次 120s UI 编译命令超时、结果未取得；确认无残留进程后拆开以 600s 时限取得 Clippy 与真实测试结果。
- 原生 UI/Tab/保守焦点、像素外观、真实系统集成未执行，不把纯状态测试当作这些验收。最终 exact-head 独审、云端开发验证、测试可恢复证据留存及清理、最终无 Rust 测试 head required CI 仍待完成；未合并，Issue 保持开放。

## 独审原三项 P1 修复增量（已在旧 head 复核）

非写入者对 `f9b5375d4bac2cbffd841fe44725e003ca480c4b` 返回 3 项 P1，不予通过。本轮逐项修复：

1. 启动恢复先把全部持久化 blocker 注册到 runtime 协调域，未进入 A 项目也不能从 B 的外部根或 Full Access 绕过 A。
2. 用户风险决定按原 `decision_id`、workspace、epoch 和选中 operation 集合精确读回；提交回执未知保留确认，不一律映射为 Changed。只解除原选中理由，新的 blocker 保留。
3. 未启动失败先建立内存持久化阻塞，仅在终态可靠保存且资源确认收束后解除；tools 返回未启动失败 session 而不是丢弃 owner。未启动失败保留无退出码/无 command_result 的事实，同时标明进程从未启动已收束。

当前增量验证：官方工具链同下，`cargo test --locked -j 2 -p vega_runtime -p vega_store -p vega_conversation -p vega_tools` 实际 3+13+6+18=40 运行/40 通过/0 失败/0 忽略；doctest 0 执行。四包 `cargo clippy --locked -j 2 --all-targets -- -D warnings` 退出 0，fmt/diff 检查通过。新增未启动失败资源/丢失确认门禁回归、全局 blocker 跨根准入回归、未启动终态事实回归；既有决定重放回归增加精确读回与错误内容拒绝断言。此处是开发验证，不代表全部合同已验收或独审通过。

## 继续交付：身份与预算增量

基于 `f9b5375d4bac2cbffd841fe44725e003ca480c4b`（Actions `42193732645` required CI 全通过、28 条开发测试通过）继续。修改前 fetch 并 fast-forward 核对任务分支，未覆盖用户改动。最新项目政策解除旧本地验证禁令；下文“未本地编译/测试”仅描述历史轮次。

- 本轮范围：tools 进程观察/信号适配层、共同清理预算、对应 9 条新开发回归。二次信号验证或组观察中的已知身份失配永久撤销该 PID 的权限；不会在同次清理或重试中重新采用。每个信号前核对共同截止时间，观察耗时用尽预算后不再发信号；重试宽限等待不超剩余预算。
- 实际工具链：官方 `rustc 1.98.0 (88d9e12ae 2026-08-18)`、`cargo 1.98.0 (797e8a9bc 2026-08-05)`，使用 `rustup run 1.98.0 cargo`；不安装、不升级、不改机器配置，构建并发 `-j 2`。
- 身份预算提交 `ee163bb9e21109ecdb968f83511c9470d2a1d147` 的实际验证：`cargo test -p vega_tools --locked -j 2`，17 运行/17 通过/0 失败/0 忽略；`cargo clippy -p vega_tools --all-targets --locked -j 2 -- -D warnings` 退出 0；`cargo test --locked -j 2 -p vega_store -p vega_runtime -p vega_conversation`，6+11+3=20 运行/20 通过/0 失败/0 忽略。合计 37 条。doctest 均 0 执行，不称行为测试通过。`cargo fmt --all`、`git diff --check` 退出 0。
- 新回归覆盖二次身份变化、组观察变化、复用后恢复旧 birth、已知后代重挂、未知组成员、观察故障、128 身份上限、耗尽预算及已过期预算。注入测试不证明消除了 OS TOCTOU 或隐藏逃逸后代；未运行 Vega、打包或用户数据库操作。
- 持久化/恢复首轮独审发现 3 项 P1，修复见上；tools 增量和 P1 修复仍需复核。完整合同验收矩阵、开发测试留痕清理、最终无 Rust 测试 head CI、合并和用户手测仍未完成。禁止以本轮 37 条绿色代替剩余交付门禁。

## 当前实施状态（冻结 v1.1 后）

用户已冻结 v1.1，以 `6c0cb43e3db3cd7d0e4368acb8915a1af73fa9f2` 的[合同与用户流程](best-effort-shell-contract-review.md)授权恢复代码实现。下文旧平台强保证阻塞只作历史，不再作为本轮启用前提；本轮不声称消除了已接受的 best-effort 残余风险。

- 当前 master 基线：`d4beacac2f01d4c09af7545163393e9d43753fe9`；Shell 迁移已顺延为 `0060_shell_operations.sql`，保留 master 的 0059。任务分支 `feat/412-shell-design`。早期实现与文档已重放，冻结合同内容未改。
- 已写入、部分开发回归已执行，完整合同尚未验证：有界进程身份跟踪与清理 owner；输出/reader 未收束所有权；独立持久化 blocker、精确终态提交读回、用户处置幂等；重启只读资源复查；双向写范围准入；恢复前使旧文件读取失效。
- 本轮静态修正：启动 blocker 查询缺失、旧 fixture 字段/API 不匹配、未解决终态占用运行槽、共享内存 blocker 误解除风险，以及 reader 超时使用 abort 的路径。保留原断言，不将这些静态修正称为行为通过。
- 四个相关开发测试文件保留本卡开发回归（含 macOS 条件入口）；源码入口数量不是 Actions 发现或执行计数。覆盖存储身份/回执/历史输出缺口、写范围/容量/独立 blocker、输出游标/脱敏/reader 与读取世代；尚不足以证明 BE01–BE26 与 legacy A–N 全部通过。
- 本轮 `cargo fmt --all -- --check`、`git diff --check` 退出 0。未本地编译、Clippy、运行 Vega、执行 Rust/Python 测试、进程实验或打包。
- UI 委派中断且无交付，已释放写入权，主 Agent 接回。第一版可信恢复面板已写入：会话外显暂停入口、后台事件路由、三种恢复运行状态、绑定稳定确认的风险接受、中英文案与键盘按钮；第六轮已完成编译，尚未完成原生验证。已增加严格会话回执投影及 running/finishing/stopping/终态工具卡展示；本轮补齐当前 owner 的可信单命令取消、活动刷新、Escape 返回、稳定返回焦点与保守确认焦点。完整恢复详情、剩余生命周期与故障验证仍待完成；键盘/像素未做原生验收。
- **未完成、禁止合并** 的 draft 实现 [PR #906](https://github.com/puzige/vega/pull/906) 已创建。首轮开发 head `7c1d3cfd133ed543ff68001dcd6679f9c6206a87` 的 [Actions run 42182204652](https://github.com/puzige/vega/actions/runs/42182204652) 失败：Clippy job `127774165953` 的死代码/type-complexity；Nextest job `127774166356` 在编译阶段因缺少 `Value` 导入、越层引用未声明 `rusqlite` 失败。未进入测试发现/执行，计数未取得；不是行为测试失败。已按原始日志修导入、复用现有 SQL 参数/错误类型、删除无用 setter、提取类型别名；断言与门禁未放宽，修正版待新轮 Actions。
- 第二轮 head `d2fcd6e98e22b1fc4e090e33b7713a1b52522a56` 的 [Actions run 42184044882](https://github.com/puzige/vega/actions/runs/42184044882) 仍在编译/Clippy 阶段失败：Nextest job `127780047120` 的应用退出调用缺少 `shutdown_shell_sessions` 导出与实现；Clippy job `127780046832` 的 `collapsible_if`、`SessionInput` 可见性。首轮导入/依赖错误未重现；仍无测试执行计数。已补应用级弱 owner 登记、先冻结准入后有界收束及导出，修正 lint，待新轮验证。
- 第三轮 head `52cc03af4e8397ec42cca6d67b755d755e93f052` 的 [Actions run 42186233940](https://github.com/puzige/vega/actions/runs/42186233940) 失败：Clippy job `127787128780` 为存储恢复函数多余借用；Nextest job `127787128416` 在编译阶段报告投影 fixture 缺 description 与四处 API 入参错误。未进入测试发现/执行，计数未取得。按真实 API 修接线、去掉多余借用，未改断言。另补清理重试 future 被取消时归还 tracker/child/temp/reader 的 guard 与开发回归；静态检查通过，行为待下一轮 Actions。
- 第四轮 head `511909d16f55c42b09c5723ad50c80359a8f4b51` 的 [Actions run 42188897854](https://github.com/puzige/vega/actions/runs/42188897854)：Nextest job `127795753656`、Clippy job `127795754229` 均因活动 owner 查询对 String 错用 as_deref 编译失败，测试发现/执行计数未取得。已按真实字段修正，不修改断言。本轮增量另修活 child/reader/资源未收束时继续保留执行槽，以及按 operation 身份同步/解除持久化内存隔离，补相应容量与独立 blocker 回归；待新轮 Actions。
- 第五轮 head `6f601401f23a24e2fa8e2fbd54e34cd1dcb72477` 的 [Actions run 42190284466](https://github.com/puzige/vega/actions/runs/42190284466) 仍未执行行为测试：Nextest 编译因 UI 刷新字段缺 GPUI Task 类型，Clippy 报活动 owner 返回类型复杂。已改完整类型和命名别名，不添加 lint 豁免、不改断言，待新轮 Actions。
- 第六轮 head `78b7f49aa2beb5f867c3bd42ada393f851615530` 的 [Actions run 42191340187](https://github.com/puzige/vega/actions/runs/42191340187)：Nextest job `127803834191` 首次真实运行 28 条（16 binaries），26 通过 / 2 失败 / 0 跳过。严格投影回归揭示输出字节数可超过游标跨度的产品缺口，已加跨度验证；恢复幂等回归实际保留正确 blocker 集合，仅因无关顺序失败，改为排序后精确比较完整集合。Clippy job `127803834450` 的启动参数过多与 fixture 默认值后赋值已用执行上下文/结构体初始化修正，无 lint 豁免。修正版重放到新 master，无冲突且 range-diff 八提交均等价；需新轮验证。
- 第七轮 head `aca915f9f5f80224f235bbc35dc0e5b9df63001f` 的 [Actions run 42192666817](https://github.com/puzige/vega/actions/runs/42192666817)：Nextest job `127808226258` 真实运行 28 条（16 binaries），28 通过 / 0 失败 / 0 跳过。Clippy job `127808225893` 仍因恢复 Action/Outcome 中确认值大枚举失败，required check 未通过；用 Box 间接保存同一确认值，未改恢复语义。再次同步 master 的 NSH 预览更新，无冲突；新 head 须重新验证，旧行为成功不替代新版本完整门禁。
- 两次只读独审代理均在无产物/无结论时中断并释放；首次恢复请求被工具以代理状态拒绝。不继续循环重建代理，需可用的非实现者审查环境，不能自审替代。实现独立审查未取得；旧文档独审不代表实现通过。最终交付前必须保留可恢复开发测试证据、清理 Rust 测试及独占支持，再取得最终 head 的 required CI。
- 当前未合并、未发布应用、未修改用户运行时数据库；本卡保持 In progress。

## 历史状态（已被上文授权与合同取代）

2026-11-04 更新：用户已接受 best-effort 剩余风险，授权先整理[合同与用户流程正式评审稿](best-effort-shell-contract-review.md)，本轮明确暂不改代码。原 §5.3 全后代收尾保证拟由新稿列明条款替代，但新稿尚未冻结，未授权本轮恢复实现或启用能力。下文保留原检查点、当时阻塞及验证记录，不将已接受的风险取舍误报为平台强保证已验证。

用户已确认最小设计并授权实施；原设计中“待确认/仅设计”的阶段描述不是当前授权状态。行为合同仍采用设计提交 `f28d8e2ab21869a71f30a7ed0dc959fb196d8035` 的 [规格](vega-long-command-waiting-design.md)，本记录不放宽验收或安全边界。

本检查点保存未完成的实现，不是可用功能，不是独立审查批准，不是 CI 通过。未建立实现 PR、未合并、未发布或修改用户数据库。分支保持 `feat/412-shell-design`。

## 已写入但未验证的产物

- `vega_tools`：显式 wait_ms、进程 owner、输出窗口/cursor、观察与取消；保留缺省阻塞协议。
- `vega_runtime`：wait/cancel schema 与路由、句柄注册表、工作区互斥、持久化 hook、run 收尾。
- `vega_store` / `vega_conversation`：追加 operation 迁移、审计身份、恢复未知结果与权限接线。
- 应用关闭 barrier 接线；开发辅助回归源码共 11 个测试入口（静态数，不是发现或执行数）。

待完成：UI/历史真实状态投影、scoped 流式脱敏、steering 后审计身份、启动清理失败隔离、生命周期/持久化故障回归、格式整理、云端编译和验证、完整非实现者独立审查。开发 Rust 测试必须在实际验证和固定证据后依仓库规则清理，不能现在删掉以规避失败。

## 阻塞：规格 §5.3 进程控制启用前提未满足

以下为静态证据，不是已执行的逃逸试验：

1. `sandbox.rs` 的 BASE_PROFILE 允许 process-fork、process-exec 及 self process-info-setcontrol；启动通过 process_group(0) 创建原进程组。现有代码未提供后代不可脱离原进程组的保证。
2. `bash/mod.rs` 的 settle_exited_group / confirm_group_stopped / wait_group_gone 只确认原 PGID；会话退出、取消或超时路径据此可能得到 cleanup_safe=true。子进程回收和 reader 排空不能证明已关闭输出并离开原组的后代不再写工作区。
3. `agent/shell_sessions.rs` 的 process_prerequisite 只检查 `/bin/kill`、`sandbox-exec` 可执行和当前 PID 的 kill -0，不能建立支持命令的后代收尾保证；Reservation::finish 在 cleanup_safe=true 后释放互斥。
4. 因此，现有代码不能支持“安全收尾后自动放行冲突写入”的启用结论。仅延长等待、按 PID 重绑、提示模型别后台化、或把进程组消失等同全部后代退出均不是修复。

依冻结规格 §5.3，不能满足支持命令的安全收尾时不得开放会话模式。当前没有可在既定边界内证明满足该前提的机制；新增 daemon、全系统进程管理或缩窄支持命令集合均不能擅自纳入本次实现。需要先明确平台安全机制与允许的验证路径，必要的产品/范围变化须用户裁决。恢复前保留检查点，不安装/启用它。

## 实际验证及版本身份

- 已 fetch；ff-only 更新结果为 Already up to date。设计 HEAD 为上述完整 SHA，远端 master 为 `037031f3352a87fe8f0e546aa652e1251d39f8a1`。
- `git diff --check` 退出 0。
- 只读 rustfmt --check 报告格式差异，未通过；输出被截取，不能以管道最后命令的退出 0 冒充格式通过。
- 未本地编译、Clippy、运行 Vega 或执行测试；未取得云端编译/测试结果。
- 新的独立安全核查代理启动失败，没有审查结论；不计作独立审查。进程 owner 补充阶段同样失败且无未知副作用记录，原实现保留；无活跃写入者。
- 文档写入前的完整未提交实现快照摘要（tracked binary diff 加按 git ls-files 顺序拼接的 untracked 文件）为 SHA-256 `0fe7584483feba45711eab5dae677c03f71df0cc206fcb05eb9504f8a5a01e71`。它不是 Git commit，也不是远端可访问证明。
- 关键文件 SHA-256：
  - `crates/vega_tools/src/bash/mod.rs`: `0d520a372fc1c33d588bb4b70c5b9c3b8f91329c54b976ad3f21e7d7caa3f875`
  - `crates/vega_tools/src/bash/session.rs`: `8ca44a8daf7008fabc82c3239351d04f8b791191a94293d51182549915e8d2ba`
  - `crates/vega_tools/src/sandbox.rs`: `54fbfe3cf018d20689e48c35b31f768b46bfdd7e2b29a7630a12214f43700256`
  - `crates/vega_runtime/src/agent/shell_sessions.rs`: `dfced68c9c9b3ee9c3cd2cd51fe9c5e1fb10be493b85ab035cfb4cfac1e07411`

## 恢复条件

先解决上述平台安全前提，保持不确定结果隔离与“不重放”合同；不能仅靠普通 CI 绿色认定 OS 边界已证明。批准的边界及验证路径确定后，从本检查点继续，不重建或丢弃现有产物。随后补完集成和必要回归，在 Actions 执行开发验证，固定源码与实际结果；完成非实现者独立审查、测试清理及最终 head required CI 后才可合并。

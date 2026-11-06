# 会话式 Shell 实施检查点（未完成，禁止合并）

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

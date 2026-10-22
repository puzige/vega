# 无效读取修复：开发证据与检查点

## 当前阶段

- Issue：[#536](https://github.com/puzige/vega/issues/536)，In progress，尚未交付。
- 生产基线：`e4f18c656aa69d192d095a4338a2108e6c9a3c2c`。
- 分支：`feat/536-invalid-read-validation`。
- 冻结验收：[v1 规格](vega-issue-536-invalid-read-validation.md)。RED 前原始 SHA-256 `5351d774a9e3be7dc4b81d2c69e21b21b2ef2e1a5f20e7535c0e6d9057b952aa`；有效 RED 后仅补充 crate 根常量导出接线授权，当前文档 SHA-256 `2a2ee308e6b287ab7a415de8ac1938e88e0148a0fdb6044992d00b03d1aef000`，验收/数据/断言无变化。
- v1 有效 RED 与同一 13 项 GREEN 已固定；独立实现审查发现 mixed-batch P1，暂停删测/交付。v2 新增 A8 第二周期，基线 `3612a970e41b1ec1998cc400c90406c82d92897b`；规格 SHA-256 `bffec1414f97ea8635a954c1d702cbdf033604f049966217c3ac9c6e6d529a56`，原 13 项测试不变。

## 委派恢复检查点

2026-10-22 首次测试写入和只读审查执行超过 20 分钟无文件/结论产物，主 Agent 核对 Git 与工作树后确认 crates 无变动，仅存在冻结规格。两个旧 execution 均已确认 interrupted，未保留可复用测试或审查结论；没有重启已经完成的工作，也未留下并行写入者。

恢复执行保留同一生产基线、规格与全部 A1–A7 验收。独立审查收窄为最小方案可复用性；测试写入分为 A1–A4 完整链小阶段、A5–A7 授权/取消/安全回归小阶段。分阶段交付不改变验收范围：全部先行测试齐备并经独立审查及既有 Actions 有效 RED 前，禁止生产实现。

## 独立方案审查（v1）

结论：无阻塞发现，仅为局部静态可行性审查，不代表测试执行或安全验收通过。

- `tools_exec.rs` 的 InvalidRead 归一化 `{}`；授权阶段直接终态，取消仍优先原有分支。现有 validation audit 已是严格 `Deny / Validation`、空 note/danger。
- `events.rs::persist_runtime_event` 的 Rejected 终态原本就要求 pending approval，再调用 `validate_runtime_terminal` 和 `tool_calls::reject`；因此无需虚假批准、Running 或新事件类型。
- `pipeline.rs::validate_recovered_projection` 同时服务首次终态审计、下一轮历史准备和 `events.rs::validate_reused_terminal`，Read 专用严格分支可保持链路一致。
- 不可仅改 Failed→Rejected；全局 Read source 匹配不接受 Validation 拒绝，必须同时实现专用严格投影。
- 专用分支要拦住 Validation 或固定校验错误与不合法组合，不得回落通用读取。不能仅凭 `{}` 拦截，否则会误伤现有取消及真正中断恢复。
- 共享固定错误常量须包含最终输出全文的 `Tool error: ` 前缀，不能造成生成与审计不一致。

审查时三文件读取前后 SHA-256 相同：

| 文件 | SHA-256 |
|---|---|
| `crates/vega_runtime/src/agent/tools_exec.rs` | `b0c23bb1099c0c535b003da09e6dbb3812f05548ca1854b60de4a7b2ecd827eb` |
| `crates/vega_conversation/src/agent/events.rs` | `2401b2d44020a7bc3c2c04b238ff1ab6cfded866ee9916a59fe094de085a980e` |
| `crates/vega_conversation/src/agent/pipeline.rs` | `47e8a7942b8bfdef3cf2ddcea95011a0010af07a99002f2c09b489a528aa5e82` |

## 先行测试小阶段产物

均为未提交的开发测试，尚未触发 Actions，不是有效 RED/GREEN 证据。

| 停写版本 | 范围与内容 | SHA-256 |
|---|---|---|
| A7 首版 | `read_validation_audit_tests.rs`，7 个函数；真实恢复投影/终态/remember 校验，无 mock 或数据库。覆盖严格阳性、16 个单不变量负例、4 类取消/recovery 阳性及固定错误冒用、其他工具、非法状态转移、truncated/remember | `6dd4f24bc36583e54a6d51b760571442b8a6b42bbcfb5f641efd8942f6146ff5` |
| A7 接线 | `agent/pipeline.rs` 仅底部 4 行测试模块接线；生产逻辑仍原基线 | `96b9f72846ef7d1a5f74a6a77d6f01dc987207aaf97d52981115789ba0c05ce3` |
| 完整链最小版（后续扩充中，非最终冻结） | `read_validation_tests.rs`，1 个函数参数化 read/Read，真实 conversation→runtime→persist；无效读取后正常读取并回答；provider 实际收到错误/内容，验证持久化与无审批运行 | `465236bbb20d96620a863174cd53cf2619be7a0b99b66e204140a72efddc86e1` |
| 完整链接线 | `agent/mod.rs` 仅测试模块接线 | `e1a49e270badc77030284076f5c34093867e6807c4539262092ed9fa337087de` |

后续仍在生产未修改的同一基线上扩充，全部 writer 已停写。当前完整先行内容为 **13 个 Rust 测试函数**（源码清点，不是 CI 发现/执行数）：

| 范围 | 精确停写版本 SHA-256 | 证据与审查状态 |
|---|---|---|
| A1–A4 `read_validation_tests.rs` | `6ddeead5f68de6c4ffdf6b691549a86345fe662a7951398a90b788dae2cc5f9b` | 2 个函数；两 alias × 10 类输入及真 `.git` 预检拒绝共 22 场景；A4 两 alias 均含同轮重复 ID、drop/reopen/recovery、history、下一轮首请求历史及跨轮 ID 复用。独立只读审查无阻塞，批准进入首轮 draft PR。 |
| A5–A6 `read_permission_tests.rs` | `d211ae797f0ac17516e2df332a0774245d72b171939ac81e3456da13417e7dc7` | 3 个函数；正常项目内与 FullAccess 外部读取、Confirm 批准/拒绝、授权前与执行前取消。独立静态审查无阻塞。 |
| A6 `read_recovery_tests.rs` | `d71dc1cf2b5c52b9eef11bfa87d2629c7f78a58df60667ad6f4139aa93e76fb8` | 1 个函数；真实 pending/approved/running 转移、重开 file-backed SQLite、恢复及二次恢复幂等。独立静态审查无阻塞。 |
| A7 `read_validation_audit_tests.rs` | `6dd4f24bc36583e54a6d51b760571442b8a6b42bbcfb5f641efd8942f6146ff5` | 7 个函数，首版未改；独立静态审查无阻塞，可并入首轮补丁。 |

A5–A6 的 runtime 测试直接构造已解析的 prepared call，再调用真实授权与执行，因此证明授权/执行边界，不冒充 preparation/persist 整链；recovery 测试是真实 store 行为。A7 是实际安全校验器单元证据，不冒充完整写库链。完整链由 A1–A4 承担。测试只使用 owned 文件和文件型 SQLite，脚本化 provider/用户决定；Full access fixture 使用实际 `full_access` 值，`Store::open` 后显式 migrate。

A1–A4 的 provider 观测同时支持当前 Tool 消息与 User 内 `vega_history_observation/tool_result` 投影，因为无效读取没有可重放模型参数；固定输出断言保留，不将模型原始输入历史当作工具泄漏。所有上述测试与接线无新增依赖、不修改生产行为、workflow 或恢复 SQL。

主 Agent 在全部停写版本上执行 `rustfmt --check --edition 2024 --config skip_children=true`（4 个新增测试及 4 个接线文件）、`git diff --check`，均退出 0；已读回所有测试并核对实际 API/hash。提交前再 fetch/ff-only 同步，远端和本地仍为原基线。尚未编译/运行，函数数不冒充发现或执行数。

## 首轮测试审查结论

A1–A7 精确停写版本均经非写入者独立静态审查，无阻塞，允许创建 draft PR 取得有效 RED。当前基线静态预计 2 个完整链函数与 3 个 A7 函数失败，另 8 个通过；这不是运行结果，不能写成实际 RED/PASS。参数化循环首个失败即退出，首轮 RED 也不能声称 22 场景全部执行。

主 Agent 独立脚本核对 4 处既有文件原始 bytes 加唯一测试接线与当前文件完全相同；无生产逻辑、依赖、schema 或 CI 变化。已审查的原始测试内容在有效 RED 后冻结，后续不按实现输出反推断言。

## 有效 RED（生产未改）

- Draft PR：[#541](https://github.com/puzige/vega/pull/541)。先行测试完整提交 `78097943411dfae1f012c71379bf8a25e2c2ed50`，父生产基线 `e4f18c656aa69d192d095a4338a2108e6c9a3c2c`。
- [原始测试 patch](https://github.com/puzige/vega/commit/78097943411dfae1f012c71379bf8a25e2c2ed50.patch) 与四个测试文件的该提交源码链接已固定在 [PR 证据评论](https://github.com/puzige/vega/pull/541#issuecomment-6339048781)，不依赖本机临时文件或 artifact。
- [Actions 39766783686](https://github.com/puzige/vega/actions/runs/39766783686)，[Nextest job 119784103641](https://github.com/puzige/vega/actions/runs/39766783686/job/119784103641)。2026-10-22 UTC 10:56 启动，11:01 按 5 分钟节奏查询并读取原始失败；无自动重试。
- `cargo nextest list --workspace --message-format json` 成功，`cargo nextest run --workspace` 实际运行 **13 tests across 15 binaries：8 passed、5 failed、0 skipped**。ignored 未另取得计数，源码无 ignore。两项完整链均在首个无效调用失败，不能声称首轮所有参数场景均执行。
- 完整链原始失败：`Runtime(RuntimeFailure(RuntimeFailureDetail { stage: ToolTerminal, reason: ApprovalAudit, sqlite_code: None, sqlite_extended_code: None, io_kind: None }))`。两项分别是整轮继续和 reopen/reuse 场景，失败位置均为真实 conversation 入口返回。
- A7 原始失败：严格 Validation 拒绝阳性为 `CorruptRow(... invalid safe projection)`；pending→Rejected 阳性为审批审计失败；取消/恢复冒用负例为 `Cancelled/ReadonlyTool claimed read validation output`。后者证明固定错误声明也须严格限制，不能回落通用读取。
- 其他 8 项通过，含真实授权/取消/recovery 与已有伪造负例。Clippy job（fmt、workspace all-targets deny warnings）成功，三组 Python 检查成功；required check 因目标测试 RED 失败，未用作合并许可。

失败归类：产品结果语义/审计投影缺陷，不是测试接线、无关编译或环境失败。有效 RED 已确认，冻结同一 13 项源码、数据和断言后委派唯一生产 writer，范围为规格 v1 最小三文件加原共享常量的 crate 根显式导出；该必要接线经主 Agent 静态复核后追加授权，不改变验收；不补测、不改预期、不 ignore、不重试换绿。

## 最小实现停写版本

唯一生产 writer 在有效 RED 后落实规格，4 文件内容相对先行测试提交的补丁 SHA-256 为 `8c2b21f8f4969f96e344f846f61cbc6530d6e85a7988e94b1f4d0341bf8e63ad`（5,513 bytes，+33/-16）：

- `vega_runtime/src/agent/mod.rs` 增加固定错误全文常量，`src/lib.rs` 只显式导出该常量并格式换行。
- `tools_exec.rs::invalid_read_result` 改用 `terminal_result` 返回 Rejected + validation audit，空执行元数据。
- `vega_conversation/src/agent/pipeline.rs` 添加只限 read/Read 的严格声明分支：Validation 来源或固定错误触发，必须同时为 `{}`、原错误全文、Rejected/Deny/Validation、空 note/danger/exit/duration；不匹配直接 corrupt，不能回落通用读取。不单凭 `{}` 触发，保留取消和真实中断恢复。
- 不改 loop/events/global approval source/state machine/recovery SQL，不伪造 approval，不加依赖/迁移或界面文案。

writer READY 后停止全部写入；主 Agent 读回完整差异、核对补丁哈希和四份冻结测试 hash 仍完全相同。writer 的 4 文件 rustfmt 与 diff 静态检查均退出 0；GREEN 和独立实现审查待现有 Actions/只读 reviewer。

## v1 GREEN 与独立审查阻塞

实现完整提交 `3612a970e41b1ec1998cc400c90406c82d92897b`，[Actions 39768987383](https://github.com/puzige/vega/actions/runs/39768987383) / [Nextest job 119791459564](https://github.com/puzige/vega/actions/runs/39768987383/job/119791459564)，真实 **13 discovered / 13 executed / 13 passed / 0 skipped**。fmt、all-targets Clippy、三组 Python 及 required check 全部成功。四份测试与 RED 提交逐字节一致，无重试或断言修改；Green 日志和源码留存 [PR 评论](https://github.com/puzige/vega/pull/541#issuecomment-6339548791)。

独立 reviewer 对同一停写实现发现 **P1，未批准**：新的 Read 所有 Validation 来源守卫遮蔽原有 mixed question/Skill 批次拒绝分支。主 Agent 复核 `loop_.rs` 两处合法 `Rejected/Deny/Validation` 固定 mixed 输出、`pipeline.rs::valid_mixed_skill_rejection_input` 和旧投影分支，确认是产品兼容回归而非环境或观测故障。它影响首次保存和下一轮准备；不能用当前 13 项绿色合并。

v2 明确新增 A8 遗漏边界，保留全部原测试，不删测。第二周期在上述 v1 实现基线上先写混合批次真实链/两输出投影与安全负例，独立审查、现有 Actions 新 RED 后再最小修正优先级。没有补测追认、伪造批准或放宽全局校验；新增提交保留两轮时序，不 amend/force-push 抹去证据。

## v2 A8 先行测试与观测修正

第二周期基线保持 `3612a970e41b1ec1998cc400c90406c82d92897b`，生产 P1 未修正。新增 `read_mixed_validation_tests.rs` 与 `agent/mod.rs` 的 3 行测试模块接线，原四份测试/13 函数和所有生产文件逐字节不变，无新增依赖。A8 共 4 函数：

1. mixed-question 真实 conversation 链，两 alias × 合法对象/`{}`，含继续回答、重开 recovery/history、下一轮准备及同 ID 复用。
2. 两种原 fixed mixed 输出 × 两 alias × 两输入共 8 组，真实 persist/终态校验/恢复投影/reused 边界阳性；Skill 只声明此层证据，不冒充完整 Skill 执行。
3. 上述 8 组 × 14 个单字段伪造负例，验证 projection/terminal/persist 拒绝且 pending 状态不变。
4. note/danger 负例验证实际严格 codec 编解码及 persist 拒绝，状态不变。

首个停写版 818 行 SHA-256 `400365f3d914bfa626ca8d45ef0676ae373c4ed2a03717caf28999d019004c27`。独立审查发现 P2 观测故障：合法对象跨轮复用保留原生 Tool 消息，production ProgressReporter 会把 runtime notice 追加在工具 output 后；旧 helper 将整个消息等同固定错误，会在 P1 修正后产生无关失败。主 Agent 复查 provider/progress/loop 原始实现确认，归类为测试观察层，不是验收变化。

仅修 helper 后的新停写版 828 行 SHA-256 `a717a92c6c1b9dc6b86b234ebbfa302b74475a0e883dacff46179305f16c0c16`：native Tool 仅剥离精确完整的唯一 Progress notice suffix，剩余 output 仍全文相等；observation 的 advisory 若存在也须精确匹配（null/错误类型/其他内容不接受）。不截任意尾部、不改脚本/可见文本/合法输入/复用场景或安全断言。修前后 delta +11/-1，SHA-256 `d13b2ff641afc6f4b9127e9142bb10c0da38d67a01f656bb28d1556b81fd0659`；原 13 项仍未改。

writer 已停写，主 Agent 读回 helper 并验证其常量与实际 provider 输出一致；rustfmt/diff 静态检查退出 0。独立 reviewer 已复审该精确 delta，确认 P2 消除、无断言弱化并批准整个 A8 先行补丁进入 Actions；A8 尚无有效 RED，禁止生产修正。

## 待执行门禁

1. v2 A8 新先行测试、独立审查及有效 RED；原 13 项继续保持。
2. 最小修正后同一原 13+A8 GREEN，与独立实现审查。
3. 带全部同一测试 GREEN 和审查未通过前不删测。
4. 在 PR 固定原测试可访问源码/patch、完整 SHA 及 RED/GREEN run/job。
5. 独立删测提交、生产差异/独占支持清理审查、最终 head `no Rust tests` 完整门禁。
6. 正式 PR 与合并；Issue 保持开放、In review 等用户 Native 手测。

本地未运行 Vega、编译、构建、Clippy、任何测试或打包；不修改用户数据库、运行配置或 CI workflow。

# 无效读取修复：开发证据与检查点

## 当前阶段

- Issue：[#536](https://github.com/puzige/vega/issues/536)，In progress，尚未交付。
- 生产基线：`e4f18c656aa69d192d095a4338a2108e6c9a3c2c`。
- 分支：`feat/536-invalid-read-validation`。
- 冻结契约：[v1 规格](vega-issue-536-invalid-read-validation.md)，SHA-256 `5351d774a9e3be7dc4b81d2c69e21b21b2ef2e1a5f20e7535c0e6d9057b952aa`。
- 生产尚未修改；先行测试仍在编写，未获得 Actions RED/GREEN，不将静态结论称为测试通过。

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

## 待执行门禁

1. 先行测试已齐备并完成独立静态审查；正式执行结果仍待 Actions。
2. draft PR 上现有 Actions 有效 RED；记录原始失败、准确发现/执行计数和完整提交 SHA。
3. 最小实现，同一冻结测试云端 GREEN 与独立实现审查。
4. 在 PR 固定原测试可访问源码/patch、完整 SHA 及 RED/GREEN run/job。
5. 独立删测提交、生产差异/独占支持清理审查、最终 head `no Rust tests` 完整门禁。
6. 正式 PR 与合并；Issue 保持开放、In review 等用户 Native 手测。

本地未运行 Vega、编译、构建、Clippy、任何测试或打包；不修改用户数据库、运行配置或 CI workflow。

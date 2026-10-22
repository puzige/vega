# 无效读取的安全终态与整轮继续

- Issue：[#536](https://github.com/puzige/vega/issues/536)
- 规格版本：v1，2026-10-22；先行测试与独立审查前冻结验收，生产尚未修改。
- 生产基线：`e4f18c656aa69d192d095a4338a2108e6c9a3c2c`（亦为 v0.1.195）。
- 分支：`feat/536-invalid-read-validation`。

## 事实、未知与边界

已有 issue 证据表明先发生 `tool_terminal / approval_audit`，随后恢复将 pending 调用改写为审批未完成。当前基线静态核对与 issue 调用链一致；本轮尚无运行复现，不能将静态诊断冒充测试结果。原始模型输入未保留，持久化 `{}` 是运行层归一化结果，不代表模型原本提交空对象。

本卡仅修复 Native `read` / `Read` 输入校验或路径预检失败的结果语义、持久化与重放一致性。不修改用户数据、配置、恢复 SQL、权限策略、数据库 schema，不重构全工具。路径预检可解析路径元数据；“未访问文件”在本卡指不进入文件内容读取、没有内容结果或读取凭据，不要求取消路径安全检查。

## 验收契约（先于实现计划）

| ID | 前置与动作 | 必须观察的结果 | 自动证据层级 |
|---|---|---|---|
| A1 | Execute + Full access，provider 提交无效 `read` 或 `Read`，随后继续回答 | 单工具安全可纠正错误；整轮不失败、不抛 `RuntimeApprovalAudit`；provider 下一请求收到该错误，仍可执行正确读取并完成回答 | 真实 runtime → conversation 事件持久化；仅 provider 外部边界脚本化 |
| A2 | 缺路径、路径非字符串、畸形 JSON、非对象、offset 为 0/负数/错误类型、limit 非整数、双路径字段、未支持字段 | 校验前终止内容读取，不调用权限 hook，不产生 Approved/Running 或虚假成功；安全参数仍归一化 `{}`，不回显原始输入、敏感路径、额外字段或文件内容 | A1 的参数表与真实存储审计 |
| A3 | 合法形状但路径预检失败（使用 owned 临时目录下 `.git` 受保护路径等确定性围栏拒绝） | 与 A1 同样的安全终态，无审批，无路径/内容泄漏；覆盖 `read` / `Read` | 实际 `Tools::file_path` 与持久化链 |
| A4 | 无效调用完成后重新打开历史、执行 startup recovery、准备下一轮；重复读取同一调用记录 | 记录保持原终态、错误与严格审计，无 pending 残留，不被恢复为“审批未完成”；历史和下一轮 provider 的工具结果一致；同调用 ID 的合法复用不新增审批/执行 | file-backed owned SQLite、history、下一轮真实准备链；同轮重复 ID |
| A5 | 正常项目内读取；Full access 外部读取；需询问的外部读取分别批准与拒绝 | 正常内容与成功终态；外部读取仍遵守原有授权/拒绝规则及准确审计，不借无效读取通路绕过权限 | 真实工具与事件/存储；权限 hook 仅提供用户决定 |
| A6 | 执行前取消，以及真实中断的 pending、approved、running 调用后恢复 | 取消不执行内容读取；已有取消与 recovery 拒绝/取消规则保持，不能把真正中断冒充校验错误 | runtime 边界与现有 recovery |
| A7 | 伪造无效读取终态/审计：非空输入、成功/Failed 状态、错误输出、批准决定、错误来源、note/danger、执行元数据；不合法状态转移 | runtime 持久化及恢复投影仍 fail closed；其他工具不得因本卡获得通用豁免；禁止补写 approval 伪造执行 | 实际 persist/validate_recovered_projection 安全边界负例 |

表中多输入可由少量行为测试参数化覆盖，不为计数或内部字段制造重复测试。外部依赖仅隔离 provider/用户决定，不 mock 被测工具决策、授权、状态机或存储。测试不得依赖用户数据库、凭据、网络或真实 shell/外部应用。真实 Native UI 和模型集成由用户合并后手测，不添加自动真实 E2E。

## 根因与最小实现计划

1. `crates/vega_runtime/src/agent/tools_exec.rs::prepare_call` 的 Read 分支在输入/路径预检失败后生成 `InvalidRead`，只保留 `{}`。
2. `invalid_read_result` 当前借 `failed_tool_result` 返回 `Failed + deny/validation`。`authorize_call` 直接给出终态；`loop_.rs` 已发 Proposed，但不发 Approved/Running。
3. `crates/vega_conversation/src/agent/events.rs::persist_runtime_event` 对非 Rejected 终态要求已持久化 approval；`validate_runtime_terminal` 还要求来自 running。pending 记录因此触发审计失败，中断整轮。
4. `pipeline.rs::validate_recovered_projection` 的 Read 审计也不接受该组合。之后 `vega_store::recovery` 只是在收尾，将遗留 pending 转成 recovery 拒绝，掩盖原始错误。

采用现有 **Proposed → Rejected** 安全拒绝通路，而非假造批准或增设执行阶段：

- 无效 Read 保持 `{}` 安全参数和固定、无原始输入的可纠正错误；结果改为 `Rejected + deny/validation`，所有执行元数据、remember rule、note/danger 为空。
- 将该固定错误定义为 runtime 共享常量（沿用当前文字），使生成与审计不漂移；不新增面向用户的说明或改变正常结果。
- 在恢复投影中增加仅限 `read` / `Read` 的严格校验拒绝分支：工具名、安全 `{}`、固定输出、Rejected、Deny、Validation 及空 note/danger/执行元数据必须同时匹配。校验拒绝声明或固定错误被用于其他组合时拒绝，不能落回宽松读取分支。
- 复用现有 `validate_runtime_terminal`、pending→reject 存储转移与通用事件投影，不放宽全局状态机、不改全局 approval source 匹配、不新增 `ToolCallValidationRejected` 输入投影类型。
- 取消仍走现有 timeout 拒绝；真正中断调用仍走原 recovery，不迁移/修复既有用户记录。

允许生产范围：`crates/vega_runtime/src/agent/{mod.rs,tools_exec.rs}` 与 `crates/vega_conversation/src/agent/pipeline.rs`。2026-10-22 有效 RED 后静态确认 runtime 的 `agent` 模块私有且 crate 根使用显式 re-export；补充允许 `crates/vega_runtime/src/lib.rs` 仅在既有 `pub use agent::{…}` 增加上述共享错误常量导出。这是原共享常量方案必要接线，不改变 v1 验收、数据、测试或安全语义，不新增行为 API。其他文件仍须先说明必要性并审查，不顺手扩大重构。

## 可行测试入口与接线

- 完整链入口：`vega_conversation::agent::run_thread_task_with_permission_sink` / crate 内 `run_thread_task_with_permission_config`，以及 `vega_runtime::run_agent_with_permission_sink`。`Provider::chat_stream` 可由进程内 scripted provider 实现，捕获 `ChatRequest` 并返回 `ProviderEvent` 流，无须新增生产 public API。
- 真实 event/store 入口：`agent::events::persist_runtime_event`、`validate_runtime_terminal`、`agent::pipeline::validate_recovered_projection`；crate 内测试可访问，不需要扩展 public API。
- 使用现有 `Store::open`、projects/threads/messages 构造 owned file-backed SQLite；`Tools::new` 使用 owned 临时目录。生产 conversation 入口会在准备阶段 reopen store 并恢复，故不能只用独立内存连接假装完整链。
- 历史入口：`history::latest_history_page`；下一轮真实任务入口会执行 prepare/recovery 并投影已持久化终态。需断言 provider 实际收到的工具结果，不只检查独立纯函数。
- 无效输入不能只测试 `invalid_read_result`，阳性正常读取和安全负例也必须在实现前写齐。新增 Rust 测试/独占依赖仅开发保留，不恢复历史测试集合。

## 冻结、验证与交付

遵守 `AGENTS.md`、[执行总纲 §7.2](../vega-exec-guide.md#72-test-first-与有效测试当前规则) 与 [Rust 测试生命周期](../vega-rust-test-delivery-lifecycle.md)。

1. 专职子 Agent 在生产未改基线上先写齐 A1–A7 测试与必要接线，READY 时固定内容标识并停写；主 Agent 组织非写入者独立审查规格/测试。
2. 主 Agent 提交规格和测试、创建 draft PR，现有 `pr-check` Actions 运行。有效 RED 必须来自目标行为缺失，不以无关编译/依赖/runner 错误冒充；保存原始失败及准确计数。
3. 冻结目标、数据、预期与测试源码；有效 RED 后才委派最小生产实现。同一冻结测试与最新 head 必须云端 GREEN；实现后不补测、不改预期、不 ignore、不重试换绿。接线错误先留证独立审查；验收变化先记录并重开周期。
4. 在 PR 固定原始测试可访问源码/patch、完整 SHA、RED/GREEN run/job 与准确发现/执行计数；独立审查同一停写实现版本。
5. GREEN 后独立提交删除 Rust 测试及独占接线/依赖，保留生产、Python、benchmark、通用 CI。审查删测生产差异和残留，最终 head 的完整门禁必须为 `no Rust tests`（0 发现/0 执行），不能冒充带测试 GREEN。
6. 最新 head required check 通过并独立审查无缺口后，转正式 PR、squash merge；Issue 保持开放，置 In review 等用户手测。不得自动关闭。

本地仅快速静态检查（如 `git diff --check`、格式检查）；不运行 Vega、cargo check、Clippy、任何测试或打包，不改现有 workflow。CI 启动后遵守 5 分钟状态查询间隔，不短轮询/watch。回滚可回退本卡合并提交；无需数据迁移。

## 证据状态

- v1：基线、调用链与实际 API 已静态复核；验收/实现计划已落文档。
- 先行测试、独立审查、Actions RED/GREEN、删测与最终门禁：待执行，尚未标为通过。
- 用户 Native 手测：待合并后进行。

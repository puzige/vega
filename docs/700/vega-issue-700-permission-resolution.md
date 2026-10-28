# Issue #700：Full access 危险确认与权限请求终态

## 当前契约与问题证据

2026-10-28 用户确认的权限边界：有效 Full access 自动批准普通工具权限请求，但始终保留危险命令确认。危险确认必须是一项单独、明确的人工决定；危险命令不能因 Full access、Auto 或普通精确 allow 规则而自动获准。一个危险调用只展示并等待一次危险确认，不再追加普通权限确认。未经批准的命令绝不执行，也不自动重试。

检查 `crates/vega_runtime/src/permission.rs`、`crates/vega_runtime/src/agent/tools_exec.rs`：判定顺序是 danger、ReadOnly 门禁、精确规则、Auto/FullAccess、Confirm。命中危险规则时，runtime 直接发出带危险事实的确认请求，响应成为该调用的最终审批；普通 Full access 分支直接批准而不请求普通权限卡。现有代码已经符合以上核心判定边界。本卡不更改危险分类、审批顺序、Full access 沙箱、授权/reset/fence，也不把危险命令加入普通 allow 规则。

截图对应的强推工具调用在本机只读审计中记录为 `git-force-push`、`source=timeout`、运行约 21 秒，输出 `Tool error: permission denied`。同一会话另一次强推在危险确认获准后成功。记录能证明前一次没有获准执行，但不能证明当时是等待超时、运行取消还是确认界面/队列关闭。

实际缺口在审批结果表达：`PermissionDecision::Timeout` 与 `RuntimeUserDecision::Timeout` 同时代表真实等待超时、父运行取消、确认卡或队列销毁、队列通知不可用及部分异常路径；所有这些最终都被保存为 Timeout 拒绝，并通常输出 `Tool error: permission denied`。明确的用户拒绝与这些非用户决定也无法被模型和日志正确区分。

## 目标

在保留 fail-closed 行为和既有持久审批兼容性的前提下，分辨并准确传递危险确认及其他 UI 权限请求的终态，让审计、历史恢复和模型输出不会把用户取消、界面关闭或真实超时伪装成用户拒绝。发生拒绝或请求失效时，工具调用必须以未执行的终态结束。

## 必须满足的行为

1. Execute + 有效 Full access 下，既有普通 Bash/Read/Write/Edit 审批仍自动批准并记录 FullAccess。受限权限、Ask/Plan、ReadOnly、失效/重置的授权、取消及其他 capability 门禁不受影响。
2. 命中危险规则时，危险检测先于普通 allow/Auto/Full access。用户明确放行一次后只产生一项危险审批并执行原有准备好的调用；用户明确拒绝、真实超时、父运行取消、确认 UI/队列不可用或卡片被关闭时都不执行、不自动重试。
3. 至少保留以下不同的决策来源与安全输出：
   - 用户明确拒绝：记录为用户决定，输出说明请求被明确拒绝，必要时保留拒绝备注。
   - 确认等待达到运行时截止时间或卡片自身截止时间：记录为超时，输出说明命令未执行、确认已超时及可再次发起操作。
   - 当前 run 被取消：记录为取消，输出说明命令未执行且 run 已取消。
   - 确认卡/队列/所属 UI 生命周期结束、请求通知或响应通道失效，且不是运行取消：记录为确认不可用/关闭，输出说明命令未执行；不声称用户拒绝或等待超时。
4. 原有普通拒绝保持拒绝语义；无效输入、运行模式禁止、批准后工具实际执行错误必须继续使用各自的验证/运行模式/工具失败结果，不能伪装为权限拒绝。
5. UI 收到拒绝终态后准确移除对应确认卡；不重放 prompt、不重复执行命令。取消、超时、通道关闭的 first-wins latch 和当前权限卡/工具调用身份匹配继续有效。
6. 扩展后的审计 source JSON 必须被 live event validation、审批 codec、数据库持久化、消息历史恢复与工具结果投影一致接受；旧版合法审批记录继续可读。新增的拒绝来源只可用于 Rejected 且未执行的调用，不能作为成功或运行中授权。
7. 普通 MCP/ACP/Computer Use 的授权契约、Bash 单次范围扩围、权限 reset、授权 fence、危险检测规则及操作系统沙箱边界不随本卡改变。为共享权限结果类型补充穷尽分支时必须保留各自现有语义。

## 实施范围

先为 `docs/700/` 中本规格版本留存后再改代码。预期最小影响范围：

- `crates/vega_conversation/src/types/permission.rs`：共享权限响应与持久审批 source 的可区分终态，以及严格 codec / runtime 映射。
- `crates/vega_conversation/src/agent/permission_queue.rs`：区分真实取消、请求 UI 关闭/队列销毁/通知不可用与显式超时；普通危险卡和普通权限卡仍复用同一 single-card UI。
- `crates/vega_runtime/src/permission.rs`、`crates/vega_runtime/src/agent/tools_exec.rs`：传递失败类别，生成准确拒绝审计和不执行输出；保留 danger 优先顺序。
- `crates/vega_conversation/src/agent/pipeline.rs`、相关事件/恢复校验：接受仅用于未执行拒绝的新增审计来源，并继续拒绝不可能的 source/status/output 组合。
- 仅在存在必要用户可见提示时更新 `crates/vega_i18n/locales/ui.en.toml` 与 `ui.zh-CN.toml`；辅助文案应说明真实结果与下一步，不重复状态条已表达的信息。

不更改 `vega_tools::danger`、bash 启动策略、普通 Full access 自动批准逻辑或独立外部工具权限策略。若实现调查发现普通 Full access 的第二次确认确实来自另一条与上述调用链无关的审批，先记录证据并确认是否属于 #700；不得静默放宽其他授权边界。

## 按风险验证

- 必要的开发期 Rust 回归由 Actions 执行，验证运行时权限决策及持久审计/恢复的可观察结果：普通 Full access 无审批；Full access/Auto/精确规则不能越过危险确认；危险一次确认后成功只经过一个确认；拒绝/超时/取消/卡片或队列关闭保持互相可区分且均不执行；历史 source/output 不匹配被拒绝。
- 本地只允许格式、差异空白及静态检查；不运行 Vega、不编译、不构建或测试。
- 如创建开发 Rust 测试，保留其精确源文件/patch、完整 SHA 和 Actions 结果供交接；合入前依仓库规则移除所有 Rust 测试及独占辅助接线，最新 head 由完整 required CI 确认 `no Rust tests`（0 发现/0 执行）。
- 实现就绪后停写固定版本，交非实现者独立审查；PR 最新 head 的 required check 必须通过。真实桌面确认 UI 和用户工作流由用户在合并后的 master 手测，未通过前 Issue 开放、不进入 Done。

## 当前风险与未知

- 本机记录不能还原 21 秒 `source=timeout` 的具体关闭路径；实现应让未来事件可区分，不得倒推本次一定由设置、切换线程或取消触发。
- 共享权限 source JSON 是持久协议的一部分。新增值须对历史兼容且在所有 live/recovery/projection 验证路径保持严格，不可只改模型可见字符串。
- 权限界面生命周期跨 thread 切换、打开设置、run cancellation、队列 listener drop 与真实 timer；确认“取消”和“UI 不可用”的 first-wins 来源时不得产生陈旧 UI 卡、复活请求或误放行。

变更记录：2026-10-28 初版，根据用户澄清将 Full access 普通免审批与危险命令不可绕过的一次确认固定为边界；将实现焦点收窄为权限终态归因与未执行反馈。

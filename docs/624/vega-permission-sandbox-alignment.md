# #624 · 权限与沙箱语义对齐

## 状态与授权

- 版本：D4，2026-10-26；阶段：实施中，尚未交付。用户在查看范围、风险与回滚限制后明确要求“按 D4 开始实现”，批准仓库代码修改及云端验证；不包含本机运行配置、真实凭据或 OS 权限修改。此前用户要求跳过 D4 独立设计审查，在途审查已取消，未取得最终结论；本次实施授权不取消最终独立实现审查、required CI 和用户手测。
- 关联：[Issue #624](https://github.com/puzige/vega/issues/624)、[#58](https://github.com/puzige/vega/issues/58)、[#378](https://github.com/puzige/vega/issues/378)、[#619](https://github.com/puzige/vega/issues/619)。
- D4 设计代码基线：`e481cd28d5de685dc79f39080a8c6cb79b9412b8`；当前实施已正常集成至 `e47afcba49592f333fdc25e508f0eb5e60e4883b` 并追加基础提交 `decccd52a675de7dbe8ebba0ffbea8285eadd512`；任务分支：`feat/624-permission-alignment`。D3 基线为 `410758d36e8fbe0802f816227948ece1486d8494`；本轮先以 `580ae08acd43942875968cd8db3cac5ed0fbf1fe` 合入 `d87957c8b68a49a033bb5a8b81ee71fedd57c0ca`，再以 `2c8481710b89a749adcab0b5fec99642e0828091` 正常合入当前基线，未重写历史。新增的模型分组、预测、运行中消息操作、Provider 失败分类和 reasoning 修复均为既有上游内容，不是本卡生产实现。
- 用户先选择 **P2：更全面开放**，补充“全面对齐 ChatGPT”，随后明确批准按已发布 D4（`e76c7d6ee4baf932cffaa2dbadf3e1e9bf6af227`）开始实现。P1/P3 不再作为候选；本机配置和真实凭据操作仍不在授权内。
- “全面对齐”覆盖 Full access、受限模式、审批、凭证、界面、迁移及恢复委派；以可验证的公开 Codex 契约作参照，不宣称复刻闭源 ChatGPT App，不把只完成 Full access 当作整卡完成。用户进一步选择 **正常重启保留授权**、**同卡完成受限模式对齐**；本版据此提出的完整范围现已获得上述实施确认。
- 本轮用户明确允许 #624 超过通常的 3 个提交，以保留已发布 D1–D3 证据、正常同步基线并追加必要提交；不重写、不强推历史，最终 PR 仍 squash。该例外只适用于本卡提交记录，不是生产权限授权。
- 仓库规则：本地不运行 Vega、不编译或测试；自动检查交现有 PR Actions，真实平台集成交用户手测。Rust 开发测试如使用，必须在合并前留痕并清除，不恢复历史测试集合。

## 目标、范围与非目标

让审批、实际 OS 隔离、权限名称及风险提示一致。Ask/Plan/Execute 是任务能力门禁，ReadOnly/Confirm/Auto/FullAccess 是审批与执行策略，两者不机械映射为 Codex 三个模式。

本轮已获 D4 实施授权，范围同时包含 P2、版本化持久授权、只读 Bash、受限默认禁网与单次扩围，按依赖顺序实现但在同卡验收，不将受限工作推迟成未获批准的另卡。受限策略是下文明确提出的新产品契约，不在 P2 名义下暗中改变。

不改凭据存储后端、不迁 Keychain、不重写 Broker 的认证/目的地规则、不撤销 MCP 独立保护和具名工具入口检查、不关闭浏览器内层沙箱。允许进入实施确认范围的是 Bash 的 Broker 启动时机、受限执行策略、独立扩围授权及必要接线；这不批准当前 Agent 修改本机配置/凭据、读取真实 secret 验证或接管日常浏览器。

## 已核实的 Vega 调用链

以下均为当前基线静态阅读，不是运行结果。

1. `vega::agent_context::protect_project_tools`：只要有配置位置，就建立 `ToolCredentialPolicy`；绑定列表为空也不例外。
2. `vega_conversation::agent::pipeline` 创建 `RuntimeToolConfig`，映射线程任务模式及权限模式。
3. `vega_runtime::agent::tools_exec` 准备 Bash，`full_access` 只来自 runtime 配置；执行时分别调用 `execute_bash` / `execute_bash_full_access`，不能由工具 JSON 选择。
4. `vega_tools::bash::execute_bash_inner`：Full access 且有绑定时，以 `CredentialProtectionRequired` 在进程及代理启动前拒绝；否则跳过 `SandboxConfig`。
5. `execute_bash_with_temp` 仍根据 `credential_policy` 建立 `ShellCredentialContext`；零绑定只是不启动租约，不会消除该上下文。
6. `vega_tools::sandbox::spawn_full_access_shell`：有上述上下文时使用 `sandbox-exec`，附加配置根读写 deny、process-info 限制及 taskgated deny；无上下文才直接启动 shell。
7. Composer 仍显示“不使用沙箱，危险操作仍需确认”。因此常规应用配置下，Full access 的声明与实际边界不一致。
8. `vega_credentials::ProtectedProcess` 是 MCP 等进程的独立路径，不由 Bash 的 `full_access` 标志控制，不应顺带删除保护。
9. Bash 之前还有统一凭据读取：`vega::app_agent` 的 `broker_credential_reader` 收集全部 binding key refs，任一 `get_key` 失败即传播错误；`owner_credential_reader` 将其与 Provider/MCP 凭据合并；`vega_conversation::agent::pipeline` 在构造运行前读取，失败为 `CredentialCheckUnavailable`。因此仅改 Bash 分支仍无法满足 A3 的缺失绑定 key 场景。
10. `vega_store::keystore::get_key` 的 `Missing` 同时覆盖 key 不存在、根/凭据目录缺失；`platform::read` 还把凭据文件缺失转换为空文档。不能用 `Err(Missing) => 忽略` 代替严格缺失分类。
11. `vega_store::migrate` 在 `user_version >= MIGRATIONS.len()` 时直接返回；任意旧 binary 不认识新授权，也不必留下可检测标志。新 marker 无法普遍检测手动降级/恢复旧数据，D4 明确修正 D3 对任意降级往返的过强保证。
12. `PermissionRequest`、runtime target/prompt 与项目 `permissions` 精确规则均无沙箱范围。`PermissionQueue` 已有 first-wins latch、取消和消失转 Timeout；新请求替换旧请求也使旧请求失效。可以复用终态机制，不能把旧 Once/Always 解释为网络/目录授权。
13. 自动化 claim 目前没有权限快照，凭据续跑仅绑定 source/raw-user fingerprint，委派 authority JSON 仅含旧 mode/rules。必须沿实际创建、排队、恢复与派发链传递新 fence，而不只在 Composer 添加确认框。

可复核文件：`crates/vega/src/agent_context.rs`、`crates/vega_conversation/src/agent/pipeline.rs`、`crates/vega_runtime/src/agent/tools_exec.rs`、`crates/vega_runtime/src/permission.rs`、`crates/vega_tools/src/bash/mod.rs`、`crates/vega_tools/src/sandbox.rs`、`crates/vega_tools/src/credentials.rs`、`crates/vega_credentials/src/lib.rs`、`crates/vega_ui/src/conversation_stream/composer_actions.rs`。

## 当前行为矩阵

表中网络指 Vega 工具层规则，仍受 OS、外部策略及网络环境限制。“可读”不等于所有文件都能读取，仍受当前用户权限和 TCC 等限制。

| 任务模式 / 权限 | 普通 Bash 审批 | 无绑定，有应用配置 | 有绑定 | 网络边界 |
|---|---|---|---|---|
| Ask / 任意权限 | 能力门禁拒绝 Bash | 不启动 | 不启动 | 无子进程 |
| Plan / 任意权限 | 能力门禁拒绝 Bash | 不启动 | 不启动 | 无子进程 |
| Execute / ReadOnly | 拒绝 Bash，审批规则不覆盖拒绝 | 不启动 | 不启动 | 无子进程 |
| Execute / Confirm | 普通命令确认或精确规则，危险规则优先 | 项目/私有临时目录可写；Git 与配置根受保护 | 同前，另用受控环境和 Broker | 无绑定时 profile 放行网络；有租约时只到该代理，CONNECT 校验绑定 HTTPS 主机/端口；认证注入另受路径与替身匹配约束 |
| Execute / Auto | 普通命令自动批准；危险规则仍优先 | 与 Confirm 相同的 Seatbelt | 同 Confirm | 同 Confirm |
| Execute / FullAccess | 普通命令自动批准；危险规则仍优先 | 项目外与 Git 可写，但仍有配置根/进程保护 Seatbelt | 启动前返回 `credential_protection_required`；runtime 当前仅返回错误码，不能记为已有可操作切换指引 | 无绑定时未施加网络 deny；有绑定时不启动 |

补充：没有应用配置上下文的工具实例，FullAccess 已直接启动 shell；不能以这个特殊入口证明常规应用行为。read/glob/grep/write/edit 还有各自路径、身份、审批和凭证检查；取消 Bash 沙箱不等于撤销这些工具自身的检查，也不保证 Bash 无法访问同一数据。

“零绑定”只指工具上下文中的 `credential_bindings` 列表为空，不代表 Provider 等真实凭据不存在。非空绑定即使引用密钥缺失、当前命令不使用凭据，现行 Full access 仍拒绝。当前判断依赖工具上下文快照，不代表配置新增绑定会立即终止既有运行。

## 对既有 Codex 评论的证据收敛

[#624 的补充评论](https://github.com/puzige/vega/issues/624#issuecomment-6406088426) 保留为历史证据，不作为完整运行证明：

- 二进制字符串只能证明包含实现片段，不能证明开关默认值或当前生效策略；配置文件没有某键，也不能排除打包默认、managed requirements 或其他配置层。
- 早期未固定的上游 `main` 不作为冻结依据。新增公开源码参照固定为 `970111f6cb62cc02fe36a82ea7d71b37a2889c97`，仍未证明与安装的 `0.164.0-alpha.11.1` 相同，也不是完整 ChatGPT App 源码。
- “shell 可读凭证文件”在无额外隔离、同 UID、有普通读取权限的前提下是权限推论，未运行读取真实凭据的试验；不得标作 observed。
- 输出脱敏是降低泄漏风险的补充措施，不保证覆盖任意 shell 输出、编码、旁路或直接网络外传。
- 检索位置没有 `.age` 文件不能证明加密存储未启用；未核实完整目录、配置及运行后端，不作该结论。
- Root:Read 与 Vega 默认读取都不是凭证隔离承诺，不能忽略 Vega 的显式凭证根 deny 而宣称二者在文件读取上等价。Claude Code 的完整可读根也未在本轮核实。
- 不将“未找到 GUI Mach 名称”写成 Brave 必然不能启动；也不以网络片段推出所有模式仅能 loopback。

### 固定公开源码能支持的结论

以下链接均绑定上述 Codex SHA；属于静态证据，不是本机生效策略或上游测试通过记录：

| 项目 | 可支持结论 | 证据与不可推出的结论 |
|---|---|---|
| 沙箱选择 | 普通 Unrestricted、网络 enabled、无 managed-network 强制且 Auto 偏好时选择 `SandboxType::None` | [选择器](https://github.com/openai/codex/blob/970111f6cb62cc02fe36a82ea7d71b37a2889c97/codex-rs/sandboxing/src/manager.rs#L313-L353)、[条件链](https://github.com/openai/codex/blob/970111f6cb62cc02fe36a82ea7d71b37a2889c97/codex-rs/sandboxing/src/policy_transforms.rs#L646-L666)。managed network、网络关闭、Require 偏好及外层 OS 限制不能省略 |
| CLI 与 MCP 凭据 | 基础 CLI auth store 默认 File；MCP OAuth 默认 Auto，优先 Keyring、不可用时使用 File | [两个枚举](https://github.com/openai/codex/blob/970111f6cb62cc02fe36a82ea7d71b37a2889c97/codex-rs/config/src/types.rs#L133-L163)。不是 ChatGPT 打包/管理配置最终值；不能称 MCP 天生更弱 |
| 文件权限 | auth 文件保存使用 Unix 创建模式 `0600` | [保存函数](https://github.com/openai/codex/blob/970111f6cb62cc02fe36a82ea7d71b37a2889c97/codex-rs/login/src/auth/storage.rs#L227-L244)。不等于修复既有文件权限，也不隔离同 UID shell |
| 环境 | TOML 转换默认 All、`ignore_default_excludes=true`；执行环境另有六个内部名称的最终剔除和可配置过滤 | [默认值](https://github.com/openai/codex/blob/970111f6cb62cc02fe36a82ea7d71b37a2889c97/codex-rs/config/src/shell_environment_policy.rs#L133-L153)、[具名剔除](https://github.com/openai/codex/blob/970111f6cb62cc02fe36a82ea7d71b37a2889c97/codex-rs/protocol/src/shell_environment.rs#L14-L27)、[过滤顺序](https://github.com/openai/codex/blob/970111f6cb62cc02fe36a82ea7d71b37a2889c97/codex-rs/protocol/src/shell_environment.rs#L139-L172)。不能概括为所有保护只有六个变量，更不是文件隔离 |
| managed deny-read | 保留 managed denied reads 的更新 helper 明确跳过 DangerFullAccess / ExternalSandbox | [helper](https://github.com/openai/codex/blob/970111f6cb62cc02fe36a82ea7d71b37a2889c97/codex-rs/core/src/config/mod.rs#L552-L574)。未追完初始配置、受约束 setter 和允许的模式/profile，不能声称管理员限制普遍可绕过 |
| 受限读取 | 内置 read-only/workspace-write 构造器以 Root:Read 为基础，不是仅项目可读 | [read-only](https://github.com/openai/codex/blob/970111f6cb62cc02fe36a82ea7d71b37a2889c97/codex-rs/protocol/src/permissions.rs#L606-L623)、[workspace-write](https://github.com/openai/codex/blob/970111f6cb62cc02fe36a82ea7d71b37a2889c97/codex-rs/protocol/src/permissions.rs#L819-L835)。最终仍受自定义/managed deny、OS/TCC 等约束，未实测读取 auth 文件 |
| 分层凭据保护 | 存在 credential broker、Keyring、加密存储及 Ephemeral 等不同机制 | [broker 实现目录](https://github.com/openai/codex/tree/970111f6cb62cc02fe36a82ea7d71b37a2889c97/codex-rs/network-proxy/src/credential_broker)、[存储实现](https://github.com/openai/codex/blob/970111f6cb62cc02fe36a82ea7d71b37a2889c97/codex-rs/login/src/auth/storage.rs)。存在不证明默认启用、本机生效或完整迁移/回退行为；不以局部搜索宣称“没有 Broker/只有 0600” |

审批与沙箱是独立维度；`danger-full-access + never` 不等于所有工具、App/MCP 都无审核，也不等于 Vega 的 Auto 是 Codex auto-review。公开[权限文档](https://developers.openai.com/codex/sandboxing.md)及[审批安全文档](https://developers.openai.com/codex/agent-approvals-security.md)是文档参照，不是该 SHA 或本机二进制行为证明。

本轮没有证据承诺“新出现的任意同 UID 可读明文凭据会自动隔离”；也不据此证明全产品不存在任何新凭据处理。冲突的是无沙箱同 UID shell 与这种文件不可读保证，不是 Full access 与所有形式的凭据保护都不能共存。不得再写“Codex 从来没有该保护”“唯一安全方式是容器”等未经证明的全称结论。

## 验收用例（先于实现计划）

以下为 D4 待确认设计预期，不是运行结果或实施批准。均使用合成凭据、独立项目和可恢复标记。相对 [D3 已审版本](https://github.com/puzige/vega/blob/aae1d34001f21c9756b28630f6343fbc4fd5fc26/docs/624/vega-permission-sandbox-alignment.md)，明确修改 A2/A4/A12 的受限策略和 A13 的回滚保证，并增加 A15–A21；不是删掉回归或按实现结果改预期。所有行为验收均未运行。

| ID | 前置 / 操作 | 可观察结果 | 验证路线 / 当前状态 |
|---|---|---|---|
| A1 | Execute + 新契约已授权 Full access；零绑定但有合成凭据文件；包含正常配置和无配置工具实例 | Bash 不经过 Vega Seatbelt；不创建 Broker/保护上下文；对合成文件的访问仅受 OS/外层策略约束，不能写成凭据不可读 | 启动分支审查 + CI 必要回归 + 用户手测；未运行 |
| A2 | Execute 下分别选 Confirm / Auto，运行项目写入、项目外写入、Git/配置根写入及联网命令；有/无绑定均覆盖 | 普通审批仅决定执行；默认项目/私有临时目录可写，其余写入拒绝；Git/配置根保护保留。默认不联网、不创建 Bash Broker；单次扩围按 A15/A16 执行，有绑定时不绕过代理目的地规则 | 策略/接线静态审查 + 必要 CI + 用户 owned-fixture 手测；未运行 |
| A3 | 已授权 Full access，Provider/MCP 必需凭据正常、绑定语法有效；分别引用存在/缺失的合成 binding key、使用无关命令；排队期间增加/删除有效绑定 | 绑定本身不拒绝 Bash；不启动受限 Broker、不调用其 SecretReader、不创建租约/CA/别名环境、不直接注入真值；不自动删绑定、切模式或加沙箱。配置错误和授权失配仍按各自原因阻断 | Bash 策略与实际接线审查 + CI 必要回归；未运行，不沿用 P1“非空即拒绝”断言 |
| A4 | Ask / Plan 的四种权限选择，以及 Execute + ReadOnly；读取、重定向写入、解释器/子进程写入、网络、危险命令；存在旧 Once/Always | 提供 OS 约束的只读 Bash，普通读取可执行，工作区/临时目录写入和网络拒绝；不依命令文本猜只读。Ask/Plan 不因 Full access 选择解除约束，目录写扩围拒绝，网络仅可单次确认；危险匹配仍拒绝，旧规则不能扩大能力 | 能力决策/沙箱接线审查 + 必要 CI + 用户正反例手测；未运行 |
| A5 | 危险命令、取消、超时、scope mismatch、临时目录身份异常 | 危险门禁、审计、进程组收拢、输出上限和安全清理不回退；不将现有收拢机制夸称为能追回所有脱离后台进程 | 静态审查 + CI 必要回归 + 用户进程手测；未运行 |
| A6 | 旧 Full access 线程/默认值、缺失/未知授权；排队、重启恢复、自动化、fork、子 Agent followup/resume | 旧记录不自动获得 P2；新持久授权正常重启后仍有效，但不自动续跑。恢复同时匹配新旧任务身份及授权 fence，不从后来设置补权；普通 fork 保持 Confirm；自动化须有自身任务身份绑定的确认 | 存储/恢复/runtime CI + 用户手测；未运行 |
| A7 | 运行前选 P2，运行中改为 Confirm；P2 曾修改合成安全配置 | 活动 run 的执行快照不热切换，后续新 run 使用受限策略；需要立即停止则用 Stop。切回不恢复配置可信性、不撤销外部效果或后台进程 | 快照/取消审查 + CI 必要回归 + 用户手测；未运行 |
| A8 | 鼠标/键盘选择与取消 P2；旧同名模式重新授权；保存失败、重开线程、默认设置 | 风险在确认前可见；取消/持久化失败不生效，重选旧同名模式有入口；授权范围与生效时点清楚；有绑定不再显示 P1 冲突拒绝 | UI/服务契约审查 + 必要 CI + 用户手测；未运行 |
| A9 | Full access 下的模型工具说明、结果、日志；read/glob/grep/write/edit 与 MCP | 具名工具检查与 MCP 独立保护保留；现有脱敏保留但不承诺覆盖编码/旁路；不把 Bash 声明为仍可使用受限绑定，也不因 Bash 模式删除 MCP 所需绑定 | 接线与独立安全审查 + 必要 CI；未运行 |
| A10 | 有/无绑定的 P2 改合成安全配置，后续受限 run 或其他线程读取 | 记录后续上下文可将修改后配置作为新快照的持久风险；不以快照相等、模式切回或数据库标记冒充用户批准或防同 UID 篡改 | 静态调用链 + 用户合成配置手测；信任取舍待实施授权 |
| A11 | P2 授权后新增一个合成明文凭据；再启动 Bash | 不自动扫描、搬迁、注入或转沙箱；不承诺自动隔离该凭据，绑定增减不改变 P2 路线 | 设计/启动审查 + 用户手测；未运行 |
| A12 | 从 P2 切回受限建立新 run，有有效绑定；不请求网络、批准代理联网、绑定变更/key 缺失分别覆盖 | 受限严格凭据检查保留；仅单次网络授权后创建 Broker/CA/alias，失配或不可用时拒绝，不退到直连/无沙箱/真值注入；无网络授权不创建代理能力 | CI 必要凭据回归 + 用户合成代理手测；未运行 |
| A13 | P2 spawn 失败、派发前取消、保存/解析失败；受支持的授权重置中途失败/重启，再完成回滚并升级 | 无未授权启动或自动重试放权；保存失败不展示成功。重置 pending 阻止 P2，完成后旧 grant 不复活；任意未走重置的手动降级/数据还原不作自动检测保证 | 错误/迁移审查 + 必要 CI + 用户手测；未运行 |
| A14 | 核对下方全面对齐差异表及所有批准的受限策略正反例 | 每项有固定依据、批准/保留差异、实现与验证结果；未决项不能标整卡完成；CI 或浏览器单次成功不能替代权限矩阵 | 最终规格/实现独立审查 + 用户验收；未运行 |
| A15 | Confirm/Auto/ReadOnly 的联网请求；普通 Once/Always、无响应、取消、卡片替换/消失、超时、重复 call、重启后旧响应 | 只有独立范围卡的本次批准提供网络能力；不记住、不继承、不自动重试；迟到/重复响应无效。受限默认阻断直连、loopback、Unix socket 和非白名单 Mach 中继 | 授权状态机 CI + 用户合成端点/IPC 手测；未运行 |
| A16 | 项目外专用目录扩围；目录路径/身份/命令变化、symlink/hardlink、Git 元数据/凭据根及祖先、Ask/Plan/ReadOnly | 仅 Execute + Confirm/Auto 可请求写目录；单次确认和身份复核后仅放行该范围，保留强制 deny；不可表示或不安全的根拒绝，不偷换成 Full access | 路径/权限 CI + 用户文件标记手测；未运行 |
| A17 | 线程/默认授权、同模式重授权、模式退出再进入、旧草稿、新建/已有线程、保存冲突 | mode/revision/grant 原子保存；同名重授权更换 identity；默认授权只供之后新建的线程独立派生，已有线程和旧草稿不补权；切离即撤销未来使用，再进入需确认 | 存储/配置/服务 CI + 用户 UI/重启手测；未运行 |
| A18 | 自动化任务修订、手动 followup/resume、凭据续跑、排队任务在授权改变前后 | 同一 fence 可恢复且仍遵守原启动规则；失配/旧序列化不得静默重建更宽授权。活动父 run 只传已有权限，单次扩围不传给子 Agent | 恢复/序列化 CI + 用户手测；未运行 |
| A19 | P2 的可选 binding 引用：安全文件中 key 存在/不存在；文件/目录/根缺失、unsafe 权限、hardlink、格式/IO 故障；同 ref 又为必需 Provider/MCP key | 仅安全、存在、完整解析的存储中缺少无关 key 可忽略；其余错误保留；必需 key 缺失仍失败。运行前和后续脱敏读取都覆盖，不记录真实值 | 严格读取与调用策略 CI；未运行 |
| A20 | 受限无/有绑定、只读/项目写/单次扩围的环境、子进程、实际 final profile、自检失败 | 统一受控环境，不继承用户代理/秘密环境或登录脚本；子进程保持约束。自检使用本次最终策略，失败不启动命令；`true` 成功不冒充所有边界已验证 | 接线审查 + 必要 CI + 用户平台手测；未运行 |
| A21 | 三类授权卡鼠标/键盘、Esc、焦点、窄窗口/浅深色、保存/派发失败、当前 run 与下次设置 | 不混用普通审批/扩围/持久 Full access；范围和必要风险在确认前可见，错误不折叠隐藏；取消不生效，保存失败留在原状态，活动权限不虚假更新 | UI/服务必要 CI + 用户纯手动验收；未运行 |

## 已选方向：P2 的执行契约

P1（非空绑定仍拒绝）和 P3（保留凭证 Seatbelt）仅作为 [D2 历史候选](https://github.com/puzige/vega/blob/1f98926e6ed77aac1eebef138a0a227e7461f86b/docs/624/vega-permission-sandbox-alignment.md)保留；本轮规格按 P2 展开，不继续推荐 P1。

### 模式、绑定与网络

下表是已获实施授权的 D4 整卡目标，Bash 路径尚未实现。默认仍为 Confirm；不新增一组独立模式选择器，而是在内部区分能力、普通审批、执行范围三维，并提供明确的单次扩围卡。

| 条件 | 普通 Bash | Vega OS 策略 | 凭证绑定/Broker | 网络与环境 |
|---|---|---|---|---|
| Ask / Plan，任意权限 | 新增只读 Bash；危险匹配拒绝 | ReadOnly；不因权限控件为 Full access 放宽 | 默认不建 Bash Broker；批准网络后有绑定才启用 | 默认禁网，网络可单次确认；受控环境；不能扩为可写 |
| Execute / ReadOnly | 同上，不按文本猜测只读 | ReadOnly | 同上 | 同上 |
| Execute / Confirm | 原普通确认/精确规则；危险优先 | WorkspaceWrite | 默认不建 Bash Broker；有绑定时获网络授权才建 | 默认禁网；联网/额外写目录分别单次确认；受控环境 |
| Execute / Auto | 普通命令自动批准；危险优先 | 同 Confirm | 同 Confirm | 同 Confirm，扩围不会自动批准；不是 Codex auto-review |
| Execute / 新授权 Full access | 普通自动批准；危险优先 | 不附加 Vega Seatbelt | 不因绑定拒绝 Bash；不建其 Broker/租约/CA/alias/context，不注入真值 | 不加 Vega 网络限制；保留直接 shell 环境/启动与私有临时目录，不主动修改用户代理；OS/外层约束仍在 |
| 选择 Full access 但缺失/旧/未知授权 | 不获得 P2；需用户重新授权或明确选受限 | 不静默改名/降级掩盖 | 不删绑定 | 旧数据可查看，但不得靠切任务模式再切回补出授权 |

P2 必须在创建 Broker、CA 和 `ShellCredentialContext` 之前分流。只删除 `CredentialProtectionRequired` 不满足契约，因为后续共享路径还会创建代理/上下文；只把 sandbox 参数改成 None 也不满足契约。保留整个 `Tools` 的凭据策略，避免连带取消具名文件工具检查。绑定本身格式非法、配置解析失败、Provider 不可用等仍是独立失败，不承诺 P2 绕过整个应用的就绪条件。与当前 Provider/MCP 无关的 binding key 缺失不属于这些例外，不能包装成通用“就绪失败”继续阻断 P2。统一脱敏仍可由可信 owner reader 读取实际存在的已知凭据；须区分缺失引用与凭据库/配置故障，不关闭统一脱敏、不吞全部错误、不放宽受限 Broker/MCP 的凭据检查。

Full access 下“绑定不自动用于 Bash 请求”与“凭据不可访问”是两回事。该模式不启动 Bash Broker，不表示 shell 不能直接读取同 UID 可读明文凭据，也不表示 MCP 或 Provider 停止使用自己的受信任凭据客户端。仅清空环境、输出脱敏或保留具名工具检查都不能提供这种文件隔离。

### 新凭据、配置变化与信任

- 零绑定仍可能有 Provider 等凭据；已有/新出现的同 UID 可读明文凭据都可能被无沙箱 Bash 访问或外传。不增加通用 secret 扫描，不静默移动/删除凭据，不把模式自动切回受限以维持虚假保密承诺。
- 在已获 P2 授权的同一 run 中，有效绑定增减不改变 Bash 启动路线，也不要求重新接受一次“零绑定”条件；P1 的绑定冲突检查及相关 race 预期不适用于 P2。权限契约、scope、取消与上下文身份仍须在派发时有效。
- 受限 run 的配置快照/SecretReader 一致性检查保留；变动失配时按既有错误停止凭据使用，不借 P2 自动退化为直接 shell。新建受限上下文会重读配置，因此旧快照的相等检查不证明新配置获得用户批准。
- 无沙箱 Bash 可修改共享配置及授权数据，影响后续 run、其他线程或后台进程。P2 采用当前用户级可信执行，不提供同 UID 防篡改；数据库中的契约版本只解决正常迁移/意外继承，不是针对恶意 shell 的权威边界。该持久影响仍须在实施授权中明确接受。

### 持久授权、原子更新与恢复（D4 实施契约）

以下是已批准的目标类型与行为；已实现部分和未完成项见阶段记录，不将本节整体视为实现完成。UI/服务共享产品类型置于 `vega_conversation::types`；store 使用持久化表示，runtime 经现有转换边界接受 headless 执行策略，不反向依赖 conversation/UI，不在各 crate 复制一套产品类型。

#### 数据表示与旧数据

- `FullAccessGrant`：`schema_version=1`、`contract_version=1`、不透明 `grant_id`（ULID）、`authority_epoch`、`scope`、`origin`、`issued_at`。线程 scope 为精确 thread id；默认 scope 为当前应用配置的“未来新线程”；origin 区分用户线程确认和默认授权派生（记录来源 grant id），不得伪造为逐线程点击确认。
- `PermissionFence`：`schema_version`、执行策略版本、数据库/项目/工作区身份、thread 身份、任务模式、权限模式、`permission_revision`、有效 grant identity/version/epoch/scope，外加现有 source/version/fingerprint、exact rules 等上下文约束。入队时绑定已分配的 submission/attempt 身份；尚未存在的 run id 不伪造为已运行，在 admission 建立 execution binding 后只追加该身份、不重新取权，运行和范围许可必须同时匹配 fence 与 execution binding。记录中无 secret，时间戳不是授权依据；必需字段缺失、版本未知、scope 错误或不一致均 fail closed。
- 增量 migration `0040_permission_contracts.sql`（上游已用 `0039_pending_thread_model.sql`，本卡顺延，不改已有 migration）：`threads.permission_revision INTEGER NOT NULL DEFAULT 0`、`threads.full_access_grant TEXT NULL`；单例 `permission_contract_state` 保存逻辑 `authority_epoch` 与 `reset_state`（ready/pending）；`credential_continuations.permission_fence TEXT NULL`；`run_permission_fences` 按 assistant message id 保存原运行权限快照，供失败/崩溃恢复原样引用，不能在 suspend 时从当前线程补权；`delegated_permission_fences` 以 delegated execution 身份在子任务创建事务保存接纳快照，覆盖 assistant message 创建前中断，worker 和检查点原样核对；独立 `command_scope_audits` 记范围请求身份/规范化范围/来源/终态和时间。结构化 JSON 校验与 typed decode 必须共同检查；创建 epoch 不创建任何用户 grant。schema 只增不删，重置仅修改记录值。
- `NewTaskDefaults` 拟增 `permission_revision`（缺失为 0）、`full_access_grant`（缺失为 None），同已有 `permission_mode` 一起原子保存到 config；不把一次默认授权再拆成 config+SQLite 两份必须同时成功的 grant 写入。默认 grant 绑定同一逻辑 epoch，数据库替换或配置复制不自动认作新批准。
- 迁移保留旧 `full_access` 字符串用于显示“需重新授权”，新 grant 留空；旧默认值不能生成已授权 P2 线程。无效原始记录不强行转换成 Confirm 后继续执行；允许查看/修复，运行入口提供重新授权或用户明确改选受限。其他旧模式按 D4 受限策略使用，旧 Once/Always 精确规则仍只是普通审批。

#### 授权动作、草稿和撤销

1. 线程变更走 conversation 服务及现有 admission/write coordination；以 expected revision 做 compare-and-set。在同一事务中更新 mode、revision、grant 与必要 pending/replay 失效标记，提交后才发布 UI 事件。首次确认和同名重授权都生成新 grant id、递增 revision；当前 equality guard 不能吞掉授权动作。取消、CAS 冲突或写入失败不提交新权限，UI 保留原状态及重试入口。
2. 切离 Full access 清除该线程未来授权并增加 revision；之后再切回需重新确认，不能复活旧 grant。普通模式切换也递增 revision。已开始的 run 使用不可变执行快照，不热升级或热降级；用户需要立即停止用 Stop。过期 pending 请求拒绝，而非按新 mode 重投自动启动。
3. 默认设置在独立动作中确认，只覆盖之后创建的新线程。`ConfigEdit` 在共同配置锁下检查 expected revision，将 mode/revision/grant 原子保存；磁盘失败不更新 UI。既有线程、旧草稿和既有自动化不补权。新草稿记录创建时的默认 fence；materialize 时在服务中重新核对，默认已改变则要求用户重新选择，不临时继承更宽权限。内存 `DefaultPermissionSnapshot` 保存配置路径及原始 mode/revision/grant（无效值不能被宽容解析后丢弃）；配置刷新只更新未来草稿的权限默认，不改已有草稿。普通发送、草稿自动化、分组创建共用 `DraftCreationGuard`，由外层按协调门禁 → ConfigEdit → DB 事务取得 guard，事务内复核 ready/epoch、完整默认快照及草稿身份，派生 grant 与 INSERT 一起提交。分组入口在点击时捕获草稿，不在 worker 里替换为更晚的默认。用户明确改选受限模式可清除默认来源；直接指定 Full access 字符串但无有效默认或独立确认仍拒绝。委派仅使用事务内专用创建入口，不读取默认；普通 fork 保持 Confirm。配置身份使用已存在配置文件的规范绝对路径，与确认时相同；丢失或不可读取的默认来源拒绝，创建入口不自动写配置。草稿自动化创建返回事务实际保存的线程及任务，提交成功后才发布派生 grant；创建期间以现有线程操作 owner 阻止竞争发送/设置，并冻结该草稿的项目重绑及默认模型刷新。回执只清除原草稿并更新匹配的 stream/current route，不覆盖其他路由或更晚的持久设置。自动化 epoch 失效只丢弃过期任务投影，不丢弃已提交的线程物化事实；物化线程不等于授权自动化任务。
普通发送的物化回执独立于后续 workspace/技能准备和启动结果：一旦线程事务提交成功，即使之后失败、取消或路由改变，也保留实际线程；仅由匹配的 submission/attempt owner 清除原草稿并更新匹配的 stream/current route，不以 ID 相同接纳任意现存行。创建期间冻结该草稿的项目重绑与默认模型刷新。权限 fence 在提交前校验并暂存，成功提交后才发布到不可变接纳状态；确认回滚且线程仍不存在时可用原始草稿来源重试，来源变更仍拒绝，不换成较晚默认。提交报错后不能确认回滚/持久状态时标记结果未知并阻止重试，不能按普通准备失败猜测无写入。A13 增加 deferred-commit 回滚后重试、默认变更后拒绝、提交成功后技能失败/取消仍保留物化回执，以及迟到回执不覆盖新 owner/路由的回归。

4. 新线程从有效默认 grant 派生自己的 thread-scoped grant，保存来源而非共享同一 thread id；创建线程与 grant 在同一数据库事务。线程自己的显式确认可先通过同一服务 materialize 空草稿，提交失败不产生已授权半成品。随后默认被撤销只影响未来线程，不追溯撤销已派生线程；提供独立的全量重置动作，不用修改默认冒充全局撤销。
5. 正常退出/重启不轮换 epoch，不清有效 grant，也不自动续跑。应用重新读取并验证完整记录后，之后的合法运行无需重复确认。当前用户级数据库/配置可以被 P2 改写；这些字段防意外继承，不提供同 UID 防伪、密码学授权或可靠降级检测。

线程/默认授权变更、默认派生、自动化授权、admission 和全局重置必须经过同一服务级协调门禁；涉及配置与 DB 时固定顺序为协调门禁 → ConfigEdit → DB 事务，不持有锁等待用户确认或运行收尾。默认派生在配置锁保护下重核 default fence 和 ready/epoch，再提交线程事务；重置不能插入该窗口。此锁序约束应用内正常并发，不声称能阻止外部同 UID 改文件。服务启动先恢复 pending 重置再开放授权/调度；任何读写故障不生成临时授权，也不以旧缓存替代。

#### 默认权限 Settings 动作细化

- 通用设置的默认权限选择不再调用普通偏好补丁。普通偏好保存拒绝 mode/revision/grant 的任何候选变更，且仅修改自己的字段，不覆盖磁盘上的默认授权。conversation 的独立服务取得现有配置与数据库来源；UI 不直接访问 SQLite，也不触发迁移/初始化来补出授权。
- 点击选项时先比较页面显示的原始 mode/revision/grant 与磁盘值，再捕获规范配置来源、数据库身份、ready epoch 和目标模式。受限模式经同一服务保存并清除 grant；Full access（包括当前已选同名模式）等待独立页内确认。确认明确仅适用于之后创建的新线程，并披露无 Vega Bash 沙箱/凭据隔离、同 UID 可读写和外传配置/秘密、持久影响不会因切回受限而自动撤销。现有线程/草稿/自动化均不补权。
- 等待确认不持锁；确认保存重新核对完整原始来源、数据库身份、epoch、revision 及 mode/grant，不能静默换绑新快照。锁序仍为协调门禁 → ConfigEdit → DB 事务；只打开已有数据库，不为设置动作迁移或创建授权状态。配置用规范来源路径原子保存，保留其他偏好，只有成功读回相同权限字段才发布结果；读回失败/失配作为结果未知，不自动重试。
- 页面保持真实已保存选择，不预先选中待确认值。准备可取消；Esc/取消返回选择器焦点，切页丢弃未确认意图。提交开始后不把返回/切页当成撤销，暂留保存态直到实际回执；不重复提交。异步回执绑定本设置实例和单调 operation 身份，取消后的准备回执不再打开确认。权限写入/读取与同页普通偏好写入互斥，确认等待期间可以编辑其他偏好，但保存中的偏好不允许确认提交。权限成功只更新三个权限字段，不回写旧模型/主题等；无关配置回执保留权限投影。只改默认权限不得触发既有草稿的模型重置，默认模型确实变化时才沿用既有模型刷新路径。
- 页内确认、取消和重新读取均提供文字按钮、可见焦点、Tab 与 Enter/Space 路径；确认出现先聚焦取消。失败保留原选择和就近错误；冲突或结果未知必须显式重新读取后重新选择并确认，不用旧确认重试。重新读取是用户请求的只读恢复，不会生成 grant。A21 增加上述取消、迟到回执、并发保存、同模式重授权、源/epoch/数据库替换和读回未知回归；本地不执行，待 Actions 与用户手测。

#### 运行、排队、自动化与委派

| 路径 | 必须绑定/核对的权限来源 | 不允许的恢复方式 |
|---|---|---|
| 普通提交、队列、replay | 入队时捕获 PermissionFence；admission 与 worker 启动时原样核对；运行内派发绑定已接纳快照及准备好的 command | 只比较旧 permission_mode；排队后从更宽默认/线程设置补 grant |
| 活动 run | 接纳时的不可变策略及 grant；派发再次检查 run 存活、取消、工具/工作区身份和单次许可 | 模式切换后把仍在运行的策略显示成已降级；忽略 Stop |
| 普通 fork | 新线程 Confirm、无 Full access grant | 复制源线程 grant 或默认 P2 覆盖 fork 安全预设 |
| spawn 子 Agent | 从父活动 run 导出上限相同的 delegated authority，绑定 parent run、child/execution id、source fence；不得把它当作独立用户线程 grant | 从父线程后来更宽设置补权，或继承父命令的单次网络/目录许可 |
| followup / resume | 保存的 delegated authority 加新契约字段；原有 parent/source/checkpoint/fingerprint/ownership/pending 条件继续成立，当前父能力不小于保存能力且对应 grant 来源未失配 | 旧序列化缺字段自动接受，重新构造 full_access=true 绕过恢复检查 |
| 自动化 | 任务创建/启用 P2 时单独确认，授权绑定 thread grant + task kind/identity/revision；保存到 task JSON，claim/run JSON 捕获相同 fence；任务修订/权限变化后需重新确认 | 线程后来授权就让既有任务无人值守变宽；默认授权直接覆盖旧任务 |
| 凭据续跑 | suspend 保存 PermissionFence；claim 同时核对该 fence、source version/fingerprint/raw-user fingerprint；旧缺失/unknown-effect 记录拒绝自动继续 | 凭据补齐后按当前更宽线程权限重新开始同一个未决操作 |

自动化已有的暂停/重启/预算语义不因 grant 持久化改变；扩围请求需要活跃的用户审批入口，没有响应按现有超时拒绝，不提供自动化长期网络/目录授权。重启后待审批卡不复活可消费许可，恢复同一任务也须重新请求该命令范围；持久审计记录不是可执行 token。

### 受限 Bash 与单次扩围（D4 拟议契约）

#### 能力与策略

- 新增执行策略 `ReadOnly` / `WorkspaceWrite` / `FullAccess`；前两种再带 `network=Denied|Direct|Broker` 和规范化的额外写根。策略由可信 runtime 决策产生，不接受模型指定“已批准”“full_access=true”或任意 Seatbelt 文本。
- Ask/Plan 和 Execute+ReadOnly 暴露 Bash，但强制 ReadOnly。它是“把任意命令放进只读约束执行”，不是把 shell 文本分类成不会写入；workspace、HOME 和 Vega 私有临时目录均不给 shell 写权限，仅保留 stdout/stderr 管道、`/dev/null` 等明确设备例外。TempRoot 由宿主管理，不能借 shell 启动需要临时目录就把 ReadOnly 变成可写。
- ReadOnly 路径仍先做工具/任务能力校验、危险检测；危险匹配直接拒绝，不通过 `Readonly` 通用工具自动批准分支绕掉检测。普通安全命令不需普通确认，但 OS 拒绝的写尝试按实际失败返回；目录扩围永远不允许。网络可独立申请一次，不保证远端 API 无副作用，所以“只读”只描述本地文件边界。
- Execute+Confirm/Auto 保留现有“危险 → ReadOnly 门禁 → 精确规则 → Auto/FullAccess → Confirm”对原有可写工具的决策顺序。新增只读执行分支是显式能力契约变更，不放宽 Write/Edit；Auto 的绝对路径文件工具特例、MCP、ComputerUse 和 ACP 审批继续独立。
- `WorkspaceWrite` 保留项目和每次私有 TempRoot 可写、项目外默认不可写、Git entry/真实 git dir 与凭据配置根强制 deny；读取沿 OS/TCC 及显式 deny，不新宣称“只读项目”或“所有凭据自动保护”。网络默认 deny，不因有 bindings 自动开代理。

#### 沙箱、环境和 IPC 的可验证边界

仅在现有 `(allow default)` 上加一行 `deny network*` 不足以宣称隔离。拟改为明确的默认拒绝 profile：允许执行/fork、OS 允许的文件读取、指定文件写例外及受控系统调用；未列的网络、Mach、IPC、进程控制默认拒绝。保留自身 `process-info-pidinfo`/`process-info-setcontrol`，signal 最多到同 sandbox，拒绝对外部进程控制及 taskgated；不批量复制 Codex 的 process-info*、Mach/IOKit/PTY/共享内存例外。

基础兼容集合只允许实现所需的系统读取、`kern.grade_cputype` 写入和账户查询 `com.apple.system.opendirectoryd.libinfo`；详细 sysctl allowlist 在实现 diff 逐项列出来源。网络启用时只追加 DNS/网络配置和证书服务所需例外，参照固定 [network policy](https://github.com/openai/codex/blob/970111f6cb62cc02fe36a82ea7d71b37a2889c97/codex-rs/sandboxing/src/seatbelt_network_policy.sbpl) 与 [conditional builder](https://github.com/openai/codex/blob/970111f6cb62cc02fe36a82ea7d71b37a2889c97/codex-rs/sandboxing/src/seatbelt.rs#L319-L380)核对；不是复制全部白名单的授权。离线不允许 DNS/网络 helper 的 Mach 查找、Unix socket、loopback 或任意 Apple Events/外部进程代理。若必要 API 不在已列范围内，先修订策略与正反例再扩大，不通过无沙箱重试解决。

所有受限 Bash 统一复用/提取现有 `controlled_environment`，显式 `env_clear`、私有 HOME/TMP 和 `zsh -fc`，不读取用户登录脚本或继承代理/SSH socket/任意秘密环境。现有 builder 已静态核对：PATH 固定 `/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin`，HOME/ZDOTDIR/TMPDIR/TMP/TEMP/TEMPDIR 指向本次私有 temp，LANG 为 `en_US.UTF-8`；不从父环境补其他项，绑定 alias/CA/proxy 环境仅在获批 Broker 路径追加。固定 PATH 不是可执行文件可信性保证，也不把环境过滤冒充文件隔离。此变化可能影响依赖 shell rc、缓存、GUI Mach 服务或全局临时路径的命令，须在实施确认中明示，不能以浏览器必然兼容收尾。

final profile 必须一次构造并同时用于自检与 spawn，包含本次网络、额外根、Git/凭据 deny 和私有临时目录；现有只用 base profile 执行 `true` 的自检需修正。`true` 成功只证明该 profile 可启动，不证明写/网络/IPC 阴性案例；这些由用户合成 fixture 手测。FD 不继承不必要的宿主能力，后代继承约束；这些不是全产品 DLP，也不防已存在的无沙箱同 UID 进程、用户主动转发或所有系统漏洞。

#### 请求、审批与执行状态机

1. Bash 输入拟新增可选 `requested_scope`：`network`（bool，默认 false）、`write_roots`（绝对目录列表，默认空）以及有非空范围时必填的简短 `reason`。未知字段/空请求/不合法路径拒绝，原 command/timeout 兼容。模型只能请求，不能指定批准结果；无沙箱升级不是这里的选项，必须由用户在权限控件确认 P2 后开始新的 run。
2. 新增独立 `CommandScopeRequest` / `CommandScopeDecision`（AllowOnce、Deny、Timeout）。UI 可复用 PermissionQueue 展示与 first-wins 生命周期，但类型/验证/codec 与普通 PermissionRequest 分离；不得接受 Always/AcpOption/MCP 响应，也不写项目 `permissions`。保留原始 ordinary/danger 审计，范围审计写独立记录，不把范围批准当成危险批准。
3. prepare 后，先验证能力、命令危险门禁与普通审批；任何拒绝不进入扩围。已有 ordinary Once/Always 或 Auto 只免普通卡；范围卡仍必须显式响应。卡片绑定 exact command bytes（摘要只作索引，不能替代相等比较）、有效 timeout、cwd/项目/工具实例身份、run/call/request id、PermissionFence、规范化范围及相关 bindings 配置快照。改命令、目标、范围或来源都使卡失效，不能凭同 command 重用另一次 call 的许可。
4. 范围批准只生成该 live run 内可消费一次的 token；dispatch 前重核取消、身份、scope 与快照，再在任何本次进程启动尝试前原子消费。取消、超时、替换/消失、重启、无监听者、存储审计失败均不启动；spawn 失败或效果未知也不把 token 放回。Broker/CA/lease 和 shell 都在必要审批完成后创建，不先启动代理再等人批准。受限路径先取得获批网络所需 lease 参数，再构造同一 final profile 自检并启动命令；自检失败或期间取消要回收已建 lease/temp，不启动用户命令、不返回可复用 token。
5. 限制内命令失败不自动重试，不根据 EPERM/非零 exit 猜测哪项权限缺失就去掉沙箱；反馈实际已知错误及“需要更大范围时提交新的请求”。后续新请求需新 call 和明确确认，原调用可能已产生的部分副作用不能隐藏。普通范围批准不切线程模式、不更改默认、不授予子 Agent 或下一条命令权限。

#### 网络与目录范围

- 默认网络 Denied：包括外网、loopback、入站监听、Unix socket 与未经允许的代理通道。无 bindings 的一次网络批准采用 Direct，允许该命令及受约束后代的直接网络（含可能访问本机服务/外传），卡片必须准确显示“允许本次命令联网”，不伪称限定某个 URL/域名；不开放 Unix socket 或任意 Mach/外部进程控制。
- 有有效 bindings 的一次网络批准采用 Broker：仅允许 shell 到本次 lease 的 loopback TCP，既有代理校验绑定 HTTPS host/port，真值注入仍要求 path+alias 匹配。卡片展示可连接目的地范围，不把认证 path 说成所有请求 path 围栏。绑定目的地之外的普通下载仍可能失败；本卡不重写代理为通用转发，也不因失败切 Direct。去除绑定/申请 P2 都不是自动恢复动作。
- 受限读取必需/绑定凭据仍严格；默认离线虽不创建 Broker，也不扩大 D3 对缺失 binding key 的 P2 豁免到受限 run。有 binding 的配置快照在范围审批及 dispatch 复核，变化即本次失效，不把旧批准解释成新目的地批准。
- 写目录扩围仅 Execute+Confirm/Auto；本次 shell 及受约束后代可写明确的额外目录，不代表只允许一条文件 API。目录必须已存在、可 canonicalize，绑定设备/inode 并复核；不支持 root/home 等过宽根、不直接授权 Git 元数据、凭据根或覆盖这些强制保护的祖先。保留项目/额外写树 hardlink 检查和不可表示路径拒绝；强制 deny 排在允许规则之后，不因父目录 grant 被覆盖。
- 目录 grant 不是对用户文件的所有权/删除批准，也不能覆盖危险硬门禁。symlink/hardlink、别名、目录替换的实际 OS 边界须用专用 fixture 正反验；若不能保证所声明路径边界，拒绝该扩围并报告，不把预检当作消除全部 TOCTOU 的证明。路径归属不明、需要修改真实用户数据时仍遵守任务授权。

### 严格凭据读取与脱敏（D4 拟新增 API）

新增只读 `lookup_existing_key(root, ref) -> Result<Option<SecretValue>, LookupError>` 或等价的内部 typed API：`None` **仅**表示已通过根/目录/文件身份、owner/mode/nlink、大小与格式检查的现存安全文档内没有这个 ref。根/目录/文件不存在分别返回 `StoreAbsent`，其他 Unsafe/IO/Malformed/Limit/Unsupported 保持错误；不在 lookup 中创建目录/空文件，也不复用会把文件缺失变空文档的 `platform::read` 而丢失原因。现有写入/初始化 API 可保留其行为，不改存储后端。

owner 汇总按接纳的执行策略选择：P2 的可选 binding refs 用上述严格 lookup，Some 进入脱敏集合，None 可忽略；必需 Provider/MCP 和受限 Broker 仍要求存在。相同 ref 若也是必需凭据，必需条件优先；真实存储错误在 P2 也失败。没有可选 refs 时不凭空初始化或探测不存在的 store。身份/私有权限/格式验证保持严格，不能 catch-all 成空列表。

改动覆盖 `app_agent` 的汇总闭包、pipeline 的运行前读取和后续 Provider/输出隐私检查；先取得可信执行策略再选择缺失处理，不能按 UI 标签或模型参数选择。旧统一 reader 为脱敏读取已存在的 key 仍允许，不属于 Bash Broker 读取；不要为绕过 A3 关闭 privacy authority 刷新或 owner redaction。启动之后的真实错误按现有失败/取消链报告，不声称可撤回已传出的内容；不新增通用 secret 扫描或记录密钥值。

### 正常重启与受支持的回滚

用户选择正常重启保留授权；D4 不承诺自动发现任意旧 binary 曾运行、手改配置或备份还原。旧 binary 对更高 schema 直接接受的事实，使“仅加新 marker 就保证所有降级后重授权”不可成立；这是对 D3 A13/迁移提案的显式修正，需随实施范围确认。

拟提供服务级“重置完全访问授权”动作，供用户准备降级时执行：先暂停新 admission/自动化及授权变更，要求活动 run 停止并完成可确认的收尾；未知效果保留恢复阻塞。进入上述协调门禁并取得 ConfigEdit 后，数据库事务设置 reset pending、轮换 epoch、清线程 grant/自动化授权并把 Full access 线程改为 Confirm，递增受影响 revision，失效未启动任务和所有单次许可；随后原子保存默认 Confirm、清默认 grant 并增加 revision；最后事务标 ready。它是跨 DB/config 的可恢复 fail-closed 协议，不声称跨两种存储原子提交。首个事务前失败则没有持久重置完成、不得提示可降级；pending 一经落库，后续失败就保持 pending，不恢复旧 grant。重启幂等继续同一重置，不反复轮换 epoch，期间不派发 P2，也不重新开放自动调度。

只有读回线程/默认/自动化与 pending 均已完成重置，才提示可退出并安装兼容旧版；回滚代码保留新增列/表，不降 schema、不删凭据。之后升级不恢复已清 grant。若直接手动降级、用旧程序改 mode 再切回、还原含旧 grant 的整份备份，则不保证自动检测；必须先走上述重置或由用户明确接受未走流程的风险。此限制不能隐藏成“正常重启会自动重新确认”。

### 界面行为与必要提示

保留简洁权限选择器；首次获得 P2、旧同名模式重新授权以及全局默认授权时显示必要确认。确认前讲清：可访问当前用户文件/凭据并联网，配置修改可能影响后续任务，绑定不会自动用于 Bash，切回/停止/回滚不能撤销既有影响；授权正常重启后保留，准备降级须先重置。提供明确的启用/取消动作；线程、未来新线程默认及自动化任务范围分别展示，不默认勾选更广范围。取消、持久化失败不更新有效权限；旧同名模式必须能触发重新授权，不能被 `mode == current` 吞掉。

三类卡有不同动作：普通命令审批保留现行选项；扩围卡显示完整命令、网络实际范围/额外写目录与理由，仅“允许本次/拒绝”；Full access 为持久范围确认。危险卡不与范围卡合并成含糊的“全部允许”。默认禁网改变在升级后的相关权限帮助与第一次阻断时可发现，不每次命令附说明；只读模式说明本地只读，网络批准可能影响远端状态。

模式控件表达真实选择，活动 run 与下次设置不混淆；普通成功不叠加常驻提示。绑定存在时仅在相关授权/绑定入口说明 Bash 不使用受限代理，不对所有命令重复警告。错误就近说明原因与可采取动作，键盘可达、Esc 取消并合理回焦；风险不得只藏在 hover。逐项验收默认、运行、保存失败、未授权、取消与恢复状态。

## 全面对齐差异表与范围门禁

不能用“全面对齐”自动撤销 Vega 危险门禁。D4 将同卡目标具体化，同时要求用户在实施确认中接受下列保留差异；当前均未实现、未验收，不是等价 ChatGPT App 的声明。公开 Codex 参照仍绑定上述 SHA/文档边界。

| 维度 | 本卡拟对齐行为 | 明确保留差异 / 完成条件 |
|---|---|---|
| Full access | 经新契约确认的 Execute Bash 无 Vega Seatbelt/Broker，绑定不阻断 | danger、任务能力、生命周期仍在；不是全部工具的 `never`；A1/A3/A5/A11 |
| 审批 vs 隔离 | 普通批准与执行范围分离；联网/目录范围独立单次确认 | 保留四种预设而非任意两维 UI 组合；Auto 不是 auto-review，无持久扩围规则；A15/A21 |
| 只读 | Ask/Plan/ReadOnly 可用 OS 约束的 Bash，不靠文本猜测 | 本地只读，私有 temp 也不可写；网络须单次确认且可能有远端副作用；危险命令拒绝，不升级可写；A4 |
| 受限读写、Git、临时目录 | WorkspaceWrite + 明确额外写根，强制 deny 不被父目录 grant 覆盖 | Vega 配置根/Git 保护保留；不提供单次无沙箱逃生或 Git 元数据扩围；A2/A16 |
| 受限网络 | 默认禁网，授权后 Direct 或 Broker；失败不自动重试 | 有 bindings 仍限既有 HTTPS host/port，不改代理为全网转发；无 bindings 的 Direct 不伪称按域名隔离；A12/A15 |
| 环境/子进程/IPC | 默认拒绝边界、统一受控受限环境、子进程继承 | 不照搬 Codex 白名单；不承诺受限 GUI/浏览器完全兼容，不关闭内层 sandbox；A20 与平台正反例 |
| 凭据分层 | P2 与受限分流，准确缺失分类，保留脱敏 | owner-only 明文后端、具名工具/MCP 独立保护保留；不迁 Keychain、不宣称所有新 secret 自动隔离；A9/A10/A19 |
| 旧数据/恢复/委派 | 版本化 grant/fence；正常重启保留；旧数据不补权 | 自动化独立确认；普通 fork Confirm；支持显式重置后回滚，不保证任意手动降级检测；A6/A13/A17/A18 |
| UI 与错误 | mode/活动快照/下次设置一致，三类授权入口清楚 | 必要风险/失败/恢复可见，不以解释代替控件，不常驻堆提示；A8/A21 |

完成定义：每项差异最终应是“已按批准契约实现并验证”或“用户明确接受的差异”；仍未裁决或未验收的行不打勾。若先交 Full access 阶段，应明确部分交付，不关闭 #624，也不声称实现 ChatGPT App 全部安全特性。

## 实现顺序（另获实施授权后执行）

1. 本轮冻结并保存 D4 精确版本，完成文档静态检查；独立设计审查按用户要求跳过，不记录为通过。将范围、必要性、风险、保留差异和回滚一次性提交用户确认。未获具体批准不改生产；本次跳过不自动豁免后续独立实现审查或 required CI。
2. 实施先完成 `vega_conversation::types::permission/thread/automation` 与 `vega_store` 的增量迁移、线程 CAS、默认 ConfigEdit、重置状态机；接 `conversation::threads/ordinary_submissions`、`agent::pipeline/persistence/subagents`、`delegated_stages`、`credential_continuation`、`automation`。构造及恢复原样携带 fence；旧授权 fail closed 后才可接无沙箱启动。
3. 在 `vega_runtime::permission`、`agent::tools_exec`、`RuntimeToolConfig` 和工具定义中增加受限只读执行与范围请求；在 conversation permission_queue/codec/persistence 及 `vega_ui::permission_card` 分开三类请求；不覆盖 MCP/ACP/文件工具特例。对权限状态机、单次 token、CAS/恢复/旧记录等安全边界选择必要进程内开发回归，交现有 Actions；不以文本比较或 mock 代替真实决策。
4. `vega_tools::bash` 用已批准执行策略替代松散 bool 分支；P2 在 Broker/CA/context 前分流并直接启动，保留 scope/cancel/timeout/output/TempRoot/清理。`sandbox.rs` 构造只读/项目写/网络/额外根的同一 final profile 供自检与 spawn；`credentials.rs` 仅在获批网络的受限路径创建 Broker，不把配置根检查从其他工具移除。
5. `vega_store::keystore` 新增严格 existing lookup，`vega::app_agent` 与 pipeline/后续脱敏调用按可信执行策略处理可选 binding 缺失；保留必需 Provider/MCP/受限 key、privacy authority 与真实故障。同步模型工具说明、错误分类、本地化和 UI 服务事件，不能将只有错误码写作已实现可操作指引。
6. `vega::window::session`、Composer 与 settings 完成同名重授权、保存失败、默认派生及当前/后续 run 状态；新增 UI 文案逐项审视用途、键盘/Esc/回焦与真实错误恢复。无新依赖；新增 libc/unsafe 若超出既有批准范围需先具体列出再确认，不能借此设计自动授权。
7. 本地仅差异/格式/静态；开发 Rust 测试如使用，固定源码/patch、完整 SHA 和实际云端结果，再清理独占支持。最终无 Rust 测试 head 做独立实现审查与完整 required CI，空集写 `no Rust tests`（0 发现/0 执行），不是权限行为通过。真实 shell/network/IPC/浏览器/原生 UI 仅用户手测，不新增真实外部 E2E 门禁。
8. 最新 head required CI 与独立审查通过才 squash 合并；整卡保留 In review 等用户对合成凭据/独立项目的 A1–A21 验收。受限平台不满足目标时报告原始证据并修责任层，不放宽断言、自动去沙箱或只交 P2 后关闭卡。

## 影响、必要性与回滚

必要性是修正声明与实际执行边界冲突，不是绕过某个浏览器报错。P2 移除获准 Full access Bash 的 Vega Seatbelt 及绑定冲突拦截，并跳过该路径的受限 Broker；仍受 OS 当前用户权限/TCC/外层策略。它不保证能读所有凭据，但也不能保证同 UID 可读明文凭据对它不可读。现有/新增凭据、配置与项目外文件、Git、网络外传及跨运行持久影响都属于授权前必须告知的范围。

受限侧同时改变可用性：默认禁网、私有受控环境和更窄 IPC 可能让现有依赖联网、shell rc、全局缓存或 GUI 服务的命令失败。必要性是使“受限”有实际 OS 边界和可解释的单次授权，不是保证所有命令无感兼容；本版不是零风险或零行为变化的一行修复。

回滚先执行上文受支持的授权重置，再退出并 revert/安装兼容旧版；schema 增量保留。切回 Confirm/Auto 只恢复后续新 run 的受限规则，不修复配置可信性，不撤销已执行命令、外部写入、网络外传或脱离的后台进程。用户须核查/恢复被改配置，真实泄露须轮换凭据。任意未走重置的降级/备份还原不保证检测。当前已按用户明确批准进入仓库实现，尚未交付可运行版本；未更改本机运行配置，也未执行任何重置。

## 当前验证与下一步

- 设计阶段已 fetch 并正常合入当时 master；当时仅修改 `docs/624/`，该范围描述不适用于当前实施阶段。实施基线与迁移编号见下方阶段记录，当前差异包含授权持久化、conversation 服务及 app/UI 接线；未操作主仓用户原有未跟踪内容。
- D1 审查绑定 `528409bf50a5180bd27873a76fca5aa6f23e9829`；D2 `1f98926e6ed77aac1eebef138a0a227e7461f86b` 吸收其发现。D3 最终独立审查绑定 `aae1d34001f21c9756b28630f6343fbc4fd5fc26` / SHA256 `c26502a9499dc3c74069e97bbadcdc4ecf2bbfad9b9be8026891f8bde2e1197f`，结论为可作 P2 待授权设计，无该版文档阻塞，不是生产 READY。原版本与 Issue 评论保留。
- D4 设计冻结时提出具体 grant/schema/原子性/恢复、只读与单次扩围、严格凭据读取、受支持回滚及保留差异；当时新增字段/API 均待实现，当前实现范围以以下阶段记录为准。初始冻结提交 `5c209da90543467d7c66746ae9ecce7dcca47903`（文档 SHA256 `8e1a54e9aef8ee2d3da575155038e7fe9fdc1911a364384a0dde060995de0e6e`）已推送并读回一致。其独立审查被用户中途取消，未形成最终结论；本版只同步跳过状态，最终静态检查绑定提交/hash 写回 Issue，不将部分阅读或旧 D3 审查算成 D4 通过。
- 未运行 Vega、编译/构建/测试、CI、真实凭据读取、浏览器或 UI 集成；未改本机权限/配置/凭据。#624 基础实现已提交为 `decccd52a675de7dbe8ebba0ffbea8285eadd512`，尚无实现 PR 或交付合并。静态检查不能证明 Seatbelt、网络或恢复行为。
- 实施授权更新：用户已明确要求“按 D4 开始实现”，并再次要求优先补齐授权与恢复接线，再推进 Bash 权限和 UI，完成后走云端 CI 与独立实现审查。此前待授权/仅文档状态属于历史阶段，不再作为实施阻塞；仍禁止本地编译测试、运行 Vega 或修改本机配置。
- 当前实现进度（阶段提交 `decccd52a675de7dbe8ebba0ffbea8285eadd512`，不是 READY）：增量迁移、grant/fence 类型、线程授权 CAS、运行快照落库及凭据续跑比对；快照包含连接数据库身份，普通准备保存不可替换快照并在运行准备复核 revision/grant/epoch/工作区，缺失不补权。默认权限增加 revision/grant 持久字段和独立保存服务；旧 grant 解析失败不阻止查看线程，执行入口仍严格拒绝。尚缺首次入队完整捕获、默认派生与 UI 接线、自动化/委派/重置完整协议，以及 Bash 沙箱和单次扩围实现。
- 前次集成上游 `60adaaff` 的 HEAD 为 `6aee1c86555d081c17283d2c94e5d23173d864f0`；随后合入 `e47afcba49592f333fdc25e508f0eb5e60e4883b`，集成提交 `4739f6e25d21a0984af2033a2f74df39cbba4996`，基础实现已按用户要求提交。恢复点 stash `07faa227412e223a8eff2b1e563c5a7a484f1895` 保留。迁移顺延至 0040。委派准备要求当前 running 执行记录与父/子线程及 message 身份精确匹配，缺失记录不再等同首次执行；前序快照缺失仍拒绝。父运行重启后的 resume 不要求旧/新父 message 相同，仍核对保存权限来源及既有恢复边界。模式/规则与父快照的一致性验证已前移到准备事务，移除提交后的权限覆盖与迟到拒绝。子任务创建提交前再次验证相同来源；resume 在产物读取前后核对中断执行保存的 fence、当前活动父 fence、epoch 和数据库身份，不以当前子线程设置代替授权来源。原 source/checkpoint/ownership/pending/unknown-effect 检查保留。检查点记录原执行 fence，resume 的事务外预检及最终创建事务均核对检查点与原持久 fence 一致；旧检查点缺字段拒绝。`delegated_permission_fences` 在创建事务内保存接纳快照，不依赖尚未创建的 assistant message；worker 准备与接纳快照精确比对，检查点及前序/中断恢复引用同一记录，覆盖接纳后启动前中断。旧数据缺记录不补造授权。创建/恢复预检使用协调门禁 → DB 事务，不跨用户等待或产物校验持锁；`DelegatedAuthority::matches_fence` 在接纳、worker 和恢复产物读取前统一核对模式及规则。该链仍待云端编译和行为验证。
- 默认派生/Settings 续接批次（阶段实现，非整卡 READY）：内存默认来源快照和共享创建 guard 接入普通发送、分组创建及草稿自动化，事务内派生并保存线程 grant；普通重试保持首次接纳 fence。草稿自动化已返回并投影实际物化线程，使用创建 owner/source 核对并冻结竞争草稿变更。默认权限选择改用 conversation 独立服务，准备/保存核对原始配置来源、数据库身份、epoch、revision/mode/grant；Full access 同名重授权也需要页内确认，受限选择清除 grant。普通偏好拒绝权限字段补丁，无关回执保留权限投影；取消/离页废弃未提交意图，保存态等待精确回执，失败提供显式重新读取。权限刷新不重置手动草稿模型。仅完成源码接线，键盘/状态/并发仍待验证；线程和自动化确认 UI、最早入队捕获及重置协议尚未完成。
- 新增开发回归 `crates/vega_conversation/tests/default_permission_authority.rs`，静态计数 10 个；覆盖准备/拒绝不写、同名重授权/重开、撤销拒旧确认、原始字段冲突、epoch/reset、配置/数据库来源、无关偏好保存、配置写失败、缺失来源不初始化及默认派生。只使用合成临时数据，无真实 shell/网络/UI；尚未执行，待现有 PR Actions，不能当作 A1–A21 全覆盖。合并前固定云端源码 SHA/结果后移除测试与专用支持，再执行最终 required CI。
- 此批本地已执行修改文件 `rustfmt --edition 2024 --config skip_children=true` 及 `--check`、`git diff --check`；TOML 解析与权限提示本地化键静态检查通过，新增生产行未见 unwrap/expect 或注释。Cargo/lock/workflows 未改；未运行任何 Vega 代码或测试，未修改本机配置。
- 下一步继续默认授权/草稿派生、入队快照、活动 run 与下一轮设置、重置/启动恢复，然后 Bash 和 UI。已静态确认 Bash 的 `sandbox=None` 仍可创建 Broker/context 并在底层重新套 Seatbelt，后续必须在凭据创建前分支，不能只删除 bindings 拒绝条件；本轮未改该路径。最新已执行检查仅为修改文件 rustfmt 和 git diff --check，无冲突标记；未取得编译、自动行为验证、独立实现审查或合并结果。Issue 保持开放/In progress。

### 版本变更

- D1：记录根因、P1/P2/P3 候选及最初验收；独立安全设计审查完成。
- D2：补持久配置风险、零绑定含义、错误指引证据及具名工具/网络边界。
- D3：记录用户选择 P2 规格方向；取消 P1 专属拒绝预期，明确无 Broker/无注入、新凭据与配置变动；增加固定 Codex 来源和过强结论更正、授权/迁移提案、整卡差异表与分阶段实施/验收。尚非实施授权。
- D4：落实正常重启保留授权/同卡受限对齐的方向，给出持久 grant/fence、线程/default/自动化范围、只读 Bash、默认禁网/单次扩围、严格缺失读取、回滚重置协议；显式替换 D3 A2/A4/A12/A13 预期，扩至 A1–A21；记录提交数例外和新基线。发布后按用户要求跳过本轮独立设计审查。发布后用户已批准按 D4 实施；保留差异与手动降级限制不变，最终实现审查、CI 与用户手测仍待完成。

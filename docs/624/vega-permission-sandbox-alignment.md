# #624 · 权限与沙箱语义对齐

## 状态与授权

- 版本：D3，2026-10-26；阶段：用户已选择 P2 规格方向 / 待实现授权，尚未实现。
- 关联：[Issue #624](https://github.com/puzige/vega/issues/624)、[#58](https://github.com/puzige/vega/issues/58)、[#378](https://github.com/puzige/vega/issues/378)、[#619](https://github.com/puzige/vega/issues/619)。
- 当前代码基线：`410758d36e8fbe0802f816227948ece1486d8494`；任务分支：`feat/624-permission-alignment`。D2 基线为 `dc7e37106f3e97aa22702e3dee6d4c7627bb3359`；本轮同步的 #544 / #627 为既有上游变更，不是本卡新增生产实现。
- 用户在“下一步沿哪条路线完善规格；这一步不修改运行权限”的问题中选择 **P2：更全面开放**，补充“全面对齐 ChatGPT”。这是规格方向裁决，不是执行放权、修改本机配置或合并生产实现的批准。P1/P3 不再作为本轮推荐；生产修改前另行明确确认本规格的范围、影响与回滚。
- “全面对齐”覆盖 Full access、受限模式、审批、凭证、界面、迁移及恢复委派；以可验证的公开 Codex 契约作参照，不宣称复刻闭源 ChatGPT App，不把只完成 Full access 当作整卡完成。
- 仓库规则：本地不运行 Vega、不编译或测试；自动检查交现有 PR Actions，真实平台集成交用户手测。Rust 开发测试如使用，必须在合并前留痕并清除，不恢复历史测试集合。

## 目标、范围与非目标

让审批、实际 OS 隔离、权限名称及风险提示一致。Ask/Plan/Execute 是任务能力门禁，ReadOnly/Confirm/Auto/FullAccess 是审批与执行策略，两者不机械映射为 Codex 三个模式。

本轮只完善规格及静态审查。实施路线分为 Full access 契约与旧数据兼容、受限模式差异裁决、完整验收三个阶段；阶段不是新增独立卡，也不替代整卡验收。下文逐项登记尚未定案的受限策略，不在 P2 名义下顺带放宽。

凭据存储后端、Keychain 迁移、Broker 重写、MCP 独立保护、具名工具入口检查、浏览器内层沙箱均不因 P2 自动改变。受限模式网络和进程/Mach 规则只有在其独立差异与风险获准后才可变更。禁止读取真实 secret 做验证、修改本机配置/凭据或接管日常浏览器。

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

以下为 P2 设计预期，不是运行结果或实施批准。均使用合成凭据、独立项目和可恢复标记；A2/A4 对受限模式的预期是首阶段保留项，后续如需改变，必须先更新相应规格与回归，不删除其安全不变量求通过。

| ID | 前置 / 操作 | 可观察结果 | 验证路线 / 当前状态 |
|---|---|---|---|
| A1 | Execute + 新契约已授权 Full access；零绑定但有合成凭据文件；包含正常配置和无配置工具实例 | Bash 不经过 Vega Seatbelt；不创建 Broker/保护上下文；对合成文件的访问仅受 OS/外层策略约束，不能写成凭据不可读 | 启动分支审查 + CI 必要回归 + 用户手测；未运行 |
| A2 | 同一无害命令切到 Confirm / Auto | 保留受限启动；项目外写入拒绝，普通项目写入可用，Git/配置根保护保留；有绑定的受控环境及代理约束不回退 | 静态差异 + CI 必要回归 + 用户 owned-fixture 手测；未运行 |
| A3 | 已授权 Full access，Provider/MCP 必需凭据正常、绑定语法有效；分别引用存在/缺失的合成 binding key、使用无关命令；排队期间增加/删除有效绑定 | 绑定本身不拒绝 Bash；不启动受限 Broker、不调用其 SecretReader、不创建租约/CA/别名环境、不直接注入真值；不自动删绑定、切模式或加沙箱。配置错误和授权失配仍按各自原因阻断 | Bash 策略与实际接线审查 + CI 必要回归；未运行，不沿用 P1“非空即拒绝”断言 |
| A4 | Ask / Plan，或 Execute + ReadOnly，含 once/always 规则 | 首阶段保留当前 Bash 拒绝；P2 不越过任务能力门禁。是否新增受限只读 shell 属对齐差异表，不冒称已等价 Codex read-only | runtime 静态审查 + CI 必要回归；未运行 |
| A5 | 危险命令、取消、超时、scope mismatch、临时目录身份异常 | 危险门禁、审计、进程组收拢、输出上限和安全清理不回退；不将现有收拢机制夸称为能追回所有脱离后台进程 | 静态审查 + CI 必要回归 + 用户进程手测；未运行 |
| A6 | 旧 Full access 线程/默认值，未知契约；排队、重启恢复、自动化、fork、子 Agent followup/resume | 缺失/未知/旧授权不能得到 P2；运行不热升级；继承与恢复必须同时匹配模式及新授权版本/范围；普通 fork 保持 Confirm | 迁移与恢复接线审查 + 存储/runtime CI + 用户手测；持久化方案待实现前冻结 |
| A7 | 运行前选 P2，运行中改为 Confirm；P2 曾修改合成安全配置 | 活动 run 的执行快照不热切换，后续新 run 使用受限策略；需要立即停止则用 Stop。切回不恢复配置可信性、不撤销外部效果或后台进程 | 快照/取消审查 + CI 必要回归 + 用户手测；未运行 |
| A8 | 鼠标/键盘选择与取消 P2；旧同名模式重新授权；保存失败、重开线程、默认设置 | 风险在确认前可见；取消/持久化失败不生效，重选旧同名模式有入口；授权范围与生效时点清楚；有绑定不再显示 P1 冲突拒绝 | UI/服务契约审查 + 必要 CI + 用户手测；未运行 |
| A9 | Full access 下的模型工具说明、结果、日志；read/glob/grep/write/edit 与 MCP | 具名工具检查与 MCP 独立保护保留；现有脱敏保留但不承诺覆盖编码/旁路；不把 Bash 声明为仍可使用受限绑定，也不因 Bash 模式删除 MCP 所需绑定 | 接线与独立安全审查 + 必要 CI；未运行 |
| A10 | 有/无绑定的 P2 改合成安全配置，后续受限 run 或其他线程读取 | 记录后续上下文可将修改后配置作为新快照的持久风险；不以快照相等、模式切回或数据库标记冒充用户批准或防同 UID 篡改 | 静态调用链 + 用户合成配置手测；信任取舍待实施授权 |
| A11 | P2 授权后新增一个合成明文凭据；再启动 Bash | 不自动扫描、搬迁、注入或转沙箱；不承诺自动隔离该凭据，绑定增减不改变 P2 路线 | 设计/启动审查 + 用户手测；未运行 |
| A12 | 从 P2 切回受限并建立新 run，有有效绑定；再使绑定与快照失配或 key 缺失 | 仅受限路径恢复原 Broker 行为；失配/不可用时 fail closed，不将 P2 的忽略绑定语义泄漏到受限路径，不静默降级无沙箱 | CI 必要凭据回归 + 用户合成代理手测；未运行 |
| A13 | P2 启动失败、取消发生在派发前、持久化/配置解析失败、回滚后再升级 | 无未授权启动或自动重试放权；原始失败原因可恢复；旧标记不在升级/降级往返时被重新解释为新批准 | 错误/迁移审查 + 必要 CI + 用户手测；未运行 |
| A14 | 核对下方全面对齐差异表及所有批准的受限策略正反例 | 每项有固定依据、批准/保留差异、实现与验证结果；未决项不能标整卡完成；CI 或浏览器单次成功不能替代权限矩阵 | 最终规格/实现独立审查 + 用户验收；未运行 |

## 已选方向：P2 的执行契约

P1（非空绑定仍拒绝）和 P3（保留凭证 Seatbelt）仅作为 [D2 历史候选](https://github.com/puzige/vega/blob/1f98926e6ed77aac1eebef138a0a227e7461f86b/docs/624/vega-permission-sandbox-alignment.md)保留；本轮规格按 P2 展开，不继续推荐 P1。

### 模式、绑定与网络

下表描述取得新契约授权后的首阶段目标；不是当前已实现行为。默认仍为 Confirm。

| 条件 | 普通 Bash | Vega OS 沙箱 | 凭证绑定/Broker | 网络与环境 |
|---|---|---|---|---|
| Ask / Plan，任意权限 | 保留能力拒绝 | 不启动 | 不启动 | 不适用 |
| Execute / ReadOnly | 保留只读门禁拒绝 | 不启动 | 不启动 | 不适用 |
| Execute / Confirm，无/有绑定 | 原确认或精确规则；危险门禁优先 | 保留受限 Seatbelt | 无绑定无租约；有绑定保留受限代理与注入 | 保留现状，网络差异另行裁决 |
| Execute / Auto，无/有绑定 | 普通命令自动批准；危险门禁优先 | 同 Confirm | 同 Confirm | 同 Confirm；不是 Codex auto-review |
| Execute / 新授权 Full access，无/有绑定 | 普通命令自动批准；危险门禁优先 | 不附加 Vega Seatbelt | 绑定不拒绝 Bash；不启动其受限 Broker、不提供代理替身或真值注入 | 不加 Vega 网络限制；保留直接 shell 的环境/启动方式与私有临时目录，不主动修改用户代理；仍受 OS/外层环境约束 |
| 旧/缺失/未知 P2 授权 | 不得派发 P2 Bash | 不通过换名字或自动降级掩盖 | 不删绑定以绕过状态 | 提供重新授权或改用受限模式的动作 |

P2 必须在创建 Broker、CA 和 `ShellCredentialContext` 之前分流。只删除 `CredentialProtectionRequired` 不满足契约，因为后续共享路径还会创建代理/上下文；只把 sandbox 参数改成 None 也不满足契约。保留整个 `Tools` 的凭据策略，避免连带取消具名文件工具检查。绑定本身格式非法、配置解析失败、Provider 不可用等仍是独立失败，不承诺 P2 绕过整个应用的就绪条件。与当前 Provider/MCP 无关的 binding key 缺失不属于这些例外，不能包装成通用“就绪失败”继续阻断 P2。统一脱敏仍可由可信 owner reader 读取实际存在的已知凭据；须区分缺失引用与凭据库/配置故障，不关闭统一脱敏、不吞全部错误、不放宽受限 Broker/MCP 的凭据检查。

Full access 下“绑定不自动用于 Bash 请求”与“凭据不可访问”是两回事。该模式不启动 Bash Broker，不表示 shell 不能直接读取同 UID 可读明文凭据，也不表示 MCP 或 Provider 停止使用自己的受信任凭据客户端。仅清空环境、输出脱敏或保留具名工具检查都不能提供这种文件隔离。

### 新凭据、配置变化与信任

- 零绑定仍可能有 Provider 等凭据；已有/新出现的同 UID 可读明文凭据都可能被无沙箱 Bash 访问或外传。不增加通用 secret 扫描，不静默移动/删除凭据，不把模式自动切回受限以维持虚假保密承诺。
- 在已获 P2 授权的同一 run 中，有效绑定增减不改变 Bash 启动路线，也不要求重新接受一次“零绑定”条件；P1 的绑定冲突检查及相关 race 预期不适用于 P2。权限契约、scope、取消与上下文身份仍须在派发时有效。
- 受限 run 的配置快照/SecretReader 一致性检查保留；变动失配时按既有错误停止凭据使用，不借 P2 自动退化为直接 shell。新建受限上下文会重读配置，因此旧快照的相等检查不证明新配置获得用户批准。
- 无沙箱 Bash 可修改共享配置及授权数据，影响后续 run、其他线程或后台进程。P2 采用当前用户级可信执行，不提供同 UID 防篡改；数据库中的契约版本只解决正常迁移/意外继承，不是针对恶意 shell 的权威边界。该持久影响仍须在实施授权中明确接受。

### 授权与迁移提案（生产修改前冻结具体表示）

1. 将“权限模式”与“新 Full access 契约授权”分开表达。建议持久化版本化授权记录，包含契约版本及作用范围；不只加一个可误用的全局 `true`。缺失、未知和旧 `full_access` 均视为需要重新授权，保留历史值，不批量把旧记录写成“已同意”。
2. 线程授权仅用于该线程后续新 run；默认设置的全局授权应在独立设置动作中确认，并明确覆盖未来新建线程，不能由一次线程选择隐式提升全局默认。更改默认值不追溯授权既有线程或自动化；旧默认值不能产生 P2 线程。
3. 运行快照同时绑定权限模式、契约版本和授权范围；在 admission、排队恢复、runtime 构造及 Bash 派发核对同一来源。运行中的切换仅用于下一个 run，不热升级/撤销当前 run；需要立即停止时走现有取消链。以明确事件失效 pending/replay，不靠 UI 一段说明替代真实状态。
4. 子 Agent 仅继承父活动 run 已有授权，不从后来更宽的线程设置补权；followup/resume 同时检查契约、authority、source/version/fingerprint，失配拒绝。普通 fork 保持 Confirm；旧自动化/持续任务/凭据续跑没有新契约时不得无交互放权。
5. 当前接线证据：`composer::request_permission_mode` 对同模式不发请求；`pipeline` 快照比较现行模式；`DelegatedAuthority` 只有 run_mode/permission_mode/exact_rules；`delegated_stages::authority` 无新契约字段。必须改实际授权事件及完整接线，不能仅改 tooltip。UI 不直接读写 SQLite，runtime 不依赖 UI。
6. 实施前补齐字段/类型、增量 migration 编号、旧/缺失/未知解析、默认继承、恢复序列化与降级再升级方案；不能在文档中把未选定的 schema 或尚不存在的 API 写成实现事实。回滚旧二进制后的再升级不得凭遗留授权标记静默复活 P2。

### 界面行为与必要提示

保留简洁权限选择器；首次获得 P2、旧同名模式重新授权以及全局默认授权时显示必要确认。确认前讲清：可访问当前用户文件/凭据并联网，配置修改可能影响后续任务，绑定不会自动用于 Bash，请求取消或切回不能撤销既有影响。提供明确的启用/取消动作；取消、持久化失败不更新有效权限。旧同名模式必须能触发重新授权，不能被 `mode == current` 吞掉。

模式控件表达真实选择，活动 run 与下次设置不混淆；普通成功不叠加常驻提示。绑定存在时仅在相关授权/绑定入口说明 Bash 不使用受限代理，不对所有命令重复警告。错误就近说明原因与可采取动作，键盘可达、Esc 取消并合理回焦；风险不得只藏在 hover。逐项验收默认、运行、保存失败、未授权、取消与恢复状态。

## 全面对齐差异表与范围门禁

不能用“全面对齐”自动撤销 Vega 现有危险门禁或把受限模式一起放宽。以下差异全部属于本卡跟踪范围；“保留”是当前方案边界，不是已获用户接受的完全等价声明。

| 维度 | 对齐目标 / 当前差异 | 下一步与完成条件 |
|---|---|---|
| Full access | 当前隐含凭证 Seatbelt、非空绑定拒绝与名称冲突；P2 取消二者 | A1/A3/A11 与新授权链验证；本轮已选规格方向，未实现 |
| 审批 vs 隔离 | Codex 两维独立；Vega Confirm/Auto/FullAccess 是预设组合 | 文档和 UI 准确映射；保留危险门禁，不把 P2 冒称 `never` 或把 Auto 冒称 auto-review；若要求独立选择器另定具体产品契约 |
| 只读 | Vega ReadOnly/Ask/Plan 拒绝 Bash，Codex read-only sandbox 是可运行受限命令的策略 | 首阶段保留；需明确是否增加只读 shell，不能仅凭 shell 文本判断“只读”，若改变须独立 OS 策略/能力验收 |
| 受限读写、Git、临时目录 | 两者都不能简化成只读项目；Vega 有额外配置根保护、Git 写入保护与私有临时目录 | 列正反例，比较固定 profile/构造器；先保留现行围栏，任何改变注明具体路径类别、风险与回滚 |
| 受限网络 | Vega 无绑定放行网络、有绑定按租约代理收窄；与 Codex 特定配置的网络关闭/升级审批并不等价 | 单独冻结默认联网、域名/代理及失败后的审批路径；不得复制局部 profile 或默认为这次已批准放宽/收紧 |
| 环境/子进程/IPC | Vega 直接 shell 与 Broker shell 环境不同；受限进程规则不等于 Codex same-sandbox | 不批量复制白名单或禁用浏览器内层沙箱；每项兼容需求有静态依据及用户平台正反例，无证据不许宣称兼容 |
| 凭据分层 | Vega owner-only 明文、Broker、具名工具/MCP 与 Codex 的后端/工具组织并不相同 | 本卡不迁入 Keychain；明确 Bash 与其他工具边界，不承诺无沙箱下同 UID 明文不可读；不以存储后端差异阻塞已明确的 P2 规格 |
| 旧数据/恢复/委派 | 现有 mode 字符串不足以区分旧契约 | 冻结并验证版本化授权、默认/线程范围、排队与恢复 fences，A6/A8/A13 |
| UI 与可操作错误 | 当前“不使用沙箱”与运行冲突；旧同项不能发重新授权请求 | 实际行为、有效权限与下一 run 设置一致，必要风险/恢复/键盘状态可验收 |

完成定义：每项差异最终应是“已按批准契约实现并验证”或“用户明确接受的差异”；仍未裁决或未验收的行不打勾。若先交 Full access 阶段，应明确部分交付，不关闭 #624，也不声称实现 ChatGPT App 全部安全特性。

## 实现顺序（另获实施授权后执行）

1. 本轮先完成 D3 规格静态检查和非写入者的独立安全设计审查，保存精确版本；补齐具体授权表示与受限策略差异的裁决，不把审查本身当批准放权。
2. 先实现新授权的 UI/服务/持久化/快照/恢复接线，再启用 P2 启动分支；不得让中间提交仅删除 Seatbelt 而所有旧 `full_access` 自动变宽。涉及 `vega_conversation::types`、`threads`、`agent::pipeline`、`agent::persistence`/`subagents`、`vega_store` 默认配置/增量迁移/`delegated_stages`/`credential_continuation`、`vega::window::session` 与相关 UI 事件。
3. `vega_tools::bash::execute_bash_inner` 删除仅对 Full access 的绑定冲突；`execute_bash_with_temp` 在 P2 分支跳过 Broker/context 创建；`sandbox::spawn_full_access_shell` 使用直接 shell。保留共享 runner、TempRoot、scope/cancel/timeout/output、受限路径、文件工具的 `ToolCredentialPolicy`、MCP 保护；不扩大通用 API 或引入依赖凑测试。
4. 核对 `vega::app_agent` 的 `broker_credential_reader` / `owner_credential_reader` 与 `vega_conversation::agent::pipeline` 运行前读取及后续脱敏调用，明确 P2 下无关 binding key 缺失的错误分类与处理；保留当前 Provider/MCP 必需凭据和真实凭据库故障门禁。核对 runtime 工具说明、绑定投影：P2 不宣称 Bash 代理可用，不输出真值补兼容性；错误区分未授权、独立就绪失败与 spawn 失败。根据差异表分别处理受限模式，任何验收变更先写 spec。
5. 按风险选择必要进程内回归交现有 PR Actions，重点覆盖新旧授权、默认与继承、A3 的存在/缺失 key 和配置变化、P2 与受限凭据路径分流、失败/取消。不能仅比较源码字符串或 mock 替代权限判断；不新增真实外部 E2E，本地只做静态检查。
6. 如使用 Rust 开发测试，先固定可恢复来源与云端结果，再清除测试及独占支持；最终 head 独立实现审查和完整 required CI。空集如实写 `no Rust tests`，不能用空集证明权限行为。
7. 最新 head 门禁与独立审查合格才合并；阶段交付与整卡状态明确记录，系统手测由用户使用合成项目/凭据及独立浏览器实例完成。只有完整范围获用户确认后才关闭/Done。

## 影响、必要性与回滚

必要性是修正声明与实际执行边界冲突，不是绕过某个浏览器报错。P2 移除获准 Full access Bash 的 Vega Seatbelt 及绑定冲突拦截，并跳过该路径的受限 Broker；仍受 OS 当前用户权限/TCC/外层策略。它不保证能读所有凭据，但也不能保证同 UID 可读明文凭据对它不可读。现有/新增凭据、配置与项目外文件、Git、网络外传及跨运行持久影响都属于授权前必须告知的范围。

回滚通过 revert 本卡生产实现与兼容的数据迁移策略恢复既有启动保护；切回 Confirm/Auto 只恢复后续新运行的受限执行规则，不自动修复配置、恢复凭证授权可信性，也不撤销已执行命令、外部写入、已发送请求或后台进程。配置若被修改，需用户核查/恢复；不能通过回滚代码恢复已经泄露的凭据，如发生真实泄露需轮换。未获准前无运行时变化。

## 当前验证与下一步

- 已核对：Issue 开放、Project In progress；任务分支无他人修改，主仓用户原有未跟踪文件保留。fetch 并同步上述最新 master；复核 Bash 分流、配置快照、模式解析、Composer 同项选择、普通 fork 和委派/恢复相关静态入口。本卡相对新基线仅改 `docs/624/`。
- 旧独立审查绑定 D1 提交 `528409bf50a5180bd27873a76fca5aa6f23e9829`（文档 SHA256 `84a22b2769a1b1d70a7e4f04c35f96dafe0b35cbe3ff15f300536daa6c3b567b`）。D2 提交 `1f98926e6ed77aac1eebef138a0a227e7461f86b` 纳入其发现。D3 初稿独立审查绑定文档 SHA256 `b56f1eaa8d9c66240e9186a8dae51e29bc4604df85e7bed5aa78369a398ba57c`，结论为可作 P2 规格/待授权设计、无本轮文档阻塞；指出 Bash 前统一读取全部 binding key 的实施风险，主 Agent 已读回实际调用链并在本版明确。具体迁移、受限差异及实施授权仍未完成；最终增量审查证据另行绑定最终版本。
- 本轮未运行 Vega、编译/构建/测试、CI、真实 shell 凭据读取、浏览器或 UI 集成；未改本机权限/运行配置/凭据，无 #624 生产实现、PR 或合并。本轮纯文档，静态检查与独立审查结果须绑定实际最终版本另行记录。
- 下一步：完成 D3 独立审查，冻结授权持久化/降级兼容和受限差异后，向用户展示具体实施范围、风险及回滚并取得明确实施授权；不再要求重新选择已经选定的 P2，也不把选择规格方向当作工具权限批准。

### 版本变更

- D1：记录根因、P1/P2/P3 候选及最初验收；独立安全设计审查完成。
- D2：补持久配置风险、零绑定含义、错误指引证据及具名工具/网络边界。
- D3：记录用户选择 P2 规格方向；取消 P1 专属拒绝预期，明确无 Broker/无注入、新凭据与配置变动；增加固定 Codex 来源和过强结论更正、授权/迁移提案、整卡差异表与分阶段实施/验收。尚非实施授权。

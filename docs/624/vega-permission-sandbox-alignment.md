# #624 · 权限与沙箱语义对齐

## 状态与授权

- 版本：D1，2026-10-26；阶段：分析 / 待安全契约裁决，尚未实现。
- 关联：[Issue #624](https://github.com/puzige/vega/issues/624)、[#58](https://github.com/puzige/vega/issues/58)、[#378](https://github.com/puzige/vega/issues/378)、[#619](https://github.com/puzige/vega/issues/619)。
- 当前代码基线：`dc7e37106f3e97aa22702e3dee6d4c7627bb3359`；任务分支：`feat/624-permission-alignment`。
- 用户要求接卡开始对齐。已授权分析与方案；不把该要求解释为已批准移除凭证隔离、放宽旧会话或修改本机配置。下列候选契约需单独确认，确认前不改生产行为。
- 仓库规则：本地不运行 Vega、不编译或测试；自动检查交现有 PR Actions，真实平台集成交用户手测。Rust 开发测试如使用，必须在合并前留痕并清除，不恢复历史测试集合。

## 目标、范围与非目标

让审批、实际 OS 隔离、权限名称及风险提示一致。Ask/Plan/Execute 是任务能力门禁，ReadOnly/Confirm/Auto/FullAccess 是审批与执行策略，两者不机械映射为 Codex 三个模式。

先解决 Full access 的声明与启动链冲突，并冻结凭证绑定、旧会话和模式切换契约。非 Full access 的完整对齐仍属于本卡后续范围：需要逐项决定读写根、Git、临时目录、环境和网络，不以一项 Full access 修复宣称整卡完成。

本轮不默认改网络策略、不更换凭据存储后端、不迁入 Keychain、不重做 Broker、不扩大进程/Mach 白名单、不修改 MCP 的独立保护路径、不为浏览器添加 `--no-sandbox`。不修改用户配置/凭据，不接管日常浏览器，不用真实 secret 做读取验证。

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

可复核文件：`crates/vega/src/agent_context.rs`、`crates/vega_conversation/src/agent/pipeline.rs`、`crates/vega_runtime/src/agent/tools_exec.rs`、`crates/vega_runtime/src/permission.rs`、`crates/vega_tools/src/bash/mod.rs`、`crates/vega_tools/src/sandbox.rs`、`crates/vega_tools/src/credentials.rs`、`crates/vega_credentials/src/lib.rs`、`crates/vega_ui/src/conversation_stream/composer_actions.rs`。

## 当前行为矩阵

表中网络指 Vega 工具层规则，仍受 OS、外部策略及网络环境限制。“可读”不等于所有文件都能读取，仍受当前用户权限和 TCC 等限制。

| 任务模式 / 权限 | 普通 Bash 审批 | 无绑定，有应用配置 | 有绑定 | 网络边界 |
|---|---|---|---|---|
| Ask / 任意权限 | 能力门禁拒绝 Bash | 不启动 | 不启动 | 无子进程 |
| Plan / 任意权限 | 能力门禁拒绝 Bash | 不启动 | 不启动 | 无子进程 |
| Execute / ReadOnly | 拒绝 Bash，审批规则不覆盖拒绝 | 不启动 | 不启动 | 无子进程 |
| Execute / Confirm | 普通命令确认或精确规则，危险规则优先 | 项目/私有临时目录可写；Git 与配置根受保护 | 同前，另用受控环境和 Broker | 无绑定时 profile 放行网络；有租约时只到本地代理，代理仅放行绑定目的地 |
| Execute / Auto | 普通命令自动批准；危险规则仍优先 | 与 Confirm 相同的 Seatbelt | 同 Confirm | 同 Confirm |
| Execute / FullAccess | 普通命令自动批准；危险规则仍优先 | 项目外与 Git 可写，但仍有配置根/进程保护 Seatbelt | 启动前拒绝，提示切换权限模式 | 无绑定时未施加网络 deny；有绑定时不启动 |

补充：没有应用配置上下文的工具实例，FullAccess 已直接启动 shell；不能以这个特殊入口证明常规应用行为。read/glob/grep/write/edit 还有各自路径、身份、审批和凭证检查；取消 Bash 沙箱不等于这些工具自动改为全盘访问。

## 对既有 Codex 评论的证据收敛

[#624 的补充评论](https://github.com/puzige/vega/issues/624#issuecomment-6406088426) 保留为历史证据，不作为完整运行证明：

- 二进制字符串只能证明包含实现片段，不能证明开关默认值或当前生效策略；配置文件没有某键，也不能排除打包默认、managed requirements 或其他配置层。
- 上游 `main` 未固定到与安装版本等价的完整 SHA，不能作为本次实现的冻结行为来源。当前方案以 Vega 的具体冲突和明确产品裁决为依据。
- “shell 可读凭证文件”在无额外隔离、同 UID、有普通读取权限的前提下是权限推论，未运行读取真实凭据的试验；不得标作 observed。
- 输出脱敏是降低泄漏风险的补充措施，不保证覆盖任意 shell 输出、编码、旁路或直接网络外传。
- 检索位置没有 `.age` 文件不能证明加密存储未启用；未核实完整目录、配置及运行后端，不作该结论。
- Root:Read 与 Vega 默认读取都不是凭证隔离承诺，不能忽略 Vega 的显式凭证根 deny 而宣称二者在文件读取上等价。Claude Code 的完整可读根也未在本轮核实。
- 不将“未找到 GUI Mach 名称”写成 Brave 必然不能启动；也不以网络片段推出所有模式仅能 loopback。

## 验收用例（先于实现计划）

均待候选契约获准后冻结期望；未运行项不记通过。使用合成凭证根/标记，不读取真实凭据。

| ID | 前置 / 操作 | 可观察结果 | 验证路线 / 当前状态 |
|---|---|---|---|
| A1 | Execute + 明确重新选择 Full access，零绑定，正常配置上下文 | 若选 P1/P2：Bash 不经过 Seatbelt；若选 P3：界面明确为受保护模式，不宣称无沙箱 | 启动决策静态审查 + 必要开发回归交 CI + 用户手测；待裁决 |
| A2 | 相同无害命令，切到 Confirm / Auto | 仍走现有受限策略；项目外写入被拒，普通项目内写入可用，Git/配置根保护保留 | 静态差异 + 用户用 owned fixtures 手测；未运行 |
| A3 | Full access + 非空凭证绑定 | P1/P3：启动前清楚拒绝并可切回受限模式；P2：仅按明确获准的取消 Broker 契约执行，不宣称凭证隔离 | 启动前失败/代理不启动的必要开发回归交 CI；待裁决 |
| A4 | Ask / Plan，或 Execute + ReadOnly，包含 once/always 规则 | Bash 仍被能力/只读门禁拒绝；Full access 不越过任务模式 | runtime 决策静态审查 + 必要 CI 回归；未运行 |
| A5 | 危险命令命中、取消、超时、scope mismatch、临时目录身份异常 | 原危险门禁、审计、进程组收拢、输出上限及安全清理不回退 | 受影响边界审查 + 必要 CI 回归；实际进程行为由用户手测；未运行 |
| A6 | 升级前已有 Full access 线程/默认值，恢复、fork、子 Agent 或持续任务 | 未经新契约重新授权不得获得更宽权限；旧运行实例不热升级；恢复/委派不绕过授权 | 迁移与恢复链静态审查 + 必要存储/runtime CI 回归 + 用户手测；待冻结 |
| A7 | 新运行显式选择 Full access 后再切回 Confirm | 权限影响范围明确；切回后下一新运行恢复受限策略，不冒充撤销已发生效果或已派生外部进程 | 状态快照/恢复审查 + 用户手测；未运行 |
| A8 | UI 鼠标/键盘选择、重开线程；有绑定拒绝/失败后恢复 | 选择可发现且持久化；必要风险在授权时可见，不靠 hover 或颜色；错误有可采取动作 | 设计审查 + 用户手测；未运行 |
| A9 | 凭证日志、工具结果、模型投影及 MCP 进程 | 现有脱敏与可信客户端继续保留；MCP 不因 Bash Full access 变化丢失保护 | 独立安全审查 + 受影响 CI 回归；未运行 |

## 待单独确认的候选契约

### P1 · 真正 Full access，保留绑定冲突拦截（最小方案建议）

- 零绑定的 Bash 直接使用当前用户权限，不附加工具层 Seatbelt；保留危险门禁、审计、取消、超时和输出处理。
- 风险：同一用户的 shell 可能读写 Vega 配置和凭据文件、项目外文件及 Git；环境清理和输出脱敏不是安全隔离。不得声称默认网络仍被代理强制限制。
- 有绑定时继续拒绝 Full access 的 Bash，需切回 Confirm/Auto 使用 Broker；不把受保护执行藏在 Full access 名称下。
- 默认 Confirm、旧线程和旧默认值不能因升级静默变成无沙箱；需要新契约的显式重新授权。具体迁移实现必须在代码修改前冻结，不预先选择破坏性的批量改值。

### P2 · 真正 Full access，同时取消绑定冲突拦截

- 除 P1 的风险外，有绑定时也直接 shell，不启动受限 Broker，不保证绑定凭据可用或保密；不能将真值直接注入以补兼容性。
- 是比 P1 更大的安全/兼容契约变化；需另外确认如何告知“切到 Full access 后绑定不可用”。不建议作为最小首轮。

### P3 · 保留凭证隔离，修正名称与说明

- 保留最小凭证 Seatbelt 和绑定冲突拦截；明确标为受保护的广域访问，不再称其“无沙箱”。
- 不完全对齐 Codex danger-full-access；保留 OS 兼容性限制。没有批准真正无沙箱时可选此路线，但不能假装满足 P1。

共同边界：Confirm/Auto、危险门禁、MCP、存储位置、默认网络及凭证脱敏暂不变。旧数据不静默放权。是否采用 P1/P2/P3 是实现前必要裁决。

## 最小实现计划（候选契约获准后执行）

1. 将用户选择及完整影响/回滚写入本规格，先完成独立安全审查；精确冻结 Full access、绑定、迁移及受限模式首轮范围。
2. 按选择修改 `vega_tools::sandbox::spawn_full_access_shell` 与 `bash::execute_bash_inner` 的策略分支；保留共享 runner 生命周期，不整体移除 `ToolCredentialPolicy`，以免连带取消具名文件工具保护或 Broker。
3. 在 thread/default 配置及 runtime 接线中实现明确的新契约授权/旧会话迁移；全量核对恢复、fork、持续任务及子 Agent，不能只改 Composer 文案。所有新持久化变更遵守增量 schema 规则。
4. 调整 Composer/设置的真实状态及必要风险/恢复入口；修改 UI 前读设计规范，不堆常驻说明；未知或未授权状态 fail closed。
5. 按风险选择少量关键开发回归交现有 Actions，留存后清除 Rust 测试及独占接线；最终停写版本独立审查与 required CI。不得以空集 CI 代替行为验证。
6. CI 与审查合格后 PR 合并，卡保留 In review，由用户进行 owned-fixture 手测；继续本卡尚未裁决/验收的受限模式范围，不提前关闭。

## 影响、必要性与回滚

必要性是修正声明与实际执行边界冲突，不是绕过某个浏览器报错。P1/P2 移除的仅是获准 Full access Bash 的工具层限制，仍受 OS 当前用户权限/TCC；但凭证文件同 UID 隔离确实不再成立。

回滚通过 revert 本卡生产实现与兼容的数据迁移策略恢复既有保护；切回 Confirm/Auto 可用于后续新运行，但不撤销已执行命令、外部写入、已发送请求或后台进程。不能通过回滚代码恢复已经泄露的凭据；如发生真实泄露需轮换。未获准前无运行时变化。

## 当前验证与下一步

- 已完成：领取与 Project In progress 读回；同步最新 master；静态核对上述调用链与现行 #58/#378 契约。
- 未运行：Vega 程序、编译/构建/测试、真实 shell 凭证读取、浏览器、UI 集成；未创建生产改动或宣称 CI 通过。
- 下一步：保存 D1 的不可变版本并独立审查安全决策；用户确认 P1/P2/P3 后修订冻结契约，再进入实现。

# D4：Bash 启动边界第一阶段

## 范围（2026-10-27）

基线 `e877e7e76f589ecac00b07ce17f04c2ba62c7bfc`。本阶段只完成可信启动策略、FullAccess 提前分流和 leader 回收后的输出收尾；不把它描述为完整 D4。后续仍须实现 ReadOnly Bash、默认拒绝的最终 profile/受控环境、单次网络/目录扩围、可信持久清理回执和 reset/UI 接线。PR 保持 draft。

## 实现契约

1. 用工具执行边界的 `BashLaunchPolicy` 替换 runtime 的布尔字段与双入口。它是 `vega_tools` 的低层执行 API（与 PreparedBash 同属工具边界），不引入 runtime→conversation 反向依赖，也不复制产品 PermissionFence 类型。暂只含现有 WorkspaceWrite 与 FullAccess；不实现一个名为 ReadOnly、实际上可写的占位分支。策略不实现反序列化或默认值，runtime 仅从已接纳的权限配置选取。原 Ask/Plan 与 Execute ReadOnly 门禁本阶段不变，危险检测、精确规则、Write/Edit/MCP/ComputerUse/ACP 不变。模型输入不能指定 policy/profile/approved。
2. 校验 PreparedBash 的工具实例/项目和取消后，创建宿主管理的私有 TempRoot。FullAccess 在任何 SandboxConfig、preflight/self-test、Broker/CA/lease/alias、ShellCredentialContext 构造前直接生成 `/bin/zsh` 启动命令。配置了 bindings（包括安全存储中可选 ref 不存在）不再触发 CredentialProtectionRequired，也不读取 binding 作为 Bash Broker 的前提。named tools 的 ToolCredentialPolicy 原样保留；Provider/MCP 必需读取、真实存储错误及 owner redaction 仍由已实现的运行级策略负责，不在 Bash 中关闭。
3. FullAccess 保留本阶段已有的继承环境及 `zsh -lc` 行为，仅覆盖本次私有 TMPDIR/TMP/TEMP/TEMPDIR；不声称隔离用户 rc、宿主环境、文件或外部服务。这是显式 P2，不是受限命令失败后的 fallback。底层 direct command builder 不接收 credential context，不能再意外选择 sandbox-exec。受限分支保留旧 profile/环境和 Broker 行为，下一阶段再按主规格整体改动，不能把旧分支宣称为默认禁网。
4. 将命令构造与进程 spawn 分离，构造产物持有本次 credential lease 至监督收尾完成；无需启动 shell 即可在开发回归观察真实产物。执行仍复核取消、TempRoot 身份，保留 process_group、kill_on_drop、超时、输出收集/截断/临时路径脱敏。启动失败不重试、不切换策略。
5. 正常 `child.wait` 成功后只排空输出，不再携带 PGID，也不向缓存进程组发送 TERM/KILL。输出在既有 500ms 内没有 EOF，停止并收束本地 reader，返回独立 `output_incomplete` 错误并保留 TempRoot，不把强制中断读取的部分数据作为成功。反馈明确 shell 已退出但后台工作可能仍在运行，要求核实后再重试，不含路径/命令/密钥。I/O/reader 失败同样不允许临时目录清理。
6. 取消/超时/缺少管道的终止路径只从仍持有的 Child 取得身份，完成 TERM、grace、KILL 后才 wait/reap；不接收调用者缓存的 PGID。身份缺失或 wait 报错不能作为已确认清理。leader/管道完成不证明全部 detached descendants 消失或外部副作用已撤回；本阶段不放宽 reset 对 successful Bash/未知外部效果的保守阻塞。

## 验证

- 不启动真实 shell、Seatbelt、Broker、网络或进程控制的进程内开发回归：真实 FullAccess launch preparation 配置 bindings 时 reader 不被调用、无 CA 文件、产物为直接 shell 且无 alias/proxy 注入；无 bindings 同样直接启动计划；实例不匹配/预先取消在启动准备前拒绝；模型策略字段被严格解析拒绝。
- 用实际异步内存管道验证正常 EOF 保留输出，迟迟未关闭的管道返回 output_incomplete、reader 已结束，I/O 错误保持失败；不以源码字符串匹配或布尔假句柄替代被测决策。
- 本地仅 pinned rustfmt、差异与静态检查；现有 Actions 验证最新 head，非写入者审查本次差异。保留已有 credential/reset/startup/真实 GPUI 导航回归。真实 OS 沙箱及进程树行为仍待用户手测，不能由命令构造测试推出。
- Rust 测试仅供 draft 开发，固定完整源码 SHA 和实际云端结果后，最终合并前移除所有 Rust 测试及独占支持；最终 CI 必须明确 no Rust tests、0 发现/0 执行。

## 状态

按上述规格完成本阶段实现和五项开发回归；尚未运行。回归检查真实 launch preparation/StdCommand 产物及异步内存管道，不启动 native shell、Seatbelt、Broker 或网络。静态核对沿用既有输出收集器去掉末尾换行的契约，不改变生产输出格式。最新 head 的云端执行及非写入者审查待完成；限制和剩余 D4 工作不变。

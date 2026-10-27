# D4：受限 Bash 默认离线与只读执行

## 阶段边界（2026-10-27）

基线 `d1d0a90eb95416f7459672427f3082c0f53af1ec`，Bash 启动边界及失败持久化修正已通过精确 head Actions（84 执行/通过、0 跳过）及非写入者增量审查。本阶段落实主规格 A2/A4/A12 的默认受限路径；单次联网/额外目录扩围及对应 UI、持久清理回执、reset 协调仍后续完成，PR 保持 draft。

## 策略与审计

- `BashLaunchPolicy` 新增真实 ReadOnly。Ask/Plan 的所有权限选项以及 Execute+ReadOnly 强制 ReadOnly；Execute Confirm/Auto 为 WorkspaceWrite，Execute FullAccess 保持已完成的直接启动分支。模型仍不能指定策略或最终 profile，requested_scope 未接通前继续严格拒绝未知字段。
- Bash schema 对所有 run mode 可见。Write/Edit、MCP、ComputerUse、ACP 原门禁不变。
- ReadOnly Bash 使用专门的纯权限判定，不能借通用 Readonly tool 自动批准绕过危险检测。先校验调用目标，再由原 danger detector 判断；匹配即拒绝，不展示可放行危险提示、不保存规则，不接受旧 Once/Always/Auto 绕过。本地只读不保证未来联网后远端无副作用。
- 新审计来源 `sandbox_readonly` 只适用于 Bash：无危险时 Once+无 note/danger；有危险时 Deny+同一 rule_id 的 Deny/no note。codec、runtime↔共享类型映射、live terminal/approval 与历史恢复白名单同时接线；其他工具不能冒用该来源。旧审计语义及失败结果保持兼容。

## 默认拒绝 profile

不批量复制上游 allowlist。固定依据：[Codex base policy](https://github.com/openai/codex/blob/970111f6cb62cc02fe36a82ea7d71b37a2889c97/codex-rs/sandboxing/src/seatbelt_base_policy.sbpl)；本次只选以下集合：

- deny default；允许 process-exec/process-fork、signal(target same-sandbox)、file-read*（仍受 OS/TCC 和下面的强制 deny 约束）。process-info 只允许 pidinfo/setcontrol(target self)，不是 process-info*。
- `/dev/null` 仅 CHARACTER-DEVICE 的 file-write-data。ReadOnly 不允许项目、HOME、私有 temp 写；WorkspaceWrite 允许项目与本次 temp 写。
- sysctl-read 精确项：`hw.activecpu`、`hw.byteorder`、`hw.cachelinesize_compat`、`hw.cpufamily`、`hw.cputype`、`hw.logicalcpu`、`hw.logicalcpu_max`、`hw.machine`、`hw.model`、`hw.memsize`、`hw.ncpu`、`hw.pagesize`、`hw.pagesize_compat`、`hw.physicalcpu`、`hw.physicalcpu_max`、`kern.argmax`、`kern.hostname`、`kern.maxfilesperproc`、`kern.osproductversion`、`kern.osrelease`、`kern.ostype`、`kern.osvariant_status`、`kern.osversion`、`kern.usrstack64`、`kern.version`、`sysctl.proc_cputype`。均是上述固定 base 的显式项；CPU/内存/系统版本/进程自身 ABI 查询，不带 kern.proc、net.routetable 或任意前缀。
- sysctl-write 只 `kern.grade_cputype`，沿固定来源所述 JVM CPU 查询兼容；mach-lookup 只 `com.apple.system.opendirectoryd.libinfo`（账户查询）。没有 PowerManagement、IOKit、PTY、POSIX sem/shared memory、DNS/证书/network helper、Unix socket 或 Apple Events 例外。taskgated 显式 deny。
- Git entry/真实 git dir 最后强制 deny write；已配置 credential root 最后强制 deny read/write。凭据路径仍需可表示，不以绑定是否为空决定保护；named tools 的原路径保护保留。
- 无网络 allow 规则；默认 deny 同时涵盖外网/loopback/入站/Unix socket。此阶段不提供 Direct/Broker 能力或可构造的假批准；待单次 token/审计接通后再增加网络授权策略。受限 run 的严格凭据 reader 不变；dispatch 前仍通过 ToolCredentialPolicy 的原 reader 复核已授权 bindings 快照及其必需值，仅验证并丢弃值，不创建 Broker/CA/alias/lease，也不注入真值。这样保留配置快照失配拒绝，不把离线误作凭据故障豁免。

## 构造与生命周期

- SandboxConfig 一次构造最终 profile 和 controlled_environment，self-test 与 shell command 使用同一 profile/参数/环境。二者统一 env_clear，固定 PATH/LANG、私有 HOME/ZDOTDIR/TMP。shell 固定 `zsh -fc`，不读取用户登录脚本、不继承代理/SSH socket/秘密环境；FullAccess 的继承环境/-lc 不变。
- 自检和实际 spawn 前复核 TempRoot 身份、项目/temp 的既有 hardlink preflight。self-test `/usr/bin/true` 只证明可启动，不能证明 negative OS 边界；加取消与有界等待（不长于命令有效 timeout，最多 2 秒），失败/取消不启动用户命令，不无沙箱重试。失去回收证据保留 temp。
- 对自检 child 的超时/取消只操作仍持有的 Child 并有界 wait，wait 错误不当清理成功。沿用已有 kill_on_drop，正常 leader 退出后绝不发缓存 PGID 信号；不增加 unsafe/libc。
- `-fc`、离线、禁止临时写入或 IPC 会让依赖 rc/cache/GUI 服务的命令失败；这是受限契约，不保证浏览器兼容。反馈真实错误，下一阶段才提供显式单次范围请求；不能用 FullAccess 自动重试。

## 开发验证与交付

- 纯权限矩阵覆盖 3 run modes×4 permission modes、危险与无危险、旧精确规则不绕过 ReadOnly；既有可写工具/外部工具门禁保持。
- 进程内观察真实 profile/命令构造产物：ReadOnly/WorkspaceWrite 的写集合差异、同一 final profile 用于 true 和 shell、固定受控环境、无代理/alias/CA；生成的安全策略不是源文件字符串测试，也不是 OS 执行证据。真实 Seatbelt 拒绝写/网络/IPC 仍交用户 fixture 手测。
- 真实 live approval/terminal 与恢复投影验证新审计来源只允许合法 Bash，拒绝伪造危险/工具/metadata。
- 现有 84 项开发回归保留；本地仅 rustfmt/diff/static，行为执行只用既有 Actions，独立审查绑定停写提交。所有 Rust 开发测试最终留痕后清除，最终 required CI 必须 no Rust tests 0/0；此阶段不是合并点。

## 状态

生产修改及七项开发回归已提交 `4251682ba8c86dd82eb55f8f2e10fedfdae6f20b`；精确 head [Actions 40432406964](https://github.com/puzige/vega/actions/runs/40432406964) 成功：Clippy `121953008519`、Nextest `121953008826`、required 汇总 `121954619874` 均 success，实际 91 发现/91 执行/91 通过/0 跳过，七项新回归和两项真实 GPUI 导航回归均 PASS。非写入者对 `d1d0a90eb95416f7459672427f3082c0f53af1ec..4251682ba8c86dd82eb55f8f2e10fedfdae6f20b` 的冻结增量审查完成、无发现（执行 `01M5XFCZD7B4ASSGD2Z8HZAF94`）；其报告中的 CI pending 是派单时状态，后续云端结果由主 Agent 独立读回。新增回归覆盖实际 runtime prepare/authorize 的 3×4×危险/普通×有无旧规则矩阵、Write/Edit 保留门禁、最终命令/profile/环境产物、自检预取消/foreign temp 拒绝、离线严格 binding、真实 Store live/recovery 审计。未启动 shell/self-test/Broker/网络；环境是否实际继承及 OS deny 强度不能仅由 get_envs/生成文本推出，env_clear 接线按静态审查，native 行为仍待用户手测。未改 dependencies/workflows，未放宽原有 84 项断言，唯一扩充旧测试矩阵是加入新 ReadOnly 策略。

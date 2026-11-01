# macOS 会话进程安全机制可行性论证

状态：研究结论与待确认验证方案；不修改冻结验收，不授权生产实现。用户在实施遇阻后选择“先做安全机制可行性论证”。

## 1. 结论

**现有进程组机制不足以证明任意 shell 命令的安全收尾；本次没有找到已获证据、可直接替换的普通应用级最小方案。会话模式继续不得启用。**

这不是“macOS 绝不可能做到”的结论。它是当前公开源码、公开接口及仓库边界下的证据结论：只确认原 PGID 消失不能自动释放工作区互斥。不能用普通 CI 绿色或有限命令样本声称任意后代已不可继续修改工作区。

最低成本下一步不是增加更多 kill 重试，而是先选择是否允许一个**独立、限时的平台验证研究阶段**：证明某个实际可部署机制在目标系统上完整约束脱组路径，失败即保留阻塞。该阶段不是本文件已获准执行的测试，也不是改现有 CI 的授权。若不接受新增平台机制/权限/兼容性成本，建议暂缓具备任意 shell 写权限的会话能力，保留原阻塞协议；不将此选择冒充 #412 完成交付。

## 2. 核验目标与当前证据

成功判据来自冻结[设计 §5.3](vega-long-command-waiting-design.md#53-停止与未知结果)：正常退出/取消/超时/关闭后，必须能确认受支持命令的安全收尾；不确定则 outcome_unknown，保持隔离；不能新建 daemon/全系统进程隔离来偷换本卡范围。

检查点：`5f2059be09ffe5a1d1df31253b9bb9f158806295`。本论证仅新增文档，未修改其中生产代码。

静态证据链：

1. Vega 用 process_group(0) 创建原 PGID；沙箱允许 fork/exec。子进程不是原组长时可另建进程组/会话，组长限制本身不能约束整个后代树。
2. Apple `kern_prot.c` 的 setsid_internal / setpgid 实现改变 pgrp/session；这些函数本体未出现专用 MAC 检查，**但不能据此说系统无策略入口**：通用 syscall dispatcher 存在 mac_proc_check_syscall_unix，受 syscall filter mask 控制。
3. syscall filter 设置接口在 XNU `proc.h` 的 KERNEL_PRIVATE 边界，不是本次找到的普通应用公开配置 API。公开 SDK sandbox.h 已废弃，也未提供按后代树撤销既有 profile 写权限的接口。尚无证据说明 Vega 现有 SBPL 能完整禁止脱组；不能猜测未公开规则名直接上线。
4. kern_exec.c 的 POSIX_SPAWN_SETSID 直接调用 setsid_internal；只过滤 setsid syscall 并不能覆盖这条路径。POSIX_SPAWN_SETPGROUP 同样是必要检查对象。zsh 作业控制、任意已授权解释器或可执行文件也不能仅靠扫描命令文本约束。
5. 当前实现只检查原组消失，然后可能 cleanup_safe=true 并释放互斥。EOF、主 child 回收、kill_on_drop、租约撤销、临时目录删除均不是“所有后代停止写工作区”的证明。保留的文件描述符、mmap、直接网络访问和延迟副作用也是候选机制必须面对的边界。

没有执行逃逸演示；以上是源码路径与能力缺口，不是目标机器的行为验证。

## 3. 候选机制与取舍

| 候选 | 可确认事实 | 本卡判断 |
|---|---|---|
| PGID + TERM/KILL + 有界等待 | 只能控制/观察同一组；脱组路径存在 | 保留作尽力终止层，不足以提升为安全收尾证明 |
| PPID/libproc 定期扫描 | 提供当前 PID/子进程列表，不提供完整出生事件或原子后代树终止 | fork/退出/重挂父进程与 PID 复用之间有竞态；多扫几轮不能建立完整性 |
| kqueue NOTE_FORK/NOTE_TRACK | 官方 event.h 写明 NOTE_TRACK/NOTE_CHILD 自 10.5 不再支持；NOTE_FORK 不传 child PID | 不作为后代树所有权方案 |
| launchd job/helper | 官方文档的默认剩余进程清理针对 same process group ID | 仍不足以证明脱组后代安全；引入服务也超出原最小实现，不自动采用 |
| coalition | 管理 syscall 要求 privileged coalition；request terminate 设置生命周期终止请求，不是直接 kill 所有任务 | 不能把 API 名字当可靠 kill-tree；无普通应用部署证明，暂不采用 |
| 限制 setsid/setpgid 与 spawn 属性 | 理论上可保留组不变性；需覆盖直接 syscall、spawn 内部路径、继承和 job control | 最值得先核实的窄研究方向，但当前缺公开可部署策略与版本证据；未判定可实施 |
| Endpoint Security 后代客户端 | Apple 文档提供整棵 subtree 事件，macOS 27 起，无 root/TCC 要求，但需向 Apple 申请专用 entitlement；旧 es_new_client 另有 TCC 等要求 | 不能默认用户已有权限/系统版本。事件可丢失且监控不等于终止；需认证身份、阻止再 fork/exec、确认退出及丢失隔离设计，非最小接线 |
| 撤销动态 sandbox 写授权 | 现有 profile 直接授予 workspace 写能力，关闭代理或释放父进程 token 不能证明后代既有权限/FD 已撤销 | 未找到可支持当前合同的公开撤销机制；不以私有 extension 猜测替代证据 |
| VM/容器/独立用户或系统扩展 | 可研究更强隔离域、关闭域或提交隔离副本 | 改变权限、路径、性能或部署模型，超出最小 #412；需要另行设计/授权 |
| 缩窄命令集合、只读模式或每次结束永久隔离 | 可能避免部分 workspace 写入，但命令文本白名单无法约束任意 exec；只读也不证明进程/网络/CPU 已收尾 | 产品验收变化，不能由实现自行采用；“全部终态未知”也不是可用会话功能 |

补充兼容性事实：仓库 computer-use 构件合同写有 minimum_macos=14.0，但本轮未找到足以确定整个应用最低系统版本的独立设置。不能把该构件字段当全应用支持声明。无论如何，采用 macOS 27-only API 都需要明确部署/降级裁决，不能默默改变支持范围。

## 4. 建议的最小研究验证矩阵（尚未执行）

先确定可部署策略，再写一个与 Vega 业务无关、完全自持临时目录/进程的验证方案，**提交用户确认执行方式之后**才执行。不得在用户项目里运行延迟写入演示，不申请 root、不安装服务、不改 TCC/签名/系统配置。禁止把下列真实进程实验混入仓库现有业务回归或修改 CI 来绕过“真实外部 E2E 不新增”的规则。

| 检查 | 必须证明的观察 | 失败处置 |
|---|---|---|
| 直接脱组 | fork 后 setsid、setpgid、新组/新会话均被机制阻止，或仍在不可逃逸可终止域 | 任一路径不受控即不可启用 |
| spawn 路径 | POSIX_SPAWN_SETSID/SETPGROUP、解释器 fork/exec 不绕过 | 不以直接 syscall 被拒就判定通过 |
| 继承与竞态 | 多代 fork、双 fork、快速父进程退出、关闭所有管道、并发退出/取消仍无丢失所有权 | 未知/监控丢事件必须隔离，不释放互斥 |
| 终态安全 | 正常完成、非零退出、timeout、cancel、run 替换、正常关闭有界完成；收尾确认后无延迟写 | 不能只断言 kill 命令成功或 reader EOF |
| 撤销与既有能力 | 已开 FD、mmap、文件别名、已连接网络的行为明确 | 不宣称撤销日志外不存在的能力 |
| 部署兼容 | 每个支持 macOS 版本、真实签名/权限身份；无 entitlement/能力时 spawn 前拒绝 | 不静默回退为不安全会话 |
| 非目标兼容 | 缺省阻塞 bash 不变；正常工具链的 fork/exec/job control 影响有明确记录 | 不为安全测试通过偷偷缩窄产品合同 |

程序化业务验证仍按用户要求仅交 Actions：在可控 adapter 上覆盖准入、未知隔离、审计、cursor/输出、原 deadline 和 stop/drain 状态机。这些能验证 Vega 的决策，但**不能代替平台实验**。真实系统实验按仓库规则由用户手测；若希望在独立云端研究环境自动运行，必须先明确允许的环境与例外边界，不擅自增加真实 E2E 门禁。

## 5. 官方来源与核验方式

Apple XNU 固定版本：`f6217f891ac0bb64f3d375211650a4c1ff8ca1ea`；`main` 当时解析为该版本。先读取 main 内容，随后解析版本并对 spawn/coalition/filter 等关键片段用固定版本复查。源码不是正在运行的 macOS 内核身份，不能冒充部署验证。

- [setsid/setpgid](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/kern_prot.c#L478-L637)
- [spawn 内部 setsid 路径](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/kern_exec.c#L4480-L4503)
- [syscall 通用策略分发](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/dev/i386/systemcalls.c#L183-L193)，检查仅为该源码架构路径证据，不推断为全架构/目标系统行为。
- [KERNEL_PRIVATE syscall filter 接口](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/sys/proc.h#L557-L582)
- [kqueue 后代跟踪不受支持](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/sys/event.h#L255-L275)，[废弃标志说明](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/sys/event.h#L362-L369)
- [coalition 管理权限](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/sys_coalition.c#L217-L249)，[terminate 生命周期语义](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/osfmk/kern/coalition.c#L2174-L2230)
- [launchd.plist AbandonProcessGroup](https://github.com/apple-oss-distributions/launchd/blob/d448a1c8f70a61202f8705f94337f686b87c30c4/man/launchd.plist.5#L364-L368)，历史公开源码，不作为现代系统未变的保证。
- [es_new_descendants_client](https://developer.apple.com/documentation/endpointsecurity/es_new_descendants_client(_:_:))；[所需 entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.endpoint-security.client)；[旧客户端 TCC 要求](https://developer.apple.com/documentation/endpointsecurity/es_new_client(_:_:))；[事件丢失检测](https://developer.apple.com/documentation/endpointsecurity/es_message_t/global_seq_num)。来源读取为 Apple 官方 DocC JSON，网页内容可能后续更新。
- 本机 SDK sandbox.h 仅静态读取：MacOSX15.5.sdk，SHA-256 `5baa98c212110d6323f7d73f4b42b0072a4c66a826a46c8bfd8fb850305f9416`；它不提供全面枚举私有 API 的证明。

实际执行：网络只读获取官方源码/文档、仓库和 SDK 文本检查。未编译/执行 Vega、测试、真实进程探针、服务安装或权限修改；未取得独立审查结论。没有生产实现变更，没有把研究结论当作安全验收通过。

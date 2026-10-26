# MCP Node 启动的自身 PID 信息兼容修复

## 根因与证据

2026-10-26 用户限时诊断对照：相同 Homebrew Node、临时 HOME、固定 PATH、工作目录与隔离策略下，`node --version` 使用当前策略退出 -5、无 stdout/stderr；仅追加 `(allow process-info-pidinfo (target self))` 后退出 0、输出 `v26.8.1`。脚本已静态核对，两组只差该规则。

此前 Vega 子进程与用户诊断的 macOS 崩溃报告均显示 SIGTRAP，libdispatch 报 `Unable to get the unique pid (error)`，栈位于 Foundation / 系统日志初始化。当前 `(deny process-info*)` 阻止自身 PID 信息查询，导致 Node 早于 npm 与 MCP 握手崩溃。外层 Agent 沙箱不允许嵌套 sandbox-exec，不能把其 sandbox_apply 拒绝当作产品故障。

## 契约与验收

依据 [Credential Broker §6.2](vega-credential-broker-design.md#62-vega-必要配套) 允许受保护进程获取自身 PID 元数据，不允许读取其他进程/Broker。

| 场景 | 预期 | 验证边界 |
| --- | --- | --- |
| Node 系统初始化 | 自身 PID 查询不再因规则被拒而 SIGTRAP | 用户对照已验证；发布后 MCP 集成仍待手测 |
| 其他进程信息 | 保留 process-info* 默认拒绝；例外仅 pidinfo、target self | 差异与独立安全静态审查；真实系统负向验收待用户 |
| 凭据文件 / taskgated / 网络 | 凭据目录读写拒绝、taskgated 拒绝、代理租约出口限制不变 | 差异与调用链审查；不声称本轮已重新执行系统隔离验收 |
| 环境和协议 | 临时 HOME、环境清理、代理、参数原样传递、连接期限与握手不变 | 差异检查 |

## 实现范围

只在 `ProtectedProcess::command` 的 Seatbelt 策略增加自身 process-info-pidinfo 例外。该共享执行器的所有调用者适用同一有限例外。不关闭沙箱，不允许通用 process-info，不读取或更改用户 MCP 配置，不顺带修参数或诊断 UI。

## 验证与交付边界

本地仅格式、差异和静态检查，不编译、运行 Vega 或执行测试。最终不可变提交接受非实现者独立审查及完整 Actions required CI。此改动不新增 Rust 测试或专属测试接线；云端实际发现与执行数单独记录。

用户对照证明的是 Node 启动阻断的因果关系，不等同于完整 MCP 连接成功。发布后需重新测试连接并确认工具发现；参数尾部空格、首次下载与协议问题若仍出现，应依据新的失败证据另查，不宣称它们已经解决。

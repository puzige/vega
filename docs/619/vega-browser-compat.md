# 独立 Brave 兼容模式（C1）

## 状态与变更依据

- Issue：<https://github.com/puzige/vega/issues/619>。
- 分析基线：`23cb3a48d5d5c981096ac1a4f6fa6b31e1596177`。
- 用户已批准实施“关闭 Chromium 内层沙箱、保留专用外层沙箱”的兼容路线；不再将禁止 `--no-sandbox` 作为本路线验收条件。
- 此变更只适用于明确授权的独立实例，不授权修改通用 Bash、日常 Brave 或系统权限设置。
- C1 取代 D2.1 的内层沙箱要求及派生 AB 前置；其余授权、数据隔离、控制入口和真实清理要求保留。
- 当前仅完成静态可行性核对；无生产代码修改、浏览器运行或测试结果。新增 WebSocket 依赖尚待批准，生产实现未开始。

## 目标与非目标

Vega 内使用固定版本原样 agent-browser（AB）控制全新 Brave 实例，完成启动、网页快照、真实 ref 点击、核对导航和仅清理本实例。

不接管已有浏览器，不读取或复制真实 Profile/Cookie，不升级全局 AB，不允许任意 CLI、脚本、插件、状态恢复、自动发现、raw CDP 或上传路径。既有扩展浏览器模式保持原行为，不能将其成功算成本模式通过。

兼容模式不是与 Chromium 原生子进程沙箱等价的保护。renderer 被攻陷后可能获得专用外层策略的权限；授权界面必须明确此项取舍。仍不承诺抵御被攻陷的 Vega 宿主或另行授权的无限制同 UID 程序。

## 已确认的控制入口缺口

固定 AB `eb05921bad874cd2a1b4fa5d1149f1ed26576cae`（0.36.0）：

1. daemon 会尝试开启 stream HTTP/WS listener；失败非致命。端口 0 不是禁用。
2. 普通 Chrome launcher 固定使用 TCP debugging，AB CDP transport 是 TCP WebSocket，不支持原生 debugging pipe。
3. 仅在 AB 外层策略禁止 TCP bind 可以抑制 stream，但继承相同策略的普通 Chrome launcher 也无法建立 CDP。
4. 仅鉴权结构化前端不能保护另一个裸 CDP listener；loopback、随机端口、0700、Origin/Referer 都不是对网络可达 Bash 的认证。

所以不实施“给普通 Bash 命令补一个 --no-sandbox”。最低限度需要额外的私有控制传输。

源码：
- <https://github.com/vercel-labs/agent-browser/blob/eb05921bad874cd2a1b4fa5d1149f1ed26576cae/cli/src/native/daemon.rs#L100-L144>
- <https://github.com/vercel-labs/agent-browser/blob/eb05921bad874cd2a1b4fa5d1149f1ed26576cae/cli/src/native/cdp/chrome.rs#L460-L475>
- <https://github.com/vercel-labs/agent-browser/blob/eb05921bad874cd2a1b4fa5d1149f1ed26576cae/cli/src/native/cdp/client.rs#L44-L53>

## 实现结构

```text
Vega task/run + epoch grant
  └─ OwnedBrowser supervisor（受信 native 能力）
      ├─ 固定 Brave：专用 Seatbelt + --no-sandbox + debugging pipe
      ├─ 鉴权 WebSocket ↔ CDP pipe broker（宿主持有）
      └─ 固定原样 AB：独立专用 Seatbelt + 禁止 TCP bind
          └─ 私有 UNIX command socket（仅宿主前端访问）
```

Brave 与 AB 由宿主分别首次施加专用策略，不从通用 Bash 再嵌套启动。Brave 不开放 TCP CDP；AB 不开放 stream/inspect TCP 服务。broker 是唯一预期控制 listener，必须在任何 CDP 转发前完成鉴权。

AB 的 WS URL 可携带宿主生成的高熵凭据；普通 `--headers` 是网站请求头，不得用来传控制器凭据。凭据只由受保护启动配置/内部 IPC传递，不进入模型、argv、普通日志或状态结果。原样 AB 是否会在错误输出回显凭据必须实测并在宿主输出层拦截；不能仅靠正则日志脱敏提供保密边界。

会话根置于现有通用工具保护根内，每实例独立目录，包含 fake HOME、Profile、tmp、AB socket/config 与预先创建的未完成标记。0700 仅是基础权限；启动前必须确认本任务所有通用工具入口确实受保护根围栏约束。缺失则拒绝，不退回共享 /tmp、不扩大 Bash 授权。

AB 与 Brave 获得的文件读写权仅限本次根、已校验可执行文件/资源和必要系统运行资源，不得读凭据根其余内容、项目或真实 HOME。使用 default-deny 专用策略，不能复用 `(allow default)` 的 Bash 文件访问面。路径 canonical/no-follow/目录身份复核是创建、使用与删除的共同前提。

网络策略须在实现阶段明确区分网页外连、AB 到 broker、系统 DNS 和本地控制；不通过放开所有 loopback 解决连接问题。不能把 hostname 检查写成 DNS rebinding 或 OS 网络防火墙证明。现有任务带凭证代理或更严格网络限制时 v1 拒绝，不通过 native 入口直连绕过。公网/本地地址策略拿不到实际拒绝证据则不开放模式。

## 模块范围

| 模块 | 改动 |
|---|---|
| `vega_runtime/src/computer_use/owned_browser/`（新增） | grant 后启动；双 Child 监督；固定 argv/env；专用 Seatbelt；CDP pipe/鉴权 broker；私有 AB IPC；ref/state/cleanup |
| `computer_use/{mod,control,backend,components,ownership,diagnostics}.rs` | 增加独立兼容模式，不改 extension 默认；模式绑定授权与会话；真实失败及待清理状态 |
| `agent/tools_exec.rs` | Execute 且非 ReadOnly才可请求；Auto/FullAccess 不豁免专用 grant；撤权/epoch 检查 |
| `vega_conversation::types` 与现有 UI 控制卡 | 共享模式/授权数据；明确降级风险、允许/拒绝、停止及 cleanup_pending；键盘可达 |
| `vega_tools` 保护策略桥接 | 仅提供内容无关的保护前提/根身份；不暴露 reader/secret，不放宽 Bash profile |
| 组件检测及打包 | 固定受支持 AB identity/version/digest；无自动安装升级；不引入派生 AB |
| `docs/619/` | 保存协议、策略、验收、精确版本与实际结果 |

不直接照搬旧 `backend::environment` 的真实 HOME，也不把旧 `ManagedClient::shutdown` 对单个 child 的退出当成全树清理。本路线使用私有 AB IPC，不要求为了复用 MCP 而新增通用命令服务。

## 授权与操作契约

先展示独立兼容模式风险并获得 run/epoch/mode/组件身份/策略绑定的单任务授权，再创建会话根或进程。拒绝/取消时零 Profile、零浏览器、零控制器；只有无副作用的静态身份校验可前置。

六个逻辑操作：

- `start`：不接受 executable/profile/flags/endpoint；授权后执行固定启动。
- `open`：只接受规范化 HTTP(S) URL，拒绝 URL 凭据及其他 scheme。
- `observe`：有限页面快照、URL、标题和新 refs；页面数据不授予能力。
- `click`：仅当前 observation 的真实 ref；绑定 run/epoch/session/document；不接受 JS、selector、文件路径。
- `status`：仅观察已持有会话；不得 ensure_daemon、重连、恢复或启动。
- `close`：仅清理已持有会话；不存在时不隐式启动。

导航、换文档、暂停/撤权后 refs 失效。已派发而结果未知返回 outcome_unknown，不重放。Pause/Stop 先撤销能力及队列，后进行独立期限清理；恢复需新 grant，不复用旧 Profile。

原样 AB 通用命令、eval、插件、restore、inspect、cdp_url 不向模型透传。宿主对私有 socket 发出的命令必须固定枚举并严格校验未知字段。

## 监督与安全判据

1. FD 默认 CLOEXEC，仅在目标 child 内映射 Brave FD3/4；MCP/其他并发 spawn 不继承。关闭所有多余副本，覆盖映射碰撞和部分启动失败。
2. Vega持有 Brave和 AB 的真实 Child，分别建立专属进程组。cleanup 不依赖已取消的动作 token 或被请求占用的锁。
3. 关闭先发精确实例命令，再按归属明确的进程进行有界停止和 reap。主进程已回收后不得盲用缓存 PGID。helper 无法确认退出时保留 cleanup_pending，不删根、不释放新实例名额。
4. 宿主 SIGKILL 后仅尽力断管收尾；重启读取预先存在的未完成记录，不能按历史 PID 杀进程。
5. 外层文件围栏是本路线核心防线：即使能诱使页面进入 file 或取得 raw CDP，也必须无法触及会话外文件。不能用“UI 没有文件按钮”代替系统拒绝。
6. 必须验证重定向、popup、iframe/worker、非 HTTP scheme、下载和文件选择器在副作用前被拒绝/约束；外层策略不能阻止的外部应用调用必须有独立硬阻断。事后关页不是执行前拒绝。
7. 专用策略允许 self process-info-setcontrol 的必要范围需独立论证和记录；不修改通用 Bash 的相同规则。--no-sandbox 不自动修复线程命名 EPERM。

## 实施顺序及当前依赖关卡

1. 固化 C1 与当前用户授权，保留 E2/D2.1 历史；不将风险取舍称为原等价安全。
2. **当前暂停点：依赖批准。** 宿主需要维护良好的 WebSocket server/frames 实现。仓库目前没有 tungstenite 系列依赖。建议批准 `tokio-tungstenite = =0.30.0`，关闭默认 features，仅 `handshake`；只用于 loopback broker，无 TLS、外部连接客户端或插件。crates.io 静态元数据：非 yanked，MSRV 1.85；不等于本仓编译/兼容验证通过。transitive 依赖需锁定并审查。不得手写 WebSocket 协议规避审批。
3. 获准后先实现固定策略、pipe/broker、真实 Child 监督及失败收尾；做启动—无副作用控制探针—关闭闭环。此阶段不向模型开放浏览功能。
4. 在既有云端按风险验证解析/鉴权/边界状态；平台 Seatbelt/Brave 集成由用户手测，不本地执行、不新建真实外部 E2E 门禁。
5. 完成文件/控制端点/网络/线程兼容与 helper 清理判据，再接六操作与授权 UI。
6. 非实现者审查、最终 head required CI、合并后 In review 等用户真实链路验收；未通过不 Done。

## 验收矩阵（全部待实施/待验证）

| ID | 必须观察到的结果 | 验证层 |
|---|---|---|
| C1 | 拒绝/取消/ReadOnly/Plan 零实例；Auto/FullAccess 不绕过授权；旧 epoch 拒绝 | 云端行为验证 + UI 用户手测 |
| C2 | 独立 HOME/Profile、固定身份；真实浏览器无变动，无凭据/环境继承 | 静态 + 用户平台手测 |
| C3 | Brave 明确 --no-sandbox 且专用外层生效；无全局 Bash策略变化 | 静态 + 用户平台拒绝证据 |
| C4 | TCP CDP及 AB stream/inspect 不监听；未鉴权 broker 请求零 CDP转发；私有 socket/凭据对通用工具不可达 | 云端协议验证 + 用户平台手测 |
| C5 | 会话外 canary 文件读写、项目写入、真实 Profile/凭据访问被拒绝；不使用真实秘密作探针 | 用户平台手测 |
| C6 | 网络/localhost/内网策略、重定向/弹窗/iframe/worker/外部 scheme不产生禁止副作用；下载/chooser拒绝 | 用户平台手测 |
| C7 | 分帧、限额、错误/取消、FD泄漏与部分启动回滚正确 | 云端必要行为验证 + 用户平台手测 |
| C8 | snapshot 真正读取→当前 ref 点击→核对 URL与目标正文；空正文不冒充成功 | 用户 Vega 真实验收 |
| C9 | controller空闲崩溃/Brave崩溃/启动失败/清理超时均正确收尾或明确 pending，不误杀、不误报 | 云端状态验证 + 用户平台手测 |
| C10 | 停止/撤权失效 refs；已确认全实例退出才删根释放名额；SIGKILL恢复不依赖旧 PID | 云端状态验证 + 用户平台手测 |

自动验证仅交 Actions；若使用开发 Rust 测试，按仓库政策保存原始源码/完整 SHA/实际结果并在合并前清理，最终 required CI确认 0发现/0执行。保留现存 Python回归。不报告未执行为通过。

## 回滚

撤销兼容 grant、禁用本模式，精确收尾已确认归属的实例；未知状态保持 pending。回退本次 Vega组件/依赖，不改全局 agent-browser、不恢复或删除日常 Brave数据，不修改系统 TCC，不修改通用 Bash策略。

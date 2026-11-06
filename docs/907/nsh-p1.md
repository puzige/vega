# NSH P1 — 宿主对齐与最小嵌入

## 决策与范围

用户在 P0 后明确批准直接实施，不做性能对比，不以收益比较作为继续门槛。本规格替代 #907 初始计划中的性能对照要求；功能、安全、独立审查和 required CI 不变。P1 不自动扩展为 P2 设置页持久化或全量会话迁移。

## 验收

| 前置状态 / 操作 | 可观察结果 | 验证 |
| --- | --- | --- |
| 启动原 Vega | 原工作区、会话、Composer、终端仍走现有业务路径；UI 活动类型统一为 Fast | Actions 编译/Clippy，用户手测 |
| 打开 NSH 预览 | 可通过 Vega 菜单 / 应用快捷键打开独立窗口，挂载随程序编译的纯 Rust root；不读取额外用户数据 | 静态 + 用户手测 |
| 主界面切换浅深主题 / 语言 | 打开的预览同步宿主主题、locale 和标签；不建立另一份持久配置 | 静态 + 用户手测 |
| 预览内输入并请求宿主回显 | 仅在内存中传递有界文本，宿主返回后更新结果；不执行文件、网络、进程操作 | Actions 类型检查 + 用户手测 |
| 关闭 / 重开 / 重载预览 | 引擎及所属任务终止，旧会话回包不污染新页面；后台 Agent 与 PTY 不受影响 | 静态 + 用户手测 |
| NSH 失败 | 显示可操作的重载入口，不冒充已成功；主工作区仍可操作 | 静态 + 用户手测 |
| 最终提交 | required CI、独立审查；Rust 开发测试按政策留痕清理 | Actions + 独立审查 |

## 固定依赖与允许例外

- GPUI Kit `=0.7.3`，features `component, assets, gpui-fast`。
- `gpui-fast` / `gpui-fast-platform` 固定 `=0.1.9`，共用 registry package identity；Kit 的 pre 依赖允许保留，但不混用两套活动 Window/Entity。
- `nsh-platform-gpui` / `nsh-core` / `nsh-base` `=0.4.0`，禁用 JS/webview/video/dev 默认特性；纯 Rust root 不引入 QuickJS。
- NSH 固定源码基准 `e626a8dd8f8d8b277b9507cfd5ac74d0e67020f3`；Kit 基准 `67265a36e260ffe03c5aaebdcd8d654dbf892eab`。
- 接受 Kit Fast 上游启用的 `runtime_shaders`。不自行移植后端，不作 unsafe 类型转换。
- 仅做必要 API 适配，不升级到 Kit 0.9 或跟随 NSH main；transitive 版本以提交的 Cargo.lock 为准。

## 最小实现计划

1. 更新统一 workspace facade 和锁文件，静态核对实际 API；本地不编译、不运行 Vega、不执行测试。
2. 增加应用级 NSH 预览窗口，快捷键 `cmd-alt-n`；单一活动预览，重复动作激活既有窗口。保留原工作区和设置页。
3. 通过发布版 `AppOptions.context` / `EngineContext` 传入专用 tokio watch 通道 / sender，NSH 组件任务在 engine thread 内接收宿主快照；操作经容量 8 的有界通道交回 GPUI，输入/回包最多 256 Unicode scalar。不得用仅面向脚本的 HostChannel 代替 Rust 通道。
4. 主题复用 Vega token，语言复用现有 i18n；输入长度限制，只有明确的回显命令。任务随 view/engine 生命周期释放，不连接 Agent/PTY 所有权。
5. 按云端原始证据修必要兼容问题；required CI 与独立实现审查结束后合并，保持 Issue 开放并等待用户手测。本轮不造机械回显或源码字符串开发测试；主要风险是宿主 API/绘制输入兼容，按云端编译、独立静态审查及真实用户手测验证。性能比较和 Agent 自产 E2E 均不作为门禁。

## 非目标与恢复

不做性能对照、TSX/QuickJS、下载包、模型生成 UI、文件系统/网络授权、新数据表、配置持久化、会话流替换或终端重写。预览关闭即可回到原业务 UI；宿主后端升级回滚按本 PR 整体回退，不修改用户数据。若统一依赖解析失败或需要长期 fork，报告具体证据，不用绕过安全门禁的方式推进。

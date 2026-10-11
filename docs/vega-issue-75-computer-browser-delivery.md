# Issue #75 · 受管桌面与浏览器控制交付记录

关联：[Issue #75](https://github.com/puzige/vega/issues/75) · [冻结规格](vega-issue-75-computer-browser-use.md)。功能点 A3-04。

## Freeze

- 本记录的核心自动验证时间：2026-10-11 UTC / 2026-10-12 Asia/Shanghai。
- 核心实施分支：`feat/75-computer-browser`；核心、UI 与打包分别独立 worktree 实施，由主代理集成单张卡的 PR。
- 环境：macOS 15.8.1 arm64，rustc 1.98.0、cargo 1.98.0、git 2.55.0。
- 发行组件固定为 Cua Driver 0.34.0、Node 24.21.0、Playwright MCP 0.0.83；资源与平台边界见规格 R29–R32。
- 真实桌面、浏览器、模型、TCC 与安装验收：NOT RUN。进程内回归不能代表真实系统集成已通过。

## 实现与证据边界

`vega_runtime::computer_use` 负责单 run 授权、单一 lease、epoch 撤权、接管/继续/停止以及受管后端。桌面发现使用只读元数据，选择后更换为单窗 bounded daemon；输入与截图只针对授权窗口。浏览器只接受扩展连接中的唯一 current http(s) 标签，允许该目标的普通点击导航，popup、多目标与不可验证结果撤销连接。

专用 managed MCP transport 限制帧、文本、结构化结果与图片；取消或协议失败后连接不可复用。进程由 Vega 直接持有和回收，停止不关闭用户应用或浏览器标签。接管先撤 epoch，直到旧执行通道关闭才进入 Paused；同一 run 的旧审批和晚到图像不能恢复动作权。

工具观察只在本次 run 保存有界缓存。模型上下文携带 Tool 来源、call ID、观察 ID、尺寸及不可信内容说明；Responses 使用函数结果内容数组，Chat Completions 在整批工具结果后投影多模态 wire 段。持久化仅保留脱敏动作摘要、输入长度与摘要值；不写截图或完整 AX/DOM 到历史。压缩会使观察失效，继续操作须重新观察。

## 核心定向回归

| 规格 | 进程内证据 | 状态 |
|---|---|---|
| CU01、CU09 | Native 模式/能力开关门禁，禁用桌面工具无 discovery 副作用，普通 MCP 仍逐次 Once | PASS |
| CU03–CU06 | 指定目标审批状态、旧 epoch 回调拒绝、暂停立即继续拒绝、停止期间其他 run 不得抢 lease、旧观察失效 | PASS（状态/控制边界） |
| CU05、CU13 | 模型尚未完成时接管会完成后台清理并阻塞 final，用户继续后运行恢复；部分写取消、错误 RPC ID 均关闭专用连接 | PASS（mock provider/内存 transport） |
| CU07 | Cua typed target 与 legacy 字段互斥；元素 token 可见且精确匹配；显示坐标映射回上游 PNG 像素 | PASS（固定合同 fixture） |
| CU08 | 仅一个 current 标签、http(s) 目标、普通导航结果，popup/多标签/空白页拒绝，DOM ref 完整匹配 | PASS（固定合同 fixture） |
| CU10、CU14 | 专用媒体解析与普通 MCP 分离，真实图片解码、MIME 伪造拒绝、缩放尺寸；缺组件、版本错、路径错、过大 manifest 拒绝 | PASS（纯函数/临时文件） |
| CU11 | Responses 工具图像数组；Chat Completions 完整工具批次后投影，保留真实 call ID | PASS（provider request fixture） |
| CU12 | 连续 20 次观察后旧上下文正文/图片清除、缓存上界、失效后晚图拒绝；相同编码长度的变图改变 fingerprint；用户图片旧估算合同保持 | PASS（runtime/context） |
| CU02、CU16 | 设置持久化、运行控制条、查看观察与关闭/删除入口 | UI 独立实现，集成证据由主代理补充 |
| CU15 | 固定依赖、打包路径、哈希和签名责任 | packaging 独立实现，集成证据由主代理补充 |
| CU17、CU18 | TextEdit 与登录 Chrome 上的真实观察、操作和接管 | NOT RUN，用户手测 |

初始 provider 失败命令：

```text
cargo nextest run -p vega_runtime issue75_provider --no-fail-fast
Summary [   0.019s] 2 tests run: 0 passed, 2 failed, 275 skipped
```

退出 100。失败原因分别为 Responses 函数结果仍是文本、Chat Completions 未生成批次后的观察段。原始日志名 `vega-75-provider-red.log`，SHA-256 `9cf8bc33d932771815457df868eac8648782633c8eb8cab2cf0e829558262b27`。

生命周期回归首次失败：`vega-75-core-final.log`，20 项中 19 通过、1 失败，退出 100。新增用例发现 monitor 在订阅后的第一次状态变更已发生时先等待 watch，导致接管一直停在 Pausing。修复为先处理当前状态，清理在 backend 锁内复核 epoch/phase，拒绝旧清理关闭新连接；随后同一断言通过。失败日志 SHA-256 `65b5074066b4606aeb97fa50d7471f746c087454fe4f5abf770dad41fb731a66`。

最终命令及原始输出摘要：

```text
cargo nextest run -p vega_runtime -p vega_conversation -p vega_mcp issue75 --no-fail-fast
Nextest run ID d9b432ed-b2c5-4758-9859-3d50a0413333 with nextest profile: default
Starting 22 tests across 13 binaries (907 tests skipped)
Summary [   2.287s] 22 tests run: 22 passed, 907 skipped
```

退出 0。原始日志名 `vega-75-core-final-4.log`，SHA-256 `a400d637d3304729899a303fcd201b4bd431e47d95901ba7bd2f0899cf8c3623`。测试时三个核心 crate 的 diff SHA-256 为 `47b5d0f0b0913a04b37ea0318ef2171ab54880e4f80be792c987d73b7987e479`；该值不包含交付文档，避免自引用。

仅执行本卡定向测试；没有本地 workspace 全量测试、全量 clippy 或真实外部进程 E2E。云端 `pr-check`、PR 与合并身份由主代理在集成后补充。

## 已接受限制与手测步骤

- macOS arm64 14+ 才启用能力；不提高 Vega 整体系统下限。Settings 的组件检测成功仅表示随包文件可用，系统权限、执行器协议和扩展连接在首次使用时检查。
- 浏览器扩展由用户安装并选择目标。首期限制的是执行器连接中的一个 current 标签，用户 Chrome 可以保留其他未连接标签。上游没有不可变 tab ID，同一 index 在两次检查之间被替换的竞态无法全部识别；不宣称浏览器级强隔离。
- 上游 `browser_tabs(list)` 调用 `ensureTab`，目标已关闭时可能先创建空白页。Vega 拒绝 about:blank 并断开，不主动关闭该页；不能保证用户关闭目标后绝不出现新空白标签。
- 桌面要求模型支持视觉；BrowserObserve 默认只将 DOM 放入模型，用户仍可查看截图，显式 include_image 才向视觉模型发送图像。页面 canvas 无 DOM 引用时交给用户接管，不提供 JS 或坐标浏览器通道。
- 截图最多最近两项、总计 8 MiB，最长边 1600；必要时 JPEG 重编码。图片 token 统计为尺寸估算，不是账单真值。截图与完整观察不恢复到历史。
- Cua fallback 与 Playwright 会使用短命文件；均限制在 owner-only run 目录，读取后清理，进程退出后清理目录。不把这一实现描述为严格无磁盘写入。
- 取消不能撤回已交给 OS/网页的事件；在途结果不可验证时标 outcome_unknown，不自动重试。接管等待执行通道回收，不在清理中允许继续或释放 lease。

用户在合并后的正式 Vega.app 手测：

1. 开启桌面能力，选择 TextEdit 的指定窗口，观察并输入无敏感测试文本；确认其他窗口未被操作、权限弹窗归属可解释。
2. 在动作期间或模型响应期间点“接管”；确认先进入 Pausing，清理后可手动操作；继续后必须先获得新观察，旧引用不得重放。
3. 安装匹配的 Playwright MCP 扩展并选一个已登录测试标签，完成 DOM 表单、普通链接导航；确认复用登录态、目标明确、用户可随时接管。
4. 打开 popup、切换连接目标或关闭目标；确认停止改控、显示可恢复的连接错误，且不会关闭用户 Chrome。关闭目标可能产生上述上游空白页残余。
5. 停止运行、关闭窗口或删除任务，确认没有新增动作；重启 Vega 后能力开关可恢复，但授权、连接与截图均不恢复。

与冻结规格偏离：无；上述限制均在 R13、R21、R24–R28 和实施裁决中明确。用户手测通过前不关闭 Issue、不标 Done、不清理本卡 worktree。

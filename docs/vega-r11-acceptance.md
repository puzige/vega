# R11 — ZCode 按钮对照与首批功能验收

日期：2026-09-06。主 Agent 负责参考端点验、规格、独立审查、集成和原生客户端验收；三组专用 Astra medium 分别实现命令面板、交互终端和输入区动作。未使用 pi。

## 范围与代码来源

[103 项对照表](vega-r11-zcode-button-matrix.md) 覆盖主界面、侧栏、输入区、右/底栏、设置、扩展、自动化和帮助。参考端逐项区分「点验」与「仅展示」；未改变 ZCode 的账户、供应商、授权、插件或自动化配置。右栏参考为辅助对话、终端、浏览器；普通文件从 Cmd+K 打开，未据此虚构文件树/可编辑代码编辑器。

本轮完成三张实施卡，不代表 103 项能力全部实现：

| 范围 | 本轮行为 | 规格与分支证据 |
|---|---|---|
| 搜索与文件 | Cmd+K 四分类面板，真实任务搜索、项目文件查询、只读预览和 Finder 入口；Cmd+O 打开并激活新/已有项目；Cmd+N 真实创建任务 | [规格](vega-r11-command-palette.md)、[交付](vega-r11-command-palette-delivery.md) |
| 交互终端 | 真实 PTY/login shell，多会话、项目隔离、右/底栏移动、显隐、键盘、Ctrl+C、尺寸更新、退出/重启、关闭回收、当前屏幕复制 | [规格](vega-r11-terminal.md)、[交付](vega-r11-terminal-delivery.md) |
| 输入区 | 可见 Stop，准备/流式取消和持久化终态；加号菜单、项目文件引用、/ask /plan /execute；模式持久化确认后仅消费命令前缀 | [规格](vega-r11-composer-actions.md)、[交付](vega-r11-composer-actions-delivery.md) |

集成 worktree：`/Users/puzige/Workspace/worktrees/vega-review-integration`，分支 `codex/vega-review-integration`。**最终应用源码 `4a10b74`**；首轮联合源码 `50e94e2`；原生反馈修正后的源码 `3782102`；最后一次完整 workspace 源码 `f0ea6e5`。4a10b74 相对 f0ea6e5 仅改变分支选择器弹层几何及对应规格，最后受影响范围和原生复验见下方，不把早期全量结果当作最终源码结果。

原生反馈修正包括：空任务名称显示、Cmd+N 调度、Cmd+O 激活与取消、搜索面板高度、Markdown 列表换行、文件候选框前景定位、窄栏选中标签及关闭按钮可见，以及分支弹层独立宽度/短列表高度。保留 R10 的设置集中统计与本地凭据方案，不恢复对话成本指标或 Keychain 依赖。

## 原生验收环境

- 自有验收副本：`/Users/puzige/Workspace/vega-review-20260905/native-r11/Vega.app`。每次替换前通过 Cmd+Q 退出已识别旧 Vega，进程清单确认零实例，再签名校验并只启动此副本。安装版未替换。
- 真实 macOS 窗口、键盘、鼠标与原生目录选择器；960×600 浅/深色、1280×750 深色实际点验。CUA 图像与操作记录保留在本任务工具历史。
- 自有项目 `r11-native-project` 和 `r11-open-workspace-check` 位于外部验收目录；通过生产注册 UI 加入现有 `ai.vega` 应用数据库，没有用脚本注入业务行。只对自有项目做创建任务、模式选择和 shell 写入。
- 配置为自有 R10 配置副本，未复制任何凭据；`ZDOTDIR` 指向自有空目录。因此真实 PTY 验收不等于加载用户 shell 启动配置的验收。
- 供应商页显示「需重新输入 API Key」。没有读取/迁移 Keychain，没有发送真实供应商请求；Stop 的网络边界使用 MockProvider，不能称为原生远端模型流式取消通过。

## 实机结果

| 操作 | 观察结果 | 源码 |
|---|---|---|
| Cmd+N | 新任务行出现；自有项目任务数由 1 增到 2，数据库读回确认；消息数仍为 0 | 3782102 |
| Cmd+K 全部/任务、空名称与单结果 | 四个分类可见；任务分类列出真实任务；空名称显示「未命名任务」；单个 README 结果时面板高度随之缩小 | 3782102 |
| 搜索文件 → Enter、Esc | README 在右栏只读打开；中心草稿保留；Esc 恢复输入。较早轮次中文粘贴经截图确认实际保留，工具超时不被当作输入未发生 | 50e94e2 / 3782102 |
| 快速切换任务后立即 Cmd+K | 打开自有任务后立即搜索 `nested`，焦点正确，返回真实 `nested/preview.rs` 文件；没有把查询打入草稿 | 3782102 |
| 设置 → Cmd+K → 文件 | 从深色设置页搜索 README 后，退出设置并显示右栏文件 | 3782102 |
| 文本布局 | 960px 窗口窄右栏 Markdown 列表逐行换行；1280px 深色代码文件按行号显示 | 3782102 |
| 不支持的文件 | 含 NUL 文件提示不支持文本预览；150 KiB 文本提示超过 128 KiB；已有预览与草稿未被覆盖 | 3782102 |
| Cmd+O 新项目 / 已有项目 / Cancel | 原生选择器注册后激活新项目；重复选取已有目录直接激活，未重复显示项目；设置中取消选择仍停留设置 | 3782102 |
| 加号 → 项目文件引用 | 原有 @file 候选框打开在输入区上方，最后一个候选可点击并插入草稿，没有自动发送 | 3782102 |
| `/plan keep this draft` → Enter | 仅消费 `/plan` 前缀，保留剩余草稿；实际 threads.mode 为 plan，持久消息数为 0 | 50e94e2；3782102 同实现 |
| Cmd+J 与真实输入 | shell 从已注册项目启动；`pwd` 返回真实项目路径，变量设置、`cd nested` 可保持；不存在逐条 Bash 工具伪造终端输出 | 50e94e2 / 3782102 |
| Ctrl+C | 实际运行 `sleep 20` 后 Ctrl+C，中断并回到 prompt，下一条命令输出 `R11_AFTER_INTERRUPT` | 50e94e2；3782102 同服务实现 |
| 终端底部 → 右侧、隐藏 → 恢复 | 相同变量继续输出 `final`；`stty size` 由 8×82 变为 27×36；选中终端标签及其关闭按钮在窄右栏可见 | 3782102 |
| 显式关闭与回收 | 较早「关闭所有终端」回收自有 PID 48214；最后一次点击终端标签关闭按钮回收自有 PID 54377，逐一检查进程已不存在 | 50e94e2 / 3782102 |
| 分支预检与切换 | 原生选择器将自有仓库 main 切到 r11-switch-check，实际 Git 读回一致；不使用 mock Git，不修改用户仓库 | f0ea6e5 |
| 分支短弹层与切回 | 960×600 浅/深色弹层独立 320px 宽，两项列表仅约 50px 高；完整分支名和 Current 状态可见；点击 main 后实际 Git 读回 main、工作区 clean | 4a10b74 |

未实机验证的行为不混入上表：操作系统中文 IME 候选窗、真实供应商流式回复/取消、加载真实用户 shell 配置、性能专项。文件 Finder 入口已由 production 路径和围栏测试验证，未在此次原生点验点击。没有声称可编辑预览或完整 xterm 兼容。

## 门禁与保留失败

| 源码 | 检查 | 结果 | 原始日志 |
|---|---|---|---|
| 50e94e2 | `cargo test --workspace` | 927 passed / 0 failed / 0 ignored，30 个结果组 | `/private/tmp/vega-r11-main-workspace-tests.log` |
| 3782102 | fmt、strict all-target clippy、build | PASS；clippy 3.32s，build 6.82s | `/private/tmp/vega-r11-final-{fmt,clippy,build}.log` |
| 3782102 | `cargo test --workspace` | 首个 app 目标 68 passed / 1 failed；exit 101，后续包未运行 | `/private/tmp/vega-r11-final-workspace-tests.log` |
| f0ea6e5 | fmt、strict all-target clippy、build | PASS | `/private/tmp/vega-r11-release-{fmt,clippy,build}.log` |
| f0ea6e5 | `cargo test --workspace --no-fail-fast` | **925 passed / 3 failed / 0 ignored**，30 个结果组；exit 101 | `/private/tmp/vega-r11-release-workspace-tests.log` |
| 4a10b74 | `cargo test -p vega -p vega_ui branch_` | 8 app + 4 UI passed / 0 failed | `/private/tmp/vega-r11-popup-final-tests.log` |
| 4a10b74 | fmt、strict all-target clippy、build | PASS；最终 bundle 签名验证及原生启动通过 | `/private/tmp/vega-r11-popup-final-{fmt,clippy,build}.log` |

3782102 失败为 `branch_controller_close_during_preflight_clears_exact_pending_then_reopens`，GPUI 退出时发现 `ConversationStream` 和 `BranchSelector` 未释放。专用 Astra medium 确认不必要的强 UI 引用随 preflight OS worker 传递；f0ea6e5 改为只传 BranchId/service/cancel/result，原有前台 fence 校验不变。真实后台线程暂停期间 UI 可以释放的新不变量测试和原生产关闭/重开测试均通过；原始偶发 panic 未独立复现，不能称为完全证明原始分配竞态。见 [所有权规格与证据](vega-r11-branch-worker-ownership.md)。

f0ea6e5 最新全量失败保留：

- `tests::diff::diff_refresh_intents_keep_content_during_background_and_retry`：retry 终态 `GitFailed`，保留 generation 1、1 文件 +1 的原快照。
- `git_workspace::branch::tests::mutation_runner::trusted_mutation_cancellation_reaps_process_group_descendant`：测试未读到 descendant pid 文件。
- `git_workspace::branch::tests::switch_e2e::newer_permit_invalidates_older_and_target_move_fails_before_switch`：refresh 返回 `process_control_failed`；诊断为 InitialDrain、outputs_len=2、status_seen=true、cleanup_failed=false。

这些失败没有被忽略或扩大超时；当前不能声称完整回归全绿。两名 Astra medium 只读分诊确认：相关 Diff controller/test 与整个 `git_workspace` 子树在 `7b0075a..f0ea6e5` 没有变更。三个 exact 用例各仅复跑一次，均通过（0.79s、0.50s、1.38s），不以此清除失败，也不能排除 R11 新测试影响并发调度。

- Diff 用例在真实后台 refresh 尚未结束时注入完成，随后发起 Retry，可能形成正常单飞路径没有的重叠。但 sequence fence 会拒绝旧结果，不能直接归因为注入的 GitFailed 泄入新 Retry；真实重试失败的 subprocess/join 来源尚未保留。
- descendant pid 缺失发生在调用 cancel 和回收断言之前，不能说已经证明 child cleanup 失败。
- InitialDrain 表示 reader 未在 500ms 内完成；随后实际收集到两路输出与子进程状态，但保留了先前的 ProcessControlFailed。调度/pipe 完成时序是合理推测，不是已证明原因。
- 历史 R0/R1 有相近完整 suite 失败而 exact 通过的记录，但未找到这两个当前 conversation 测试名称的既往失败，不能声称它们就是同一已知问题。

分诊原始记录：`/private/tmp/vega-r11-diff-readonly-triage.log`、`/private/tmp/vega-r11-triage-mutation-once.log`、`/private/tmp/vega-r11-triage-switch-once.log`。下一步单独跟踪 Git 生命周期时序诊断，采集无内容的 spawn/readiness、reader EOF/send 与阶段状态；不放宽超时、安全断言，不将 UI 交付扩大成未定范围的 Git 重写。

更早首次 strict lint 报告 theme helper 位于测试模块之后，已由 `4a78433` 修复；原始 `vega-r11-main-clippy-pre-palette.log` 保留。两个子任务早期包回归中的既有 Diff retry `GitFailed` 也保留在各自交付记录，后续通过不能替代根因证明。现有依赖 `block 0.1.6` 的 future-Rust 提示仍在；当前 strict lint 通过。

## 剩余范围与下一批优先级

1. 常用任务操作：更多菜单统一入口、复制路径/会话 ID、未读、后退/前进、分组与排序。侧栏分支后缀目前来自项目注册时的缓存值，切换后不会实时更新；真实选择器状态与实际 Git 正确，缓存后缀列入此卡修正。
2. 模型与设置闭环：真实连接测试、模型发现/启停/排序、字体/换行、通知、代理设置。商业套餐与账户配额不适用当前 BYOK 产品。
3. 对话操作与附件：复制/编辑/分叉、一般文件和图片上传、消息队列与转向。
4. 独立能力：辅助对话、内置浏览器/电脑控制、MCP/技能/插件、产品内子 Agent、自动化、远程工作区。需要各自的路由、持久化和生命周期规格，不能仅添加空按钮。

性能继续按用户要求延期。终端正常关闭会回收 owned shell/process group；主动脱离进程组的后代和系统阻塞超出该保证，GPUI quit observer 的 200ms 上限仍是限制。标签/终端不声称跨重启恢复，复制功能明确为当前屏幕。

本轮为本地集成与可运行验收副本；未 push、未合并 master、未发布。

最终 bundle 来源与签名哈希：外部验收目录 `native-r11/provenance.json`；原始日志哈希及各次 gate 结果：`native-r11/verification-manifest.json`。最终二进制与源码固定为 4a10b74，交付文档提交不改变二进制。

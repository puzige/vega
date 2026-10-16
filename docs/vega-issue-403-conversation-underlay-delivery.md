# Issue #403 实现交付记录

依据 [冻结规格](vega-issue-403-conversation-underlay.md)。本记录在代码实现前建立，运行状态只记录实际证据。

## 验收矩阵与实现计划

| ID | 风险与前置状态 | 操作 | 可观察结果 | 证据层级 | 状态 |
|---|---|---|---|---|---|
| U1 | Native / Codex 非空长会话，浅深主题 | 上翻历史并检查生产视口和绘制 | 真实列表覆盖 Composer 所在区域；48px 背景透明过渡绘制在消息之上；正文与 Composer 同轴 | GPUI 布局与 painted_quads；用户视觉手测 | 已实现 / 待 CI / 待手测 |
| U2 | 贴底长会话 | 多行输入、真实附件与附件错误、待发送队列、控制器错误及隐私错误增减，窗口缩放 | 原生列表余量随实际前景高度变化；末项在前景上方完整可见，列表项数与业务项数一致 | GPUI 生产布局 / 原生 ListState | 已实现 / 待 CI |
| U3 | 回看历史 | 改变 Composer 高度并追加流式文本，再点击回到底部 | 业务消息身份和像素读位保留；回底恢复跟随 | GPUI 生产滚动 / 交互 | 已实现 / 待 CI |
| U4 | 消息在前景下方 | 在过渡区滚动，在实色前景点按；打开模型、权限和上下文菜单 | 装饰不拦输入，实色面阻止底层命中，菜单位于前景之上且可关闭回焦 | GPUI 命中 / 焦点 / 绘制；既有菜单回归 | 已实现 / 待 CI / 待手测 |
| U5 | 空会话、较短但可滚动会话、底部必要提示 | 空任务与消息恢复；检查阅读导航和必要状态 | 空态 utility bar 保持布局；导航依实际可读高度显示；必要提示可操作 | 既有回归与新增生产测试；用户手测 | 已实现 / 待 CI / 待手测 |

实现顺序：先新增生产回归，再将非空会话底部栈置于前景，通过实际布局测量提供原生列表 bottom padding，绘制纯装饰渐变并保护实色面的命中；最后核对锚点可读高度及被新需求直接替代的旧位置断言。空态沿用现有路径。不改存储、权限与提交接口，不新增依赖或产品文案。

本地禁止编译、构建、打包、Clippy、test / nextest。仅运行快速格式和差异检查。新增生产测试在云端 PR Check 运行；本地不伪造红 / 绿测试证据。

## 实现

- `conversation_stream/render.rs`：非空消息区域保持全高，底部栈独立固定于前景。48px 透明至当前 `bg_base` 的渐变没有事件处理器或命中遮蔽；导航按钮在该区域之上。实色前景使用 `occlude()`，覆盖 Composer 两侧与下方，阻止被遮消息响应点击。既有模型、权限、文件选择和待发送浮层的 deferred priority 未改动。
- 定位状态和电脑操作提示移入可测量的实色前景，位于阅读导航下方、隐私和 Composer 之前；提交面板保留在 Composer 之后。所有既有文案和动作保留。
- `conversation_stream/core.rs`：保存底部栈实际高度，供原生 `list().pb(...)` 使用。测量只在高度变化时通知下一帧，不重建 ListState、不插入业务消息、不扫描全量消息、不主动恢复 Tail；既有原生跟随与历史锚点继续生效。
- `conversation_stream/message_anchors.rs`：导航的可读视口高度扣除前景，避免内容仅在前景下溢出时漏掉导航。
- `vega_theme/src/lib.rs`：共享 `Layout::CONVERSATION_BOTTOM_FADE_HEIGHT`，48 逻辑像素。
- 仅按冻结规格替换 `issue98_scroll_button.rs` 与 `issue296_desktop_ui.rs` 四处被需求直接替代的“按钮 / 导航必须在列表之外”位置条件；同轴、尺寸、内距、绘制顺序、点击、Enter、焦点和阅读位置断言保留。空会话继续原布局。

未偏离规格；没有新增依赖、业务消息、配置、产品解释文字或代码注释。

## 生产回归（未在本地执行）

新增文件：`crates/vega_ui/src/conversation_stream/tests/issue403_conversation_underlay.rs`，已注册至既有测试模块。

| 测试 | 可证伪的生产风险 |
|---|---|
| `issue403_transcript_extends_beneath_themed_foreground` | Native / Codex × 浅 / 深主题的真实列表与 Composer 重叠，消息行确实位于遮蔽区，渐变及实色前景的真实 Quad 和绘制顺序 |
| `issue403_tail_clearance_tracks_composer_and_required_states` | 多行、图片、附件错误、队列、控制器错误、隐私状态增减及窗口缩放后，末条完整可见且原生列表项数不变 |
| `issue403_footer_resize_preserves_history_anchor_and_resume` | Composer 增高与流式追加不改变历史身份和像素读位，点击回底恢复原生跟随 |
| `issue403_fade_passes_scroll_and_foreground_blocks_hidden_tools` | 渐变区域内的真实滚轮生效；把真实可点击工具标题移到 Composer 顶部内侧 4px 空白处后点击，不误触底层；模型 / 权限浮层绘制在渐变之后，菜单 Esc 关闭，添加上下文的原有内联菜单保持可达 |
| `issue403_empty_layout_and_short_overflow_navigation_remain_usable` | 空态与 utility bar 无前景渐变；短内容介于实际可读高与完整视口高之间时仍显示锚点导航 |

短内容用两条单行用户消息；当前真实行含顶部 24px、气泡上下共 16px、正文 23px、尾距 48px，合计约 111px / 条。两条约 222px，在 320px 窗口中介于可读高（扣除前景后约 150px）与全视口高之间；测试保留这两个边界断言。

用户禁止本地编译 / 测试，因此没有本地红 / 绿运行记录。上述测试调用生产渲染与 ListState；存储恢复、选区复制、长会话水合、既有菜单及权限回归由现有云端 workspace suite 一并执行，不新增外部 E2E。

## 本地快速检查证据

检查时间：2026-10-16 01:11 UTC / 09:11 Asia/Shanghai。分支 `feat/403-conversation-underlay`，基线由主 agent 更新至当时最新 `origin/master`。下列八个变更代码文件按排序后的相对路径、NUL、文件内容、NUL 逐个拼接的 SHA-256：`47c603e9e24c0c34f1473ef81ade2a7fe7bf7697cab4d37f9fa006e1ee495656`。

| 命令 / 检查 | 退出码 | 原始输出 / 事实 |
|---|---|---|
| 首轮 `cargo fmt --all -- --check` | 1 | 仅报告本卡新增代码格式差异；例如 `foreground_height: px(` 多行格式须折叠。未执行编译或测试 |
| `cargo fmt --all` | 0 | stdout / stderr 为空；修正上述格式 |
| 最终 `cargo fmt --all -- --check` | 0 | stdout / stderr 为空 |
| `git diff --check` | 0 | stdout / stderr 为空 |
| `git diff --no-index --check /dev/null crates/vega_ui/src/conversation_stream/tests/issue403_conversation_underlay.rs` | 1 | stdout / stderr 为空；no-index 的新增文件差异退出码，不是 whitespace 诊断 |
| `python3` 对本卡新增 diff 与新测试文件的静态断言 | 0 | 下列原始输出；仅文本检查，无编译 / 测试 |

```text
Added code comments: 0
Added production unwrap/expect: 0
Issue 403 GPUI regressions: 5 (not executed locally)
```

其他目录、Cargo 配置和 target 未修改；没有创建共享 target。Executor 交付时未提交、push、创建 PR、合并、部署或安装应用；后续集成与云端证据由主 agent 记录在关联 PR 和 Issue 中。

## 云端与剩余验收

NOT RUN / 待 CI：本卡五项 GPUI 回归与现有 suite。可用过滤命令为 `cargo nextest run -p vega_ui issue403_`，实际现有 PR gate 运行 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings` 和 `cargo nextest run --workspace`。编译、运行与失败日志由主 agent 开 PR 后记录，不能把静态检查标作行为通过。

待用户手测：

1. 浅色和深色分别打开 Native、Codex 长会话，上翻历史；观察消息真实延伸至输入框后方，渐隐连续、底部和两侧无消息碎片，正文与输入框同轴。
2. 贴底增减多行输入、图片、待发送消息；改变窗口大小、展开 Workspace；末条仍可读，输入和发送入口可用。回看历史时重复操作并接收新输出，读位应保留；点击或键盘激活“回到底部”恢复跟随。
3. 在过渡区滚动，点击输入框边缘；不误触被遮住的工具或消息。操作模型、权限、添加上下文、文件选择和待发送菜单，验证 Esc、外部点击、Tab 和回焦。
4. 检查定位失败、电脑操作权限 / 暂停提示、隐私检查错误与提交面板仍可见可操作。空任务保留欢迎内容和 utility bar。

LIMIT：本地仅格式及静态证据，未启动或截图真实 Vega。前景高度在实际布局测量后只对变化安排后续帧；首次与多行高度变化的实际绘制、边缘像素和设备输入须由 CI / 用户真实应用确认。没有存储迁移。回滚为回退本卡合并提交；Issue、Project 和分支 / worktree 的交付状态由主 agent 管理，用户手测前不关闭或清理。

# #552 续接 writer 异常检查点与恢复结果

当前结果见文末“恢复结果”及[当前交接](vega-issue-552-test-first-handoff.md)；下列原始检查点保留为恢复来源，不是当前待办。

- 原检查点阶段：完整先行测试补齐中；尚无有效 RED，未进入生产实现。
- 原记录时间：2026-10-22T18:34Z（后续只读核对至 18:36Z）。
- 分支/源码 HEAD：`feat/552-workspace-motion` / `6615748f91273bfb8ee424011b3b5a09b3777d2b`。
- 原 writer 续接执行已由工具确认 failed；没有具体错误详情，不据此推断产品缺陷。该执行已终止、停止写入，父 Agent 保留全部已保存内容，不恢复旧 ref、不 reset。

## 内容与来源

原 16 测试部分包经父审后按[规格 §5.6](vega-issue-552-workspace-motion.md#56-主-agent-缺口诊断与测试范围续接尚未生产实现)补齐，验收 W1–W12 不变。续接已新增默认关闭的跨 crate test-support、无 PTY 的真实 TerminalSession + driver、TerminalView 复用构造/轮询，以及 File/Diff/stream 的只读滚动/绘制观察。原 TerminalView 普通入口机械抽取 `with_session`，普通调用顺序仍待最终等价审查。

父 Agent 在 writer 终止后只读核对 17 个已变/未跟踪文件；将 `SHA256  两空格  相对路径\n` 按路径排序拼接的清单 SHA-256 为 `14d48972284a84f1fe6936cbbe14edfb47f74f69e3e1fd61d769070d2f88212f`（不含本检查点）。关键内容：

| 文件 | SHA-256 |
|---|---|
| `crates/vega/src/window/workspace/motion_tests.rs` | `60482441a3378dacc67a72c2b1671dc89aec329ab871c5772e8fdfbf69c04467` |
| `crates/vega_ui/src/test_support.rs` | `d2b4a87814b3bbd3e2ffc9e3922e7e48fb2534f9f4a6dc8db1a6bfe2f83c1891` |
| `crates/vega_conversation/src/terminal/test_support.rs` | `7c5eca5f68e6beef6d2852b85f724c2b9cb01f44363b4fe692c167246d95dabd` |
| `docs/552/vega-issue-552-workspace-motion.md` | `bc6524d6371c6f48c0db488a69a57003a5f0c9880ccf3e6056a1577183fea8a8` |

旧 `test-first-handoff` 的 hash/范围只对应第一次部分交付，不对应本检查点，后续 writer 必须区分历史与当前版本。

## 原始证据与责任层

- 工具仅报告 subagent failed，无编译/测试日志；这是执行环境/Agent 故障，不是有效产品 RED。
- 新 test-support 代码尚未完成静态核对。父审已发现 `paint_terminal` 的 `cx` 是 `&mut App`，当前调用 `cx.entity_id()` 不存在；锁定 API 是 `Window::current_view()` 或 `Context<T>::entity_id()`。先修接线，不更改验收。
- `window.text_style().color` 不含继承的 element opacity；`element_opacity()` 是 crate-private。当前 PaintLog 不能据此声称全不透明。必须先证明真实绘制观察可检测父 opacity 变化；不可读取私有 API、伪造 alpha 或把只读 style 当实际绘制。
- `SelectionSamples` 目前只是测试期记录容器，尚无真实生产采样；不得由观察器合成目标 status。完整组合测试要先写目标断言，未来由真实用于填色的 sampler 接线。
- W7/W11 组合、无 inactive draw 恢复和真实内容/滚动断言尚未完整写入；保留原 16 测试与 W1–W12，不删除困难用例。

## 原检查点的下一步（历史）

由单一替补 writer 继续同一内容、同一测试前置阶段：修最窄观察接线，补齐真实 Workspace + TerminalView + ConversationStream 输入/输出/阅读回归和阳性对照，更新真实交接。禁止生产动画、第三方升级、新执行设施、真实外部进程/网络、本地 Vega/编译/测试/CUA。完成后停写给非写入者独审，再由父 Agent 建 draft PR 使用现有 Actions；harness 失败与产品 RED 分开归因。

## 恢复结果（当前，取代上方待办状态）

- 当前内容版本：`v1.2-test-first-recovery-ready`；HEAD/branch 不变；开工已复核上方 17 文件清单与四个关键 hash 全部匹配。父 Agent 已同步基线，替补 writer 未执行 Git/index 写操作，未操作 Issue、PR、CI 或嵌套委派。
- 修复 App/Context 观察接线：Terminal paint 使用公开 `Window::current_view()`；Retry observer 使用锁定 Kit test 模块导出。PaintLog 不再以 text_style alpha 冒充继承 opacity，而是读取同 paint 边界实际提交的微小 quad，保留父 opacity=0.25 的阳性对照；其测试构建增加 probe quad 的证据边界已明确记录。
- 原 16 个测试保留，新增 6 个 GPUI 用例：真实绘制 opacity 对照、无 PTY 终端 parser/poll/input/paste/scroll/close 对照、Terminal/Diff/文件与 Subagents 输入/滚动/生命周期、文字/关闭按钮/focus quads 几何、无 inactive draw 快恢复、生产 Workspace + TerminalView + ConversationStream 业务组合与装饰停止分层。W6 原测试增强实际错误/Retry/恢复文字，不删除原断言；W12 Some(true/false) 保留。
- 对话正文通过默认关闭的 SelectionDocumentBuilder 转发模块委托原 selection.rs，只观察真实叶子 paint；没有复制 renderer 或生产选择算法。Terminal 私有 constructor 的 spawn/poll/字段初始化与基线机械等价，仍需非写入者独审。
- 当前 SelectionSamples 仍没有生产填色 sampler：测试按实际 pane/key 要求 Running 阳性与停止状态；None 明确失败，不由 observer 合成、不驱动帧。无有效 RED，未生产实现共享 helper、填色、aria_selected 或业务修复。
- 本地仅完成格式/差异/offline metadata 和静态审计，最终退出码及精确内容清单见 [当前交接](vega-issue-552-test-first-handoff.md)；Actions 发现/执行/通过/失败/忽略/跳过均未取得，未运行项不能称为通过。当前 W1–W12 映射见 [spec §5.7](vega-issue-552-workspace-motion.md#57-恢复完成的先行测试映射与证据边界)。
- 恢复 writer 在最终交接时 READY 并停止写入；下一步由父 Agent 对精确清单组织非写入者独审，再使用既有 Actions 验证阳性对照和有效 RED，harness 失败先修责任层。

# Composer 输入预测实现交付

2026-10-18；分支 `feat/469-composer-predictions`；依据 [规格 v1.3](vega-issue-469-composer-predictions.md)。代码、测试和本报告同次提交，提交标识以 Git history 为准。状态：实现交回，待 Codex review、PR CI 和用户手测。

## 修改范围

- `vega_store/src/config.rs`：默认开启字段及旧配置兼容、往返断言。
- `vega_conversation/src/composer_predictions.rs` 与 `lib.rs`、`types/mod.rs`：有限可见上下文、严格 JSON、独立取消/超时、前后持久化身份复核及辅助 usage。
- `vega/src/app_agent.rs`、`window/composer_predictions.rs` 及 agent/artifact/pending_messages/session/window 集成：复用受保护的本轮 provider；成功轮次和队列释放后异步生成；凭据、配置、路由和生命周期 fences。
- `vega_ui/src/text_input`、`conversation_stream`、`lib.rs`：ghost、真实按键接受、IME/revision 保护和集中失效；内部 Global 经 bool setter 广播设置。
- `vega_ui/src/settings`：立即生效的可访问开关、逐字段保存和冲突处理。
- 对应 conversation、config、settings、TextInput、stream 和 app 测试；既有 app fixture 按辅助用途路由，不消耗主轮脚本或放宽主轮断言。

未修改依赖、CI、数据库 schema、真实配置或已安装应用；未改动规格、handoff 或 README。

## 本地静态证据

以下命令仅做快速静态检查，没有编译或运行 Vega：

| 命令 | 结果 | 有界输出 |
|---|---|---|
| `git diff --check` | exit 0 | 无输出 |
| `cargo fmt --all -- --check` | 最终 exit 0 | 无输出 |
| `cargo fmt --all` | exit 0 | 无输出 |

初次格式检查 exit 1，原因是新增代码尚未格式化；执行 formatter 后检查为 0。以上结果不证明类型、Clippy 或运行行为通过。

## 自动回归与待 CI 项

全部自动回归均为 **NOT RUN，待 GitHub Actions**：

| 规格 | 已编写断言 |
|---|---|
| P1、P10 | 当前模型一次独立请求；真实 app 成功轮和队列释放；ghost 不进入消息；usage 含缓存和价格出处，未知价格不伪造；弃权/收到 usage 后错误仍计量 |
| P2、P3 | GPUI 真实 Tab/Right/Enter 一次接受、光标末尾且不发送；原发送/换行/遍历及修饰键；保留 TextInput 导航覆盖 |
| P4、P5 | 延迟响应不覆盖草稿；空白和删除回空不复活；中文与 emoji UTF-16 marked composition、候选 Enter/Tab 不接受或发送 |
| P6、P7 | 附件、队列/steer、权限/提问、模型/模式/后端、上下文操作、预览/子会话；取消、Esc、切路由、删除、关闭；后台其他线程完成/删除不撤销当前建议；存储在请求期间新增消息或改变身份后丢弃 |
| P8 | 旧配置默认开；设置逐字段并发保存/冲突/失败；键盘开关及保存中禁用；所有缓存实体立即撤销；重开无历史补生成 |
| P9 | 缺字段/重复键/额外字段/错误类型/null/边界/control JSON；工具、重试、异常和未完整结束；建流及读流超时；独立取消且零重试 |
| P11 | 失败/停止、plan、自动任务、子会话、ACP、历史打开不生成；凭据缺失、歧义、撤销、轮换及权限失效不调用底层 provider |
| P12 | 浅深主题、窄宽度、长中英文 ghost；placeholder 替换、原行高、真实文本/选区/IME 查询隔离；footer 提示 |

还包含进程内 Chat Completions/Responses SSE 替身和有限上下文/隐私断言，不依赖真实服务或外部应用。

待执行命令：`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo nextest run --workspace`。云端原始输出尚不存在，交主 agent 从对应 PR 最新 head 记录。

## 限制与交回

- 规格偏离：无。含展开 `[@` 前缀的 user 记录整条跳过，遵守 v1.3 的可见正文契约。
- 本地 Vega 运行、编译、构建、打包、Clippy 和测试：NOT RUN，遵守仓库限制。
- 真实模型、系统输入法、浅深主题和窄窗口体验：待用户手测；自动替身不代表真实集成通过。
- 当前没有已知阻碍；代码及测试的类型和行为结果均等待 PR CI。由主 agent 继续 review、CI、合并和手测交接。

## 首轮 CI 修复记录

[PR #472 首轮 Actions](https://github.com/puzige/vega/actions/runs/39118482298) 在测试编译阶段失败；Clippy 和 Nextest 均 exit 101，未执行 Rust 行为测试。原始诊断为 `recursion limit reached while expanding #[test]`，定位于 `composer_prediction_authority_tests` 的普通测试属性。

该模块的 `use super::*` 意外导入父模块中的 GPUI `test` 宏。修复仅将导入缩窄为 `use super::composer_prediction_authority`，保留普通测试和全部断言；未增加递归限制。修复后的编译、Clippy 和行为验证仍待 PR CI，本地未运行。

## 第二轮 CI 修复记录

[PR #472 第二轮 Actions](https://github.com/puzige/vega/actions/runs/39118968627) 在测试编译阶段以 E0063 失败；Clippy 和 Nextest 均 exit 101，未执行 Rust 行为测试。`tests/model_selection.rs` 的既有 `ActiveAgentRun` 夹具缺少两个新增字段。

修复为补入与控制器默认构造一致的 `composer_prediction: None`、`composer_prediction_config_generation: 0`，保留全部断言和其余逻辑。静态核对其余 `ActiveAgentRun`、`AgentBatch` 构造及 `AgentUpdate` 分发均已覆盖预测字段/变体。

主会话已将任务分支同步到最新 master（原生滚动边界淡化），修复基于同步后的 HEAD；新 head 的编译、Clippy 和行为结果待 PR CI 重验，本地未运行。

## 第三轮 CI 修复记录

[PR #472 第三轮 Actions](https://github.com/puzige/vega/actions/runs/39119487281) 的 fmt/Clippy 成功；Nextest 运行 1,648 项，1,642 项通过、6 项失败，另有 3 项既有 skipped。修复依据 [云端回归交接](vega-issue-469-composer-predictions-ci-repair.md) 和规格 v1.4：

- 两组 reasoning replay SSE 夹具依据系统提示分流预测，Chat 使用 `messages[0].content`，Responses 使用 `input[0].content[0].text`。预测返回合法 null SSE，独立记录并按成功轮次验证一次调用、当前模型、512 token、无工具；不消费主轮脚本 index。主轮精确 4 次请求、授权及 reasoning/tools/usage/终态断言保留。
- 权限/提问 listener 在安装卡片前 reconcile，队列 latch 到达即可撤销预测；回归明确验证卡片尚未出现，以及阻塞解除后旧 fence 仍无效。
- `insert_newline` 在 marked composition 期间传播动作并保留草稿；回归增加 Shift+Enter，保留中文、emoji、UTF-16 marked/selection 范围及不提交断言；无合成状态的原换行断言保留。
- 持续 blocker 使用具名 case 并拒绝新申请；线程/模型身份变化单列，撤销旧 owner 后允许下个完成轮申请，旧 fence 不能覆盖新 owner 的建议。

本次仅修改交接列出的四个代码/测试文件及本报告；没有本地运行 Vega、编译、Clippy 或测试。修复后 `git diff --check`、`cargo fmt --all -- --check` 的结果由交回消息记录，新 head 行为仍待 PR CI，不能沿用第三轮旧 head 的成功。

用户最新要求为**本轮只把 PR 准备好，不自动合并 master**；此前段落的合并安排属于历史交付记录。PR 保持 OPEN，Issue 保持 In progress，保留分支和 worktree；后续由主会话完成 review 和云端验证。

## 第四轮 CI 修复记录

[PR #472 第四轮 Actions](https://github.com/puzige/vega/actions/runs/39120769717) 的 fmt/Clippy 成功；Nextest 运行 1,649 项，1,648 项通过、1 项失败，另有 3 项既有 skipped。唯一失败仍是 IME 回归：Enter 后原 `中😀文` 被换行替换；第三轮其它修复的回归已通过。

修复依据 [IME 命令交接](vega-issue-469-composer-predictions-ci-ime-repair.md) 和规格 v1.5。锁定 GPUI 的高层按键派发在 action 继续传播时回退为输入换行/Tab；macOS marked 输入法已经先于应用 action 处理事件，传播不能把命令送回输入法。上一轮传播动作的处理不足以保护草稿。

- Composer 聚焦且 composing 时消费发送、换行和焦点遍历动作，不提交、不插入控制字符、不移动焦点；非 Composer 焦点的发送动作继续传播，非合成态原行为保留。
- 保留高层 Enter/Shift+Enter/Tab 模拟及原中文、emoji、UTF-16 和不提交断言，增加 Cmd+Enter、Shift+Tab、焦点保持及真实 `ComposerSubmitted` 零事件断言。
- 仍在 marked 状态时使用原 `replace_text_in_range(None, committed_text, ...)` 提交文本，验证整段 marked 被替换、composition 清除、UTF-16 光标位于末尾且不自动发送；随后保留幂等 unmark 和旧 fence 拒绝断言。平台输入提交回调未修改。

本地只做差异和格式静态检查，未运行 Vega、编译、Clippy 或测试；新 head 的行为结果仍待完整 PR CI。此次仍只准备 PR，不提交或推送，不合并 master；由主会话 review 和集成后继续云端验证。

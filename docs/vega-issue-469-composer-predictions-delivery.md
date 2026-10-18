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

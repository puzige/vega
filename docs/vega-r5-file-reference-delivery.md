# Vega R5 文件引用应用链交付

- **日期**：2026-09-05
- **分支**：`codex/vega-r5-file-reference`
- **范围**：真实 `@file` 输入、索引、候选选择、提交解析和 provider 请求注入
- **SDD**：[vega-r5-file-reference-sdd.md](vega-r5-file-reference-sdd.md) v0.1

## 已落地行为

`vega_tools` 现在以串行 `ignore::Walk` 生成确定性候选快照。索引保留 512 条候选上限，并对每个 yielded item、4096B 路径、2MiB retained bytes 和 2 秒 cooperative wall budget 做检查；8192 是可观察 yielded item 上限，ignore 库内部过滤和已经开始的阻塞 filesystem syscall 不伪称为可计数或可强制中断。取消返回 typed failure，worker 不发布部分快照。

共享的 `FileIndexFailureCode` 与 `FileReferenceFailureCode` 位于 `vega_conversation::types`。UI 只消费快照和失败 code，维护 Loading/Ready/Empty/Failed、Retry、Esc、输入退出 `@`、generation 和晚到结果 fence；窗口层通过真实 `VegaWindow` subscription 启动单一命名 worker，并在 stream/thread/project/generation/Settings 路由任一变化时取消或丢弃旧结果。channel disconnected 在 owner 仍有效时也会结束 Loading 并显示短失败。

提交时会先重新 canonicalize 并执行项目根目录 fence、symlink/directory、binary、8 文件、16KiB 单文件和 48KiB 总量检查，再构造 provider。解析失败以原子 terminal update 携带 typed 原因，释放 pending/run owner、保留可编辑原输入、不新增 user echo 或 durable history，provider 请求数为零；既有历史不删除。成功时实际请求包含 fenced reference block 和原始用户文本。

索引失败态使用独立的 `FileSelectRetry` 键盘作用域：Tab 可达 Retry，Enter 可从失败态重试，Space 只在 Retry 控件获得焦点时触发；Esc 清除失败态、关闭选择器并恢复 Composer 焦点。隐藏失败面板不再保留 `FileSelect` 按键捕获。

`TextInput::complete_at_query` 的替换范围从 `@` 后的 token body 开始。此前真实 app 选择 `notes.txt` 会把已有 `@` 再拼一次，产生 `@@notes.txt`，导致 resolver 将其视为错误路径；现在只写入 `notes.txt `，保留已有 `@` 并维持尾随空格结束 token。这是输入范围语义修正，不是测试放宽。

## 验证证据

所有命令使用 `CARGO_NET_OFFLINE=true`、共享 target `/Users/puzige/Workspace/vega/target`，原始输出保存在 `/private/tmp/vega-r5-file-reference-20260905/`：

| 日志 | 命令结果 |
| --- | --- |
| `29-test-vega-tools-final.log` | `vega_tools` 全部 **98 passed / 0 failed**；含 absolute external temp path、512 deterministic prefix、cancel、deadline、8192 yielded-item 行为测试 |
| `33-test-vega-ui-after-extract.log` | `vega_ui --lib` **119 passed / 0 failed**；含 generation overflow、late result、editable rejection、TextInput completion |
| `43-test-vega-ui-file-retry.log` | 新增失败态键盘回归 **2 passed / 0 failed**；覆盖 Tab→Retry、Esc→Composer、Enter 换行和 Enter retry event |
| `44-test-vega-tools-reference.log` | reference budget 定向回归 **8 passed / 0 failed**；含取消、deadline、8192 yielded item、512 deterministic prefix |
| `45-check-vega.log` | `cargo check -p vega --locked` **passed** |
| `46-clippy-vega-ui.log` | `cargo clippy -p vega_ui --lib --locked -- -D warnings` **passed** |
| `47-clippy-vega-tools-app.log` | `cargo clippy -p vega -p vega_tools --lib --locked -- -D warnings` **passed** |
| `48-fmt-check.log` | `cargo fmt --all -- --check` **passed** |
| `30-test-real-reference-final.log` | 真实 GPUI `VegaWindow` subscription 与提交链 **3 passed / 0 failed**；成功请求、重开持久化、headless zero-provider rejection 和同一真实窗口的 8 类负面矩阵均在对应测试中覆盖 |
| `24-test-terminal-reference.log` | typed resolver failure 原子 terminal **1 passed / 0 failed** |
| `38-test-file-index-cancel.log` | app-level owned-worker cancellation **1 passed / 0 failed**；token 立即取消、active owner 清除，poller 无需 join |
| `40-clippy-focused-final.log` | `vega_tools`, `vega_conversation`, `vega_ui`, `vega` all-target clippy **passed**，`-D warnings` |
| `32-fmt-after-extract.log` | `cargo fmt --all -- --check` **passed** |
| `41-build-final.log` | `cargo build --workspace --locked` **passed** |

真实订阅 E2E 使用 owned temporary project/store 和 `MockProvider`，成功路径检查实际 request 的 reference block、marker 和原始文本；随后对 missing、outside-project、binary、oversized、directory、symlink、total-bytes 和 too-many references 逐项提交，provider request count 保持为成功路径后的 1，输入和 pending 状态恢复，历史条目数量不增加。

## 未测与交接边界

完整 workspace 测试、原生 CUA 走查和 clean-master 对照由 root 在联合树执行。未读取真实用户文件、未使用真实 provider key、未发送真实 provider 请求、未运行性能 bench/soak。2 秒 deadline 对已进入的阻塞 filesystem syscall 仍是 cooperative 语义；worker 的 cancellation 和 owner/generation fence 已覆盖其 late result。真实重试经过 UI event/owner 代码路径，尚未增加会等待 worker 两次的原生 retry E2E。

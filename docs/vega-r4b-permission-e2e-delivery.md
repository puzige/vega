# R4b 权限真实应用入口验收补充交付

v0.1 · 2026-09-05 · test-only bounded acceptance

## 交付结果

提交 `cb74392`（`test: cover permission approval through VegaWindow entry`）在
`/Users/puzige/Workspace/worktrees/vega-permission-e2e` 的
`codex/vega-permission-e2e` 分支补充了一条真实应用入口回归。测试从已渲染的
`ConversationStream` 发送 `cmd-enter`，经 `ComposerSubmitted`、
`VegaWindow::submit_composer` 和 `start_agent_run` 启动生产 worker；没有直接调用
`run_agent_worker` 或手造 `PermissionRequest`。

夹具全部来自 owned `tempfile` repo、SQLite 和 config，provider 使用现有
`cfg(test)` override。第一轮 MockProvider 提出一个 `write` proposal，Confirm
模式下实际 PermissionQueue/app poll 建立 PermissionCard；测试用真实键盘 `Enter`
选择 Once，随后第二轮请求收到该 call 的 tool-result，文件内容、成功 audit、审批
来源和当前 stream 的卡片清理均得到断言。`write` 是覆盖语义，测试没有把最终字节
内容宣称为底层 syscall exactly-once 计数；既有 bash recorder 回归保留其单次副作用
证据。

`has_active_permission_card` 是唯一新增 seam：它只报告 stream 是否持有 active card，
不暴露 request payload、call id 或 responder，也不改变权限状态机。

## 验收命令与日志

命令均在该工作树执行，环境为：

    env PATH=/Users/puzige/.cargo/bin:$PATH CARGO_TARGET_DIR=/Users/puzige/Workspace/vega/target CARGO_NET_OFFLINE=true

先清理本树涉及的包：

    cargo clean -p vega -p vega_ui -p vega_conversation

保留的首次失败是 `02-entry-test.log`：真实 `start_agent_run` worker 的
PermissionQueue watch wakeup 触发 GPUI deterministic scheduler 的跨线程 activity
守卫。测试只增加 `cx.executor().allow_parking()` 以覆盖生产专用 worker 线程的真实
wakeup；app 侧仍由 GPUI pump 驱动。修复后入口测试通过：

    cargo test -p vega --bin vega tests::agent::production_agent_start_entry_surfaces_write_permission_and_continues -- --nocapture

既有 request-first 和 agent 回归通过：

    cargo test -p vega --bin vega tests::agent::production_agent_request_first_keeps_permission_until_proposal_ingress -- --nocapture
    cargo test -p vega --bin vega tests::agent:: -- --nocapture

既有 UI 权限矩阵通过 11/11：

    cargo test -p vega_ui --lib conversation_stream::tests::permissions_cards -- --nocapture

fmt、clippy、workspace check 和 workspace build 均通过：

    cargo fmt --all -- --check
    cargo clippy --all-targets -- -D warnings
    cargo check --workspace --locked
    cargo build --workspace --locked

原始输出保存在 `/private/tmp/vega-permission-e2e-20260905/`：

| 日志 | 结果 |
|---|---|
| `01-clean.log` | 清理 2114 个文件，约 938.0 MiB |
| `02-entry-test.log` | 首次失败；GPUI 跨线程 scheduler guard |
| `03-entry-test-after-allow-parking.log` | 入口回归 1/1 |
| `04-existing-request-first.log` | request-first 1/1 |
| `05-ui-permission-cards.log` | UI 权限矩阵 11/11 |
| `06-vega-agent-suite.log` | agent suite 9/9 |
| `07-fmt-check.log` | 首次 fmt check 发现新增代码格式差异 |
| `08-fmt.log` | cargo fmt 修正格式 |
| `09-fmt-check-final.log` | fmt check 通过 |
| `10-clippy-all-targets-first.log` | clippy `-D warnings` 通过 |
| `11-check-workspace.log` | workspace check 通过 |
| `12-build-workspace.log` | workspace build 通过 |

## 边界与交接

未运行 workspace 全量测试、性能测试、soak、真实 provider、用户配置/Keychain 读取
或 CUA。已有 R4b 的拒绝、超时、重复、terminal-first、取消和身份不匹配反例继续
由原测试矩阵负责。共享 Cargo target 已释放；root 可 cherry-pick `cb74392`，并在
联合树统一安排 workspace 全量和原生验收。

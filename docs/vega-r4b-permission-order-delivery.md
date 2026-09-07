# R4b 权限确认事件顺序修复交付

v0.1 · 2026-09-05 · Feature A2-08 · Executor Luna/max

## 交付结果

本卡修复了 runtime 权限请求早于 app 的 ToolCallProposed 事件进入 GPUI 时被直接丢弃的问题。ConversationStream 现在最多暂存一条尚未关联工具卡的脱敏 PendingPermission；只有当前 stream/run 中 call、tool、target 全部精确匹配的 proposal 到达后，才取得现有 PermissionCard lease。重复 proposal 不会重复建卡或执行。

ToolCallFinished、Settings、Stop/cancel、切任务、关闭窗口和 terminal/error 路径均按对应 owner 清理。完成事件只终结同一 call 的 deferred 请求或 card lease；不会通过 queue 的当前 active 槽位误清理较新的请求。terminal-first、取消后迟到 proposal、无 proposal、身份不匹配和重复通知均保持 fail closed。

生产改动集中在 ConversationStream 权限投影、PermissionQueue 的只读 resolved 查询和 PermissionCard 的 exact-lease timeout；未改 DDL、依赖、@file、thinking、Git 实现或 UI 样式。源码提交为：

    f2913b54724050384930c7427f444abc8364b4de
    fix(A2-08): preserve permission requests across event ordering

工作树为 /Users/puzige/Workspace/worktrees/vega-r4b-permission-order，分支为 codex/vega-r4b-permission-order，基线为 d01ae6c8dec721aaa2b442c0a82abafa8214f66b。未 push、未 merge master、未替换安装应用。

## 运行级验收

production_agent_request_first_keeps_permission_until_proposal_ingress 使用 owned tempfile repo、独立 Store、MockProvider、真实 run_agent_worker、真实 apply_agent_batch_ingress、PermissionQueue 和文件 recorder。测试先让 queue listener 看到 permission request，再将 proposal 通过 app ingress 应用；批准 Once 后 recorder 只写入一次。该测试覆盖 runtime worker 与 app ingress 的真实连接，不宣称覆盖完整 start_agent_run 全入口。

UI 权限顺序矩阵包含 request-first、proposal-first、duplicate、terminal-first、旧 card 终态与新 request 交错、Settings/cancel/window/thread 清理以及 mismatch/late guards。安全反例由 GPUI unit seam 覆盖；运行级测试与 UI 矩阵在结果中分列。

## 命令与原始日志

以下命令均在本工作树执行，并使用：

    env PATH=/Users/puzige/.cargo/bin:$PATH CARGO_TARGET_DIR=/Users/puzige/Workspace/vega/target CARGO_NET_OFFLINE=true

先清理了共享 target 中本卡相关包：

    cargo clean -p vega -p vega_ui -p vega_conversation

该命令移除 36,386 个文件、约 12.7 GiB。随后保留首次失败，再执行修复后的聚焦回归：

    cargo test -p vega_ui permission_request_first_waits_for_matching_proposal -- --nocapture
    cargo test -p vega --bin vega tests::agent::production_agent_request_first_keeps_permission_until_proposal_ingress -- --nocapture
    cargo test -p vega_ui --lib conversation_stream::tests::permissions_cards -- --nocapture
    cargo test -p vega --bin vega tests::agent:: -- --nocapture
    cargo test -p vega_ui --lib -- --nocapture
    cargo fmt --all -- --check
    cargo clippy --all-targets -- -D warnings
    cargo build --workspace

原始日志目录为 /private/tmp/vega-r4b-permission-order-20260905/rawlogs/：

| 日志 | 结果 |
|---|---|
| 01-before-fix-ui-request-first.log | 修复前首次失败；无 proposal 时 active permission 不存在，exit 101 |
| 02-before-fix-production-request-first.log | 修复前真实 worker/app ingress 首次失败；queue listener 先消费 request，exit 101 |
| 03-after-fix-ui-request-first.log | 第一版暂存修复仍失败，保留作修复过程证据 |
| 06-after-fix-ui-request-first.log | request-first UI 回归 1/1 |
| 09-production-request-first.log | runtime worker + app ingress 回归 1/1 |
| 10-ui-permission-matrix-final.log | UI 权限矩阵 11/11 |
| 11-vega-agent-final.log | vega agent 既有 suite 7/7 |
| 12-cargo-fmt-check.log | 初次 fmt check 发现新增代码格式差异，随后运行 cargo fmt --all |
| 13-cargo-fmt-check-final.log | fmt check 通过 |
| 14-cargo-clippy-all-targets.log | clippy all-targets -D warnings 通过；仅有既有 block v0.1.6 future-incompat warning |
| 15-cargo-build-workspace.log | workspace build 通过；同一既有 future-incompat warning |
| 16-vega-ui-lib-final.log | 完整 vega_ui lib 120/120 |

## 边界与联合复核

本卡没有运行 workspace 全量测试、bench、soak 或真实 provider。已知的 AppleGit/Artifact 基线失败未删断言、未改测试口径、未被本卡重复声称为无回归；root 将在 R0/R5 联合树统一复核完整 workspace 结果。完整 start_agent_run 入口覆盖也留给联合复核，当前报告只声明上述真实 worker/app ingress 路径。

冻结规格为 docs/vega-r4b-permission-order-sdd.md v0.1；本交付报告与该 SDD 一并提交。共享 Cargo target 已释放给 root，后续不再由本卡运行 Cargo。

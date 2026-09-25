# Issue #79 交付记录：将提交错误显示在 Composer 上方

## Freeze

- 验证时间：2026-09-25 12:58 UTC / 20:58 CST
- 分支：`feat/79-composer-error-placement`
- 任务规格：`docs/vega-issue-79-composer-error-placement.md`
- 主机：Darwin 24.6.0 arm64
- 工具链：rustc 1.98.0、cargo 1.98.0、git 2.55.0
- 测试时改动：仅移动 `controller_error` 的 GPUI 节点，并新增本卡 GPUI 回归测试
- 首轮测试时代码与测试补丁 SHA-256：`c2fc96c49776aee615d6d811464609174620957984c30a79c88cdbc634dab425`

## Results

| 验收项 | 证据级别 | 精确命令 | 结果 |
|---|---|---|---|
| 错误节点相对 Composer 的上下位置、正文列宽；New Task utility bar、凭据失败草稿、错误清除无空位、会话引用拒绝、连续成功提交及 MCP warning/run 状态 | GPUI production render | `cargo nextest run -p vega_ui -E 'test(/issue79_errors_stay_above_composer_across_draft_and_session_routes/)'` | PASS，1/1 |
| #79 与凭据、引用拒绝、MCP warning、Composer preflight/run 按钮、成功回显关联回归 | GPUI production render | `cargo nextest run -p vega_ui -E 'test(/issue79_|credential_failure_keeps_draft_and_renders_recovery_error|reference_rejection_releases_submit_and_keeps_editable_draft|issue73_enabled_mcp_failure_is_visible_without_remote_content|issue174_pending_preflight_keeps_composer_wrapper_height_and_stop_projection|composer_echo_waits_for_durable_acceptance/)'` | PASS，6/6 |

第一次定向 Nextest 原始输出：

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1m 54s
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
────────────
 Nextest run ID b2cf90e5-b4b7-4ee6-a4ce-72104c836850 with nextest profile: default
    Starting 1 test across 1 binary (476 tests skipped)
        PASS [   0.050s] (1/1) vega_ui conversation_stream::tests::issue79_composer_error::issue79_errors_stay_above_composer_across_draft_and_session_routes
────────────
     Summary [   0.051s] 1 test run: 1 passed, 476 skipped
```

关联回归定向 Nextest 原始输出：

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.48s
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
────────────
 Nextest run ID 6b6c73ac-dc5b-4543-b346-1724243f4e84 with nextest profile: default
    Starting 6 tests across 1 binary (471 tests skipped)
        PASS [   0.021s] (1/6) vega_ui conversation_stream::tests::core_flow::credential_failure_keeps_draft_and_renders_recovery_error
        PASS [   0.022s] (2/6) vega_ui conversation_stream::tests::core_flow::reference_rejection_releases_submit_and_keeps_editable_draft
        PASS [   0.023s] (3/6) vega_ui conversation_stream::tests::preparing_indicator::issue174_pending_preflight_keeps_composer_wrapper_height_and_stop_projection
        PASS [   0.024s] (4/6) vega_ui conversation_stream::tests::core_flow::issue73_enabled_mcp_failure_is_visible_without_remote_content
        PASS [   0.025s] (5/6) vega_ui conversation_stream::tests::core_flow::composer_echo_waits_for_durable_acceptance
        PASS [   0.032s] (6/6) vega_ui conversation_stream::tests::issue79_composer_error::issue79_errors_stay_above_composer_across_draft_and_session_routes
────────────
     Summary [   0.032s] 6 tests run: 6 passed, 471 skipped
```

## Residuals

- `ACCEPTED`: 本地 Nextest 报告依赖 `block v0.1.6` 有未来 Rust 兼容性警告；本卡代码及 6 项定向测试通过。
- `NOT RUN`: 未在系统安装的 Vega.app 上检查这个未集成分支。真实桌面包验收需待此提交进入可运行构建后进行；本卡 GPUI production render 测试验证了节点顺序、对齐、宽度与清除后的布局。
- Spec 偏离：无。

## PR #199 云端失败复验与修复

- 验证时间：2026-09-25 13:41 UTC / 21:41 CST。
- 复验基线：已 fetch 并 rebase 到包含 #156 与 #157 的最新 `origin/master`；冻结规格需求没有变化。
- CI run：`36138397685`。Clippy job `108081938604` 报 `clippy::bool_assert_comparison`；Nextest job `108081939178` 仅有 R49 几何测试失败。
- CI Clippy 原始错误摘录：

```text
error: used `assert_eq!` with a literal bool
--> crates/vega_ui/src/conversation_stream/tests/issue79_composer_error.rs:123:5
123 |     assert_eq!(reference_state.0, false);
= help: replace it with `assert!(..)`
```

- CI Nextest 原始错误摘录：

```text
thread 'window::workspace::tests::r49_utility_bar_mounts_above_the_card_only_on_the_new_task_page' panicked at crates/vega/src/window/workspace.rs:3513:9:
utility bar meets the card top: bar bottom 717.5px, card top 741px
Summary [ 68.552s] 1823 tests run: 1822 passed, 1 failed, 5 skipped
```

- 根因：这个 23.5px 间距来自已渲染的 `controller_error` 行，不是 utility bar 或 Composer 布局回归。R49 原测试夹具使用内存数据库，数据库路径为 `None`，Skills projection service 因缺少磁盘数据库 / 配置上下文而将 `storage_failed` 投影成“Skills 来源已变化，请刷新后重试”。#79 正确把该真实错误放在 utility bar 与 Composer 之间，因此它占据了 4px 上边距和 19.5px 行高，导致旧 R49 夹具的“紧贴”几何断言失败。
- 修复：仅为 R49 几何测试提供临时文件数据库和空 `config.toml`，等待 Skills projection 完成，并断言计数为 `Some(0)` 且没有 controller error 后再做布局断言。未改生产布局，也未放宽 R49 几何阈值；#79 对错误顺序、列对齐、宽度、清除行为与连续提交流程的既有断言保留。Clippy 布尔断言改为 `assert!(!reference_state.0)`。
- 代码补丁 SHA-256（相对本次 `origin/master`，覆盖 `render.rs`、本卡 GPUI 测试和 R49 测试夹具）：`b31a72a576d994f12f32c4f129a59de5759d7860ee6809f3408b56155d31a07e`。

在当前最新基线上，R49 定向 Nextest 原始输出：

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 7.70s
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
────────────
 Nextest run ID 588bfc38-8878-4f5e-829a-79847fd155fb with nextest profile: default
    Starting 1 test across 2 binaries (208 tests skipped)
        PASS [   0.181s] (1/1) vega::bin/vega window::workspace::tests::r49_utility_bar_mounts_above_the_card_only_on_the_new_task_page
────────────
     Summary [   0.181s] 1 test run: 1 passed, 208 skipped
```

在当前最新基线上，#79 定向 Nextest 原始输出：

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 8.54s
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
────────────
 Nextest run ID 52a0721f-2793-470e-b5cf-dece4861d8db with nextest profile: default
    Starting 1 test across 1 binary (481 tests skipped)
        PASS [   0.048s] (1/1) vega_ui conversation_stream::tests::issue79_composer_error::issue79_errors_stay_above_composer_across_draft_and_session_routes
────────────
     Summary [   0.049s] 1 test run: 1 passed, 481 skipped
```

修复后定向 Clippy / 格式检查：

```text
$ cargo clippy -p vega_ui --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.01s
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`

$ cargo clippy -p vega --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.79s
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`

$ rustfmt --edition 2024 --check crates/vega/src/window/workspace.rs crates/vega_ui/src/conversation_stream/tests/issue79_composer_error.rs
<no output; exit 0>

$ git diff --check
<no output; exit 0>
```

- Residuals：未重跑整个 workspace Nextest；未推送，所以没有新的云端 CI run。两个改动 crate 的定向 Clippy、格式检查，以及 R49 和 `issue79_` 定向 Nextest 均通过。旧 CI 的 `block v0.1.6` future-incompatibility warning 仍存在，和本卡无关。

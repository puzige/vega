# Issue #296 · 运行终止提示交付

依据：[冻结规格](vega-issue-296-terminal-notices.md)。基线 `f1cf5ee7c2b47c8e36a0d24385b5fb4020df969f`，分支 `feat/296-stream-polish`。证据均使用合成数据。

`run_notice.rs` 删除工具 Failed / Rejected / Cancelled 派生的尾部提示、工具扫描和 fallback。保留 typed failure、Failed / Interrupted 终态、message owner、唯一提示、具体诊断优先、Composer 去重及共用高度/失效链路。工具自身状态、详情、安全交互及原文不变。删除唯一生产调用后，`tool_card.rs` 中既有 `compact_visible_text` 仅用于测试，按规格加 `#[cfg(test)]`。

## 验收证据

| 契约 | 进程内证据 |
|---|---|
| N01/N02 | P03 在每次工具失败、拒绝、取消后立即画帧，断言仍 Running、无尾卡；继续思考/正文/成功工具后 Final 收起、重开和再次收起均无尾卡，保留各工具状态和详情输出 |
| N01/N04 | 新增一条 history 对照：只有工具、Done、Failed、Interrupted；前两者无尾卡，后两者分别保留唯一失败/中断提示，工具状态与输出、正文原文均保留 |
| N03/N04 | 首帧测试保持一次完整 Window draw、模型由空到同步的断言，无额外工具卡；随后真实 Error 产生唯一 Runtime 尾部提示且 Composer 去重。P04 多轮、多片段、晚到 Summary、具体 owner、独立同文案错误及空正文失败原断言通过 |
| N05 | 拒绝工具与待审批场景只迁移尾卡预期，权限卡及工具自身拒绝原文保持；既有权限、独立 Composer 错误、产物外显及 App A7-03 安全错误入口回归通过 |
| N06 | P05 全程无工具尾卡，保留 40 段原文字节、source identity、Completed Summary 后锚点恢复、实际 item/offset 和非追尾阅读断言；空失败正文高度与唯一终态提示估算原断言通过 |

仅迁移本规格明确替代的 P03、P05、首帧和拒绝工具/权限用例中的工具尾卡预期。没有更改运行状态、协议、持久化、错误分类或主题。

## 命令与原始结果

日志目录：`~/Workspace/artifacts/vega-296-terminal-notices/`。以下命令均使用本卡 worktree 默认 target，无本地全量或 Clippy。

先在旧实现上复现 P03，退出码 `100`，`red.log`：首个 Failed 工具后仍 Running，却绘制了 `run-notice-polish`。

```sh
cargo nextest run -p vega_ui -E 'test(/issue296_polish_p03/)'
```

```text
Summary [   0.049s] 1 test run: 0 passed, 1 failed, 716 skipped
SHA256 eec3e57bbbd24432e9af6b79060f33ea3d53970c69808a74497bf5d9b210b892
```

实现后核心定向，退出码 `0`，`green-first.log`：

```sh
cargo nextest run -p vega_ui -E 'test(/issue296_polish_p03/) | test(/issue296_polish_p04/) | test(/issue296_polish_p05/) | test(/issue296_terminal_notices_/) | test(/issue296_expansion_p07_first_render/) | test(/issue296_expansion_p07_empty_failure/) | test(/issue296_phase_rejected_tools_and_permissions/)'
```

```text
Summary [   0.084s] 8 tests run: 8 passed, 710 skipped
SHA256 cc05cf335723db26b985f8827605c37ed2b9f68de1b5bdf702d650cd9ae672dc
```

该次编译发现上述 helper 只剩测试调用的 dead-code 警告。加 `#[cfg(test)]` 后运行既有增量回归，退出码 `0`，`regression.log`，无本 crate 警告；未重复已绿 8 项。

```sh
cargo nextest run -p vega_ui -p vega -E 'test(/issue146_empty_runtime_failure/) | test(/issue146_terminal_failure_cancel/) | test(/r70_hydrated_failure_after_last_tool/) | test(/transcript_hides_completed_statistics/) | test(/issue79_errors_stay_above_composer/) | test(/permission_queue_installs_matching_card/) | test(/terminal_for_old_card_does_not_clear_new_permission/) | test(/issue70_t70_5_lifecycle/) | test(/issue70_e70_live_bash_elapsed/) | test(/issue296_single_activity_visible_artifact/) | test(/a7_first_submit_provider_quota_failure/) | test(/a7_provider_http_failure/) | test(/a7_foreign_runtime_error/)'
```

```text
Summary [   0.347s] 13 tests run: 13 passed, 961 skipped
SHA256 85a7986b23c520b8dbe5d2c48561e781f3f7dcd95635a44bf19a52f22ebde6ed
```

`cargo fmt -p vega_ui -- --check`、`git diff --check` 均退出 `0`。日志中的既有依赖 `block v0.1.6` future-incompatibility 提示仍在；本轮不修改依赖。没有 fixture 失败或测试重试。

## 冻结与限制

代码冻结：`2026-10-13 03:00:18 UTC` / `2026-10-13 11:00:18 +08:00`。环境：macOS 15.8.1 / Darwin arm64；rustc `1.98.0 (88d9e12ae 2026-08-18)`、cargo `1.98.0 (797e8a9bc 2026-08-05)`（Homebrew），git `2.55.0`。

源码 fingerprint 为 `cb1606f2bcfe73219be839e1f5b3f6d2f426ab625bcb2719849f9547c76ace74`：对本次五个 Rust 文件按相对路径排序，生成 `sha256 + 两个空格 + path + 换行` 的 manifest 后再取 SHA256。文件为 `conversation_stream/run_notice.rs`、`conversation_stream/tests/{issue296_expansion,issue296_phases,issue296_stream_polish}.rs`、`tool_card.rs`，均在 `crates/vega_ui/src/` 下。

M01 未执行：用户需在合并后的原生应用确认可恢复工具失败不再有重复尾卡，真正失败或中断仍有明确提示。本轮未安装、启动或操作原生应用；云端全量门禁由主控 PR 执行。无数据迁移，回滚为撤回本轮 UI 展示改动。

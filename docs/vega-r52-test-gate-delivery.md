# Vega R52 负载敏感测试隔离交付

- **日期**：2026-09-13
- **基线**：`master @ 1c23fc4`
- **分支**：`chore/isolate-load-sensitive-tests`
- **worktree**：`/Users/puzige/Workspace/vega-test-gate`
- **Spec**：[vega-r52-test-gate.md](vega-r52-test-gate.md)（SPEC FROZEN）

## 交付结果

全量门禁的并行模式现在可用。9 个断言墙钟时间**上界**的测试改为 `#[ignore = "load-sensitive: ..."]`，代码与断言强度原样保留，可用 `--ignored` 跑回来。新增 1 个冻结测试，断言被禁用集合不能悄悄扩大。

**没有删除任何测试体，没有放宽任何断言，没有改任何期望值，没有改产品代码。** 本轮改动只有 9 行属性 + 1 个新测试 + 本文档。

## 实测耗时对比

同一 worktree、同一共享 target、单次串行执行（未并行跑两个 cargo 进程）：

| 门禁命令 | 耗时 | 结果 |
|---|---|---|
| `cargo test --workspace`（并行，新日常门禁） | **79–89 s**（3 轮：85 / 79 / 80 / 80 s） | 1052 passed / 0 failed / 9 ignored |
| `cargo test --workspace -- --test-threads=1`（旧日常门禁） | **399 s** | 1052 passed / 0 failed / 9 ignored |
| `cargo test --workspace -- --ignored --test-threads=1`（完整门禁第二步） | **16 s** | 9 passed / 0 failed |

并行比串行快 **5.0 倍**（399 s → 80 s），与 spec §1 记录的 4.7 倍一致。被禁用的 9 个测试串行单独跑只要 16 s，完整门禁两步合计约 96 s，仍比旧串行门禁快 4.1 倍。

## 被禁用测试清单及预算

| # | 文件 | 测试函数 | 预算 | 判定依据（源码行） |
|---|---|---|---|---|
| 1 | `crates/vega_conversation/src/agent/tests/stream_persistence.rs` | `lone_text_delta_flushes_during_provider_stall_within_sixteen_ms` | **16 ms** | `assert!(display_delay.unwrap() < Duration::from_millis(16))` |
| 2 | `crates/vega_conversation/src/agent/tests/tool_lifecycle.rs` | `cancellation_is_persisted_as_interrupted_under_one_second` | 1000 ms | `assert!(started.elapsed() < Duration::from_secs(1))` |
| 3 | `crates/vega_conversation/tests/restart_repair_e2e.rs` | `duplicate_stop_races_converge_to_exactly_one_terminal_event` | 1000 ms | `assert!(started.elapsed() < Duration::from_secs(1))` |
| 4 | `crates/vega_conversation/tests/restart_repair_e2e.rs` | `one_hundred_case_delay_matrix_converges_with_p99_under_one_second` | 1000 ms | `assert!(p99 < Duration::from_secs(1))` |
| 5 | `crates/vega_runtime/src/agent/tests/loop_tools.rs` | `cancellation_stops_a_delayed_provider_under_one_second` | 1000 ms | `assert!(started.elapsed() < Duration::from_secs(1))` |
| 6 | `crates/vega_runtime/src/openai/tests/http.rs` | `retry_429_honors_retry_after_header` | **500 ms** | `assert!(started.elapsed() < Duration::from_millis(500))` |
| 7 | `crates/vega_runtime/src/openai/tests/http.rs` | `cancel_during_backoff_aborts_without_another_request` | 2000 ms | `assert!(started.elapsed() < Duration::from_secs(2))` |
| 8 | `crates/vega_runtime/src/openai/tests/http.rs` | `cancel_mid_stream_stops_immediately_with_no_further_events` | 1000 ms | `assert!(started.elapsed() < Duration::from_secs(1))` |
| 9 | `crates/vega_conversation/src/git_workspace/trusted_git/tests/summary_draft.rs` | `draft_deadline_covers_setup_pre_done_and_post_done_stalls` | 1000 ms × 3 phases | `assert!(started.elapsed() < Duration::from_secs(1))`（Setup/PreDone/PostDone 各一次） |

标注统一为 `#[ignore = "load-sensitive: asserts a wall-clock budget (<预算>), fails under parallel test load; run with --ignored"]`，属性顺序为 `#[tokio::test]` 之后、`fn` 之前（spec §2.2）。

spec §2.3 明确要求**保持启用**的测试一个都没动，包括 `resume_repairs_stale_rows_then_runs_exactly_one_provider_round`（5000 ms）、`commit_summary_stderr_overflow_is_fully_drained_then_rejected`（`READ_TIMEOUT` 是语义）、`production_cancel_and_total_deadline`（17000 ms）等。**所有下界断言（`elapsed() >= ...`）原样保留**，包括 `http.rs` 的 `retry_429_without_retry_after_falls_back_to_backoff`（`>= 70ms`）与 `summary_draft.rs` 的 `>= 20ms`。

## 冻结测试

`crates/vega/src/tests.rs::r52_load_sensitive_ignores_are_frozen`，与既有门禁测试 `r21_default_window_geometry_is_frozen` 同族（spec §2.5 首选位置）。

Rust 无运行时反射枚举 `#[ignore]`，因此按 spec 的实现注记做源码扫描，但扫描范围**强于** spec 要求的 5 个文件：递归遍历 `crates/` 与 `xtask/` 下全部 `.rs`（跳过 `target/`），断言

1. 全树 `#[ignore` 出现次数与 §2.1 的 9 个函数名集合**逐一相等**（多一个、少一个、名字不符都失败）；
2. 每处 `#[ignore` 都必须带 `= "..."` 字符串 reason；
3. 每个 reason 必须以 `load-sensitive:` 开头。

扫描全树而非 6 个文件，是因为"防止后人把无关测试也 `#[ignore]` 掉"（spec §2.5 原话）需要覆盖**新增文件**——只扫 6 个已知文件会漏掉在别处新加的 `#[ignore]`，那正是要防的情况。

路径用 `env!("CARGO_MANIFEST_DIR")` 起跳、逐级上溯到含 `[workspace]` 的 `Cargo.toml`，因此不依赖进程 cwd，在任意 worktree、任意 target 目录、`cargo test -p vega` 或 `cargo test --workspace` 下都成立（不硬编码 worktree 名）。

冻结测试经 3 个反向探针验证非空转，全部按预期变红后还原：

| 探针 | 注入 | 结果 |
|---|---|---|
| A | 新增一个 `#[ignore = "load-sensitive: probe"]` 测试 | FAILED，`left` 比 `right` 多出 `r52_probe_extra_disabled_test` |
| B | 把一处改成裸 `#[ignore]` | FAILED，`#[ignore must carry a reason` |
| C | 把一处 reason 改成 `"flaky: probe"` | FAILED，``#[ignore` reason must start with `load-sensitive:`` |

## 验收门禁证据

日志目录 `/tmp/r52-logs/`。所有命令在 `/Users/puzige/Workspace/vega-test-gate` 单次串行执行，无并发 cargo。

| # | 命令 | 结果 | 日志（SHA256 前 16 位） |
|---|---|---|---|
| 1 | `cargo test --workspace` | exit 0，**1052 passed / 0 failed / 9 ignored**，85 s | `par-a.log` `12e8f9d4f67ce312` |
| 3 | `cargo test --workspace`（第 2 轮） | exit 0，1052 / 0 / 9，79 s | `par-b2.log` `f74519ea8429cf64` |
| 3 | `cargo test --workspace`（第 3 轮） | exit 0，1052 / 0 / 9，80 s | `par-c.log` `e784fe58d9551510` |
| 3 | `cargo test --workspace`（第 4 轮） | exit 0，1052 / 0 / 9，80 s | `par-d.log` `e4a44017f00d85b0` |
| 2 | `cargo test --workspace -- --ignored --test-threads=1` | exit 0，**9 passed / 0 failed**，16 s | `ignored.log` `5d71969aa51241c9` |
| 4 | `cargo fmt --all -- --check` | exit 0，无输出 | — |
| 5 | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 | `clippy.log` |
| 附 | `cargo test --workspace -- --test-threads=1`（旧门禁基线） | exit 0，1052 / 0 / 9，399 s | `serial.log` `ff9e9d17b95ff89a` |

计数自洽：1052 passed + 9 ignored = 1061 = spec 基线 1060 + 本轮新增的 1 个冻结测试。

### 验收 #2 的完整输出（本轮关键证据）

9 个被禁用测试全部通过，证明它们只是被隔离，不是坏了：

```
test agent::tests::stream_persistence::lone_text_delta_flushes_during_provider_stall_within_sixteen_ms ... ok
test agent::tests::tool_lifecycle::cancellation_is_persisted_as_interrupted_under_one_second ... ok
test git_workspace::trusted_git::tests::summary_draft::draft_deadline_covers_setup_pre_done_and_post_done_stalls ... ok
test duplicate_stop_races_converge_to_exactly_one_terminal_event ... ok
test one_hundred_case_delay_matrix_converges_with_p99_under_one_second ... ok
test agent::tests::loop_tools::cancellation_stops_a_delayed_provider_under_one_second ... ok
test openai::tests::http::cancel_during_backoff_aborts_without_another_request ... ok
test openai::tests::http::cancel_mid_stream_stops_immediately_with_no_further_events ... ok
test openai::tests::http::retry_429_honors_retry_after_header ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

## 串行规矩的来源与本次修正的理由

仓库**没有**任何配置强制 `--test-threads=1`：`.cargo/config.toml` 只有 `xtask` alias，`rustfmt.toml`/`clippy.toml` 均为默认值。该参数从 R11 起写进交付文档的命令行，之后逐轮被抄成惯例。

它最初的动因是**三个 `cargo test` 进程同时跑**互相抢 git fixture —— 那是**进程间**竞争，与单进程内测试函数是否并行无关。共享 target 后 cargo 用文件锁把并发构建串行化，该动因已结构性消失。

本轮的修正：日常门禁改用并行；负载敏感的上界断言测试用 `#[ignore]` 隔离，合并前或涉及并发改动时用 `--ignored --test-threads=1` 单独跑回来。**不再**用 `cargo test --workspace -- --test-threads=1` 作为日常门禁。

## 本轮暴露的两个既有偶发失败（不属于本轮禁用集合）

并行跑第 1 轮（后经查明为陈旧二进制，见下节）与一次中途轮次各出现 1 个失败，**均不在 spec §2.1 的 9 个之列，本轮按 spec 边界未做任何处理**，只记录事实：

1. `git_workspace::trusted_git::tests::runner_mutation::service_reports_authoritative_state_after_add_and_commit_process_failures` — `add plan nonzero` 期望 `GitFailed`，实际 `TimedOut`（3 s mutation 期限）。**既有已知偶发**：`docs/vega-r12-acceptance.md` 记录了同一测试的 `stdout-overflow` 分支 `TimedOut`，并明确"支持时序敏感性，不能将最终全量标绿"。孤立运行 1/0/0，30.78 s。
2. `git_workspace::branch::tests::lease_cleanup::refresh_registered_before_owner_cannot_commit_after_lease_acquisition` — `owner did not enter mutation barrier`，是 500×2 ms 的屏障等待超时。孤立运行 1/0/0。
3. `tests::diff::diff_refresh_intents_keep_content_during_background_and_retry` — retry 终态 `refresh_error=Some(GitFailed)`。**既有已知偶发**：`docs/vega-r6-diff-refresh-delivery.md`、`vega-r8-zcode-parity.md`、`vega-r9-workspace-panels-delivery.md`、`vega-r11-acceptance.md`、`vega-review-current-status.md` 均有同一失败记录。孤立运行 1/0/0，0.90 s。

三者都在孤立运行时通过，都在高 CPU 争抢（Spotlight `mds_stores` 100%+，另一 worktree 并发跑 `cargo test`）时失败，形态与本轮要解决的负载敏感问题同类，但 spec §2.1 未列它们、§2.3 也未列它们，故**未加 `#[ignore]`、未改断言、未改超时**。建议后续单独一轮按 §2.1 同一判定标准重新审计（尤其这三处都含上界等待或紧超时）。

## 验收复核：CPU 满载不是充分条件（主 Agent 追加）

主 Agent 在合并前独立复核时追加了一组对照实验，**修正了上面对偶发失败成因的归因**，记录如下。

复核中另观察到一次失败：`vega_conversation::terminal::tests::owned_login_pty_persistent_interrupt_resize_exit_and_reap`（3 轮全量并行中 1 次失败，孤立运行 0.60 s 通过）。该测试内部用 `deadline = Instant::now() + Duration::from_secs(8)` 等待真实 PTY 拉起登录 shell，属上界等待，形态与上节三者同类。

**对照实验（决定性）**：

| 条件 | 结果 |
|---|---|
| 全量并行 × 4 轮，机器空闲 | **4/4 干净**（1052 passed / 0 failed） |
| 仅 `-p vega_conversation -p vega_ui` × 4 轮 | 4/4 干净（543/0） |
| 全量并行 + **10 核 `yes` 满载** | **1052 passed / 0 failed** |

**结论：单纯 CPU 争抢不足以复现这些失败。** 10 核饱和下全量门禁仍然全绿。因此上节把成因归于"高 CPU 争抢"是不完整的——真正触发条件是**同一 target 目录被另一个 worktree 并发使用**（产物串味 + 测试期无锁），而不是机器负载本身。本仓库现已提供 `scripts/cargo-lock.sh` 把构建与测试整体串行化，从机制上消除该触发条件。

**据此对后续审计的建议修正**：这三处（以及 PTY 那个）是否需要隔离，取决于**锁是否生效**，而非机器快慢。在 `cargo-lock.sh` 已生效的前提下，先按现状跑若干轮全量门禁观察；若仍复现，再按 §2.1 标准审计并隔离。不要仅凭"机器忙"就判定为负载敏感——那会掩盖真正的产物串味问题。


## 环境勘误：共享 target 导致的陈旧二进制

`target/` 是指向 `/Users/puzige/Workspace/vega/target` 的符号链接，`vega-r50-sidebar-rhythm`、`vega-r51-tabs`、`vega-skill` 等 worktree 也链接同一目录。

首次并行运行（89 s，日志 `parallel-1.log`）报告 1056 passed / 4 ignored：`vega` 与 `vega_conversation` 的测试二进制其实是 **`vega-r50-sidebar-rhythm` worktree 构建的**（`.d` dep-info 首行指向该路径），cargo 的 mtime 新鲜度判定把它们当成可复用产物，因此本 worktree 的 3+2 个 `#[ignore]` 与冻结测试**根本没进入该次运行**。

修正：`touch` 本 worktree 改动的 7 个源文件强制重编，随后校验 `.d` 首行均指向 `vega-test-gate`，并用 `--list --ignored` 确认 4+3+2=9 个忽略项与冻结测试都在二进制里。**本文档上表的所有计时与计数均来自修正后的运行。**

后续在本机跑门禁时，建议先用 `.d` 首行或 `--list --ignored` 校验产物归属，避免把别的 worktree 的结果当成自己的。

## 边界

未 push、未创建 MR、未提交到 master。未改产品代码，未改其他测试，未删除测试体，未放宽断言或超时。性能/并发性的正确处理（把上界断言改成不依赖墙钟的机制）按用户裁决延期到性能阶段，本轮只做隔离与冻结。

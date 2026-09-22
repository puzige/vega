# R52 · 隔离负载敏感的测试，让并行门禁可用

> 2026-09-22 [Issue #140](vega-issue-140-ci-test-throughput.md) supersedes the shadow selector and per-shard compilation topology: Python shadow reporting is removed; full-suite native nextest archives are built once and reused across workers. Existing safety assertions, resource weights and ignored inventory remain.

> 状态：**SPEC FROZEN**
> 基线：`master @ 1c23fc4`
> 分支：`chore/isolate-load-sensitive-tests`
> worktree：`/Users/puzige/Workspace/vega-test-gate`
> 动机：当前全量门禁必须 `--test-threads=1`，实测 **416 秒**；默认并行 **89 秒**（快 4.7 倍），但有 1 个负载敏感测试会偶发失败。用户决策：**先把容易出问题的测试禁用掉**，并发性的正确处理放到最后（现阶段还在改 UI，没到性能阶段）。

---

## §1 背景（实测数据）

| 运行方式 | 耗时 | 结果 |
|---|---|---|
| `cargo test --workspace -- --test-threads=1` | **416 s** | 1060 passed / 0 failed |
| `cargo test --workspace`（默认并行） | **89 s** | 1060 passed / 0 failed（3 轮中 2 轮干净） |
| 其中 `vega_conversation` 单 crate | 串行 312 s → 并行 52 s | |

串行规矩的来源已查明：仓库**没有**任何配置强制它（`.cargo/config.toml` 只有 `xtask` alias）。`--test-threads=1` 从 R11 起被写进交付文档的命令行，之后逐轮被抄成惯例。它最初的动因是**三个 `cargo test` 进程同时跑**互相抢 git fixture —— 那是**进程间**竞争，与单进程内测试函数是否并行无关。共享 target 后 cargo 用文件锁把并发构建串行化，该动因已结构性消失。

已观测到的偶发失败（并行时）：

```
one_hundred_case_delay_matrix_converges_with_p99_under_one_second
  panicked at crates/vega_conversation/tests/restart_repair_e2e.rs:594
```

单独运行时实测 `p50=107ms p99=189ms max=190ms`，预算 1000ms（**5.3 倍余量**）。即该测试本身健康，只在 CPU 争抢时超标。

---

## §2 契约

### 2.1 处理对象：所有断言**墙钟时间上界**的测试

判定标准：测试体内存在对 `Instant::elapsed()` 的**上界断言**，且上界是绝对时间预算。这类断言在并行负载下可能失败，与产品正确性无关。

**本轮禁用的测试（9 个）**——已逐个核对属性行与预算（第 10 个见下方 2026-09-22 追加）：

| # | 文件 | 行 | 测试函数 | 预算 |
|---|---|---|---|---|
| 1 | `crates/vega_conversation/src/agent/tests/stream_persistence.rs` | 329 | `lone_text_delta_flushes_during_provider_stall_within_sixteen_ms` | **16 ms** |
| 2 | `crates/vega_conversation/src/agent/tests/tool_lifecycle.rs` | 751 | `cancellation_is_persisted_as_interrupted_under_one_second` | 1000 ms |
| 3 | `crates/vega_conversation/tests/restart_repair_e2e.rs` | 452 | `duplicate_stop_races_converge_to_exactly_one_terminal_event` | 1000 ms |
| 4 | `crates/vega_conversation/tests/restart_repair_e2e.rs` | 506 | `one_hundred_case_delay_matrix_converges_with_p99_under_one_second` | 1000 ms |
| 5 | `crates/vega_runtime/src/agent/tests/loop_tools.rs` | 592 | `cancellation_stops_a_delayed_provider_under_one_second` | 1000 ms |
| 6 | `crates/vega_runtime/src/openai/tests/http.rs` | 240 | `retry_429_honors_retry_after_header` | **500 ms** |
| 7 | `crates/vega_runtime/src/openai/tests/http.rs` | 462 | `cancel_during_backoff_aborts_without_another_request` | 2000 ms |
| 8 | `crates/vega_runtime/src/openai/tests/http.rs` | 500 | `cancel_mid_stream_stops_immediately_with_no_further_events` | 1000 ms |
| 9 | `crates/vega_conversation/src/git_workspace/trusted_git/tests/summary_draft.rs` | 367 | `draft_deadline_covers_setup_pre_done_and_post_done_stalls` | 1000 ms ×3 phases |

**2026-09-22 追加（issue #123）**：第 10 个负载敏感测试入列——`crates/vega_conversation/src/mcp_settings.rs` 的 `issue73_connection_deadline_covers_probe_and_catalog_together`（断言 probe <280 ms、总耗时 <700 ms 的共享 deadline 预算）。该测试在 CI 并行负载下偶发失败并阻塞 PR check，按用户指示先临时禁用（测试体与断言原样保留，`cargo test -- --ignored` 可手动运行）。`R52_LOAD_SENSITIVE_TESTS` 冻结清单已同步从 9 项更新为 10 项。

### 2.2 机制：`#[ignore]`，不删测试

**必须用 `#[ignore = "<reason>"]`，禁止删除测试体或删除断言。**

理由：`#[ignore]` 是 Rust 标准的"禁用但保留"机制，代码与断言强度原样保留，随时可用 `--ignored` 跑回来。删除会永久丢失并发/取消/持久化的覆盖，而这些正是最容易出隐蔽 bug 的地方。

标注格式（reason 必须说明是负载敏感，并给出预算）：

```rust
#[ignore = "load-sensitive: asserts a wall-clock budget (<16ms), fails under parallel test load; run with --ignored"]
```

属性顺序：`#[ignore]` 放在 `#[tokio::test]` **之后**、`fn` 之前。

### 2.3 明确**不**禁用（余量充足，属真实行为断言）

这些同样有墙钟断言，但预算宽松，并行下已验证稳定，**保持启用**：

| 文件 | 测试 | 预算 | 理由 |
|---|---|---|---|
| `restart_repair_e2e.rs:166` | `resume_repairs_stale_rows_then_runs_exactly_one_provider_round` | 5000 ms | 余量大 |
| `restart_repair_e2e.rs:241` | `retry_429_honors_retry_after_header`（runtime 版） | — | 无紧上界 |
| `caps_runner.rs:343` | `commit_summary_stderr_overflow_is_fully_drained_then_rejected` | `READ_TIMEOUT` | 超时是语义，非性能 |
| `caps_runner.rs:484` | `commit_summary_deferred_overflow_never_eof_uses_bounded_timeout` | 3000 ms | 余量大 |
| `lifecycle.rs:4` | `git_workspace_read_timeout_is_typed_and_bounded` | `READ_TIMEOUT+3s` | 余量大 |
| `provider_settings/tests.rs:317` | `production_cancel_and_total_deadline` | 17000 ms | 余量极大 |
| `permission_flow.rs:537` | `running_bash_cancellation_waits_for_process_reap` | 2000 ms | 余量大 |
| `runner_mutation.rs:137` | `trusted_mutation_runner_times_out_cancels_and_reaps_process_groups` | 循环等待，非断言 | 不是断言 |

**下界断言（`elapsed() >= ...`）一律保留**：它们验证"确实等了足够久"，不受负载影响（负载只会让它更容易通过）。

### 2.4 门禁命令

两个命令，各司其职：

```sh
# 快速门禁（日常 / UI 轮次）：并行，跳过负载敏感测试
cargo test --workspace

# 完整门禁（合并前 / 涉及并发改动时）：先并行跑常规，再单独跑被禁用的
cargo test --workspace
cargo test --workspace -- --ignored --test-threads=1
```

**禁止**再用 `cargo test --workspace -- --test-threads=1` 作为日常门禁——它比并行慢 4.7 倍，且当初的理由（进程间 git fixture 竞争）已被共享 target 的文件锁消除。

**2026-09-22 后续（issue #123）**：云端 PR 门禁的常规测试改由 `cargo nextest run --workspace` 执行（配置 `.config/nextest.toml`，`retries = 0`），语义与上面「快速门禁」一致：并行、跳过 `#[ignore]` 的负载敏感测试；因 nextest 不跑 doc-tests，workflow 另加 `cargo test --workspace --doc`。本地命令不变，`cargo test --workspace` 仍可用。用 nextest 单独跑被隔离的 10 个测试：`cargo nextest run --workspace --run-ignored ignored-only --test-threads=1`（实测 10 passed）。

**同日 C9 提速**：云端使用 4 个 macOS runner 执行 `--partition hash:<shard>/4`，quality job 并行跑 fmt/clippy/doc-tests。`vega_conversation` 的 `git_workspace::trusted_git::` 测试设置 `threads-required = 2` 以降低重型测试并发竞争；该权重不保证消除 flaky，也不增加单测 CPU 数。首轮云端发现取消/pricing 两条测试失败：按 C9 对 pricing 测试及后续暴露 8 秒超时的真实 PTY 测试精确设置 `threads-required = "num-test-threads"`；取消测试在独占运行仍失败，改用仅测试构建可见的同步点替代 2ms 定时猜测，保留全部断言和真实文件读取。保留既有 10 项 ignored 清单，不新增 ignore 或重试；独占调度不保证消除测试内在时钟竞态。所有分片必须成功才能通过汇总门禁。

### 2.5 冻结测试

新增一个测试，断言"被禁用的测试集合"不会悄悄扩大：遍历 `#[ignore]` 标注，要求每个 reason 都以 `load-sensitive:` 开头。防止后人把无关测试也 `#[ignore]` 掉。

> 实现注记：Rust 无运行时反射枚举 `#[ignore]`。用源码扫描实现：读取本文件列表中的 5 个源文件，断言其中 `#[ignore]` 出现次数 == 9，且每处紧跟 `load-sensitive:`。该测试放在 `crates/vega_conversation/tests/` 或作为 `xtask` 检查，由实现者择一（优先放 `crates/vega/src/tests/`，与既有门禁测试同族）。

---

## §3 交付物

1. 9 处 `#[ignore = "load-sensitive: ..."]` 标注
2. 1 个"禁用集合不扩大"的冻结测试
3. 交付文档 `docs/vega-r52-test-gate-delivery.md`，含：
   - 两条门禁命令的实测耗时对比
   - 被禁用测试清单及预算
   - 串行规矩的来源与本次修正的理由
4. **不改**任何测试断言、不改产品代码

## §4 验收门禁

| # | 证据 | 判据 |
|---|---|---|
| 1 | `cargo test --workspace`（并行） | 0 失败，耗时 ≈89 s，被禁用测试显示为 ignored |
| 2 | `cargo test --workspace -- --ignored --test-threads=1` | 9 个被禁用测试**全部通过**（证明只是隔离，不是坏了） |
| 3 | 连续 3 轮并行 `cargo test --workspace` | 0 失败（消除偶发） |
| 4 | `cargo fmt --all -- --check` | 干净 |
| 5 | `cargo clippy --workspace --all-targets -- -D warnings` | 干净 |

**验收 #2 是本轮的关键证据**：它证明这 9 个测试本身是好的，只是不能在并行负载下跑——而不是我们掩盖了真 bug。

## §5 提交约定

- 分支 `chore/isolate-load-sensitive-tests`，worktree `/Users/puzige/Workspace/vega-test-gate`
- **不 push、不创建 MR**
- Conventional Commits，scope `A6-02`
- 测试运行**单次串行**，不要并行跑两个 cargo 进程

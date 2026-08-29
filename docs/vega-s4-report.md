# ✦ Vega — S4 验收报告（Runtime 核心）

**日期** 2026-08-30 · **范围** T19-T22 · **结论** S4 headless DoD 达成

## 1. 交付范围

| 任务卡 | 交付 | PR | master squash commit |
|---|---|---|---|
| T19 | `Provider`/`BoxFuture` 抽象、OpenAI-compatible SSE、重试/取消、`MockProvider` | #19 | `cffd69a` |
| T20 | 100 次上限的 agentic 循环、上下文组装、事件转换、异步落库与恢复 | #21 | `cec9b75` |
| T21 | 带路径围栏的 `read`/`glob`/`grep` 只读工具 | #20 | `6f09907` |
| T22 | 「找出 repo 里所有 TODO」完整 mock→工具→observe→落库验收 | #22 | 本 PR squash merge 后生成 |

T22 实现提交：

- `5da6793 feat(A3-03): prove TODO discovery end to end`
- `feat(A3-03): document S4 acceptance`（本报告与 README 状态）

T22 的集成测试放在 `vega_conversation/tests/todo_e2e.rs`。这是完整 T20 编排与 SQLite 持久化的所有者边界；测试仍通过公开 API 驱动 `vega_runtime`、真实 `vega_tools` 和 `vega_store`，避免为了测试文件位置反转 `vega_conversation → vega_runtime` 的依赖方向。验收语义无变化。

## 2. T22 端到端证据

临时项目预埋 Rust、Python、TypeScript、Markdown 四类 TODO。`MockProvider` 按真实 observe 流程执行三轮：

1. 收到 system + 用户任务，调用一次真实 `grep`；
2. 从下一轮请求中观察 grep 输出，针对四个命中文件串行调用真实 `read`；
3. 从最终请求中观察全部 read 输出，返回包含全部路径与 TODO 原文的清单并 `end`。

测试断言：

- provider 请求恰好 3 轮，工具定义始终为 `read`/`glob`/`grep`；
- 最终输出包含 4/4 预埋 TODO 的文件路径和原文；
- 后一轮请求确实携带前一轮的真实工具结果，不只验证预制最终文本；
- `messages` 2 行（user/assistant，均 `done`）；
- `tool_calls` 5 行（1 grep + 4 read，顺序连续，`success`/`once`，输出和完成时间完整）；
- `token_usage` 3 行（逐 provider 调用落库，model/message 归属正确，S4 计价占位均为 0）；
- SQLite 仍恰好只有六表；测试本身无 `unwrap()`/`expect()`，不使用真实 LLM、API key 或网络。

聚焦命令原始摘要：

```text
$ cargo test -p vega_conversation --test todo_e2e -- --nocapture
running 1 test
test finds_every_seeded_todo_with_real_tools_and_persists_the_run ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

## 3. 四门禁与测试总数

```text
$ cargo fmt --all -- --check
exit 0

$ cargo clippy --workspace --all-targets --all-features -- -D warnings
Finished `dev` profile ...
exit 0

$ cargo test --workspace --all-features
245 passed; 0 failed; 1 ignored
5 doctests passed; 0 failed
exit 0

$ cargo build --workspace --all-features
Finished `dev` profile ...
exit 0
```

测试分布：

| crate / suite | passed | ignored |
|---|---:|---:|
| `vega_conversation` unit | 55 | 0 |
| T22 `todo_e2e` integration | 1 | 0 |
| `vega_markdown` | 32 | 0 |
| `vega_runtime` | 54 | 0 |
| `vega_store` | 43 | 1（真实 macOS Keychain，按设计仅手工运行） |
| `vega_theme` | 6 | 0 |
| `vega_tools` | 21 | 0 |
| `vega_ui` | 33 | 0 |
| doctest | 5 | 0 |
| **合计** | **250** | **1** |

`clippy`/`test`/`build` 仅报告上游 `block 0.1.6` future-incompatibility 提示，不是当前 warning lint 或失败。

## 4. Sprint DoD 对照

| DoD | 结果 | 证据 |
|---|---|---|
| T19-T22 全绿 | ✅ | 全 workspace 245 个单元/集成测试 + 5 个 doctest；四门禁 exit 0 |
| headless「找出所有 TODO」 | ✅ | T22 三轮完整编排；真实 grep/read；4/4 TODO；messages/tool_calls/token_usage 落库 |
| 中断 `<1s` | ✅ | `cancellation_stops_a_delayed_provider_under_one_second`、`cancellation_is_persisted_as_interrupted_under_one_second` |
| 100 次工具上限 | ✅ | `stops_after_one_hundred_tool_calls_with_visible_notice` 与重复 call-id 第 101 次收敛测试 |
| 围栏逃逸全拒 | ✅ | `..`、绝对路径、symlink 三类 fence 测试；grep/glob/read 扩展覆盖 |
| runtime/tools headless | ✅ | `cargo tree -p vega_runtime` 与 `-p vega_tools` 的 GPUI/UI 依赖命中均为 0 |
| UI 唯一消费 `ConversationEvent` | ✅ | `crates/vega_ui`/`crates/vega` 中 `RuntimeEvent` 命中 0；转换留在 conversation 层 |
| 六表外零 DDL | ✅ | T22 查询 `sqlite_master` 精确断言六表；Rust 源 DDL 命中 0；migration 仍仅 `0001_init.sql` |
| bench 不回退 | ✅（见限制） | release probe frozen rematerialization=0；frame build 远低于 8.33ms 帧预算 |
| hooks/master 绿 | ✅（合并门禁） | feature branch 四门禁已绿；PR #22 squash 后 master 包含同一已验代码 |

## 5. 红线复核

| 红线 | 结果 |
|---|---|
| 新增白名单外依赖 | 0；`Cargo.toml`/`Cargo.lock` 未改 |
| 真实 API key / 费用 | 0；T22 仅 `MockProvider` |
| `vega_runtime` / `vega_tools` UI 依赖 | 0 命中 |
| UI 直接使用 `RuntimeEvent` | 0 命中 |
| UI 六位硬编码色值 | 0 命中 |
| 非测试 `unwrap()` / `expect()` | 0；静态扫描唯一额外命中是 `vega_markdown` 文档示例中的 doctest `expect` |
| Rust 代码内 DDL | 0 命中；schema 未变 |
| T22 测试 `unwrap()` / `expect()` | 0 命中 |
| 用户工作区写入/删除 | 0；只操作 `tempfile` 创建的测试目录 |

## 6. `xtask bench` 与 UI Checklist

本 Sprint 只改 headless runtime/tools/conversation 测试，未改 UI。仍执行一次 release probe 作为“不回退”证据：

```text
cold_start     p50=2016ms p99=2020ms      spawn-to-exit placeholder
memory_idle    105.3 MB                   RSS after 5 s
render_frame   fps=61                     60Hz vsync cap
scroll build   p50=7.041µs p99=16.583µs
stream build   p50=46µs p99=223µs         ~500 delta/s
P3             frozen_rematerializations=0
```

| UI Spec §6 | S4 复核 |
|---|---|
| 色值/字体全部 token | ✅ `vega_ui` 六位色值 grep 0；S4 未改 UI |
| Light/Dark 无遗漏 | ↔ 未改 UI；6 个 theme 自动化测试通过，未重复人工闪烁走查 |
| CJK 混排 | ↔ 未改 UI；既有 CJK 宽度测试通过，未重复人工字体走查 |
| 键盘全流程 | ⏳ 完整「批准权限→diff→提交」依赖 S5/S6，不属于 S4 headless 范围 |
| 960×600 | ↔ 未改布局，未重复人工窗口走查 |
| P1-P8 | ⚠️ frame build/P3 无回退；60Hz 机器无法字面验证 P1=120fps；P7 仍是 spawn-to-exit 占位；P8 105.3MB 高于 100MB |
| Codex/ZCode 并排截图 | ↔ S4 无 UI 变化，未重复截图 |

## 7. 偏离与遗留

**行为/spec 偏离：无。**

已知遗留（均已在 Phase 1 计划中后置，不阻塞 S4 DoD）：

- T22 测试文件由 runtime 行为的持久化所有者 `vega_conversation` 承载；避免依赖倒置，验收覆盖未缩减。
- 真实 LLM/API key/费用与 dogfood 由人类执行；S4 只准备 mock 可重复验收路径。
- S4 计价钩子按预裁保持 `cost_microcents=0`，S7 接入定价引擎。
- P1 字面 120fps 需 ProMotion 硬件复测；P7 首帧埋点、P8 空闲内存调优均归 S8。
- 完整权限门禁、三模式、键盘批准流程归 S5；diff/commit 流程归 S6。

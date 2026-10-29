# #667 开发证据与清理清单（冻结基线）

本文件是可执行交付清单，不是完成证明或删测授权。依据 [规格 §6](vega-issue-667-claude-compaction.md#6-当前证据未知与下一步)、[测试交付政策](../vega-rust-test-delivery-lifecycle.md)及[执行指南 §7.2](../vega-exec-guide.md#72-快速交付与按风险验证当前规则)。只记录路径、内容身份和证据要求，不复制 Rust 源码、测试或恢复 patch 到最终仓库。

## 1. 冻结边界与已知状态

| 身份 | 完整值与用途 |
|---|---|
| 清单冻结开发提交 | `d404cb891909504f252f5e1a6913a4f47ce27729` |
| 冻结 tree | `31ae775d2bbd66c68cb8ce10834336963af82028` |
| 先前委派结果实现提交 | `2a670c6f92c02074e252c02fb00995de533713b9` |
| 先前 tree | `18bc17d81aa10c9864d2e60b644699f859a72709` |
| 归属比较主线 | `7a91c12217d264ab3f100fcc71571c00f1da5c40`，已合入任务历史；不用早期规格基线误归属后续主线内容 |

前次只读清点在上述两个开发提交均得到 **195 个静态测试声明、49 个测试独占文件**；主线比较点为 0 个声明。195 = 单元/GPUI 164 + runtime 集成 31；按 crate 为 `vega` 10、`vega_conversation` 97、`vega_runtime` 55、`vega_store` 26、`vega_tools` 1、`vega_ui` 6。另有 4 个含内联测试的生产混合文件。**这些是静态估计，不是 CI 发现或执行计数。** 未发现可执行 doctest；`crates/vega_markdown/src/stream.rs` 的 `text` 围栏不是测试。

本轮只持久化该清单，不重数或覆盖并行产物：runtime skill refill、conversation partial-ready 两项组合回归正在补充，可能改变文件和声明数量。主会话须在其集成后另行固定完整开发 SHA/tree、增量清单及实际验证结果，不能沿用 195 作为最新计数。历史 `2a670c6f…` 委派结果实现和 `d404cb89…` 外层入口 fixture 修复均有固定非写入者静态 Clear；**Clear 不是行为证明**。

### 待补组合验收（当前规格“跨路径验收补齐”）

- **A17**：同一运行交替经过技能预检与普通自动入口，覆盖显式/模型加载技能；第三次快速回填须在下一次摘要调用前拒绝，主请求和 hook 计数精确。
- **A08/A11/A12/A19**：从既有局部摘要及保留原文的 checkpoint 生成 Ready，合法追加后消费；验证摘要输入与最终投影标记顺序、原始任务 ID/ContinueExistingRun、即时/reload 一致、原始审计不变、已结算用量不重复计费。
- 复用真实 runtime/SQLite/规划与消费路径，仅隔离外部 provider。发现缺陷先保存证据再修责任层，不调整期望迁就实现。本清单不重复实现审计、不宣布这些回归完成。

## 2. 冻结测试独占文件：49 个

下列每个代码块以标题目录为前缀，叶子均为精确路径。清理时拟整文件删除，其内部 fixture/mock/helper 随文件删除；冻结清点未发现另设的已跟踪独占 fixture 资源目录。**必须先解决开发失败并固定恢复证据，不能现在删除。**

### `crates/vega/src/window/`

```text
context_tests.rs
context_tests/precompute_lifecycle_tests.rs
```

### `crates/vega_conversation/src/`

```text
agent/compaction/continuation_tests.rs
agent/compaction/continuation_tests/generation_tests.rs
agent/compaction/continuation_tests/generation_tests/fork_tests.rs
agent/compaction/continuation_tests/generation_tests/plan_restore_tests.rs
agent/compaction/continuation_tests/generation_tests/skill_restore_tests.rs
agent/compaction/continuation_tests/generation_tests/task_restore_tests.rs
agent/compaction/continuation_tests/micro_tests.rs
agent/compaction/continuation_tests/precompute_tests.rs
agent/compaction/continuation_tests/precompute_tests/arming_tests.rs
agent/compaction/continuation_tests/precompute_tests/attribution_entry_tests.rs
agent/compaction/continuation_tests/precompute_tests/attribution_tests.rs
agent/compaction/continuation_tests/precompute_tests/consume_tests.rs
agent/compaction/continuation_tests/precompute_tests/consume_tests/borrow_tests.rs
agent/compaction/continuation_tests/precompute_tests/consume_tests/timeout_tests.rs
agent/compaction/continuation_tests/precompute_tests/diagnostics_tests.rs
agent/compaction/continuation_tests/precompute_tests/registry_tests.rs
agent/compaction/continuation_tests/precompute_tests/registry_tests/coordination_tests.rs
agent/compaction/continuation_tests/precompute_tests/registry_tests/lifecycle_tests.rs
agent/compaction/continuation_tests/reactive_tests.rs
agent/compaction/service_tests.rs
agent/context_history/tests.rs
agent/pipeline/compaction_tests.rs
agent/subagents/precompute_tests.rs
agent/subagents/provider_tests.rs
privacy/tests.rs
```

`attribution_tests.rs` 通过非默认 `#[path]` 接入 `attribution_entry_tests.rs`；三项外层封存回归不得漏归档，不能通过先删掉它们消解失败。

### `crates/vega_runtime/`

```text
src/agent/context_files/tests.rs
src/agent/context_history/tests.rs
src/agent/context_plan/tests.rs
src/agent/context_skills/tests.rs
src/agent/context_tasks/tests.rs
src/agent/mcp_registry_tests.rs
src/openai/compaction_tests.rs
tests/compaction_micro.rs
tests/compaction_overflow.rs
tests/compaction_precompute.rs
tests/compaction_precompute/delegated_tests.rs
tests/compaction_precompute/skill_tests.rs
tests/compaction_refill.rs
tests/compaction_replay.rs
tests/context_budget.rs
tests/context_output_request.rs
```

`tests/compaction_precompute.rs` 显式引入两个子文件；顶层集成测试由 Cargo 自动发现，没有本卡新增 `[[test]]` 条目。

### `crates/vega_store/src/context_precompute/`

```text
tests.rs
tests/persistence.rs
tests/persistence/consume_tests.rs
tests/persistence/coordination_tests.rs
tests/persistence/privacy_tests.rs
```

### `crates/vega_ui/src/conversation_stream/`

```text
context_control/compact_command_tests.rs
```

## 3. 混合生产文件：仅移除独占测试部分

### 内联模块（4 文件）

| 精确路径 | 冻结测试部分；生产实现保留 |
|---|---|
| `crates/vega_conversation/src/agent/compaction/summary_variant.rs` | `#[cfg(test)] mod tests`，2 项 |
| `crates/vega_runtime/src/agent/compaction_refill.rs` | `#[cfg(test)] mod tests`，3 项 |
| `crates/vega_tools/src/read.rs` | `#[cfg(test)] mod context_tests`，1 项 |
| `crates/vega_ui/src/conversation_stream/composer_actions.rs` | 尾部测试模块，1 项 |

### 外置模块接线

以下仅移除测试模块声明及测试专用 `#[path]`，不删除生产模块：

```text
crates/vega_conversation/src/agent/compaction.rs
crates/vega_conversation/src/agent/context_history.rs
crates/vega_conversation/src/agent/pipeline.rs
crates/vega_conversation/src/agent/subagents/precompute.rs
crates/vega_conversation/src/agent/subagents/provider.rs
crates/vega_conversation/src/privacy.rs
crates/vega_runtime/src/agent/context_files.rs
crates/vega_runtime/src/agent/context_history.rs
crates/vega_runtime/src/agent/context_plan.rs
crates/vega_runtime/src/agent/context_skills.rs
crates/vega_runtime/src/agent/context_tasks.rs
crates/vega_runtime/src/agent/mod.rs
crates/vega_runtime/src/openai/mod.rs
crates/vega_store/src/context_precompute.rs
```

### 生产文件内测试 helper/替身

- `crates/vega/src/window/context.rs`：移除 `context_tests.rs` 接线、`ContextTestWorker`、`ContextController.config_path`/`.worker` 测试字段、`context_config_path()` 测试覆盖分支、`test_worker` 捕获及线程内替身分支。**保留**真实 `composer_config_path()`、线程启动、取消与完成处理。
- `crates/vega_ui/src/conversation_stream/context_control.rs`：移除 `compact_command_tests` 接线及 `context-test-support` 下的 `context_status_snapshot()`/`context_status_rows()`。**保留**真实记录、状态恢复、UI 操作和可访问性。

所有混合文件须逐段审查清理差异，保护 provider、隐私边界、持久化、原始审计、预算/计费和上下文回读生产行为及安全断言。

## 4. Manifest 与锁文件候选闭包

相对归属主线，明确独占的 manifest 增量：

| 路径 | 拟移除项 |
|---|---|
| `crates/vega/Cargo.toml` | dev-dependencies 中 `gpui_kit` 的 `test-support`、`vega_ui` 的 `context-test-support` |
| `crates/vega_ui/Cargo.toml` | 空 feature `context-test-support`；dev-dependencies 中 `gpui_kit` 的 `test-support` |

其他 crate 无本卡新增 dev-dependency 清理项；不改根 `Cargo.toml` 生产 GPUI 配置。

`Cargo.lock` 是混合文件。冻结静态依赖入边给出的候选新增包身份为：`convert_case 0.11.0`、`proptest 1.11.0`、`proptest-macro 0.5.0`、`quick-error 1.2.3`、`rand_xorshift 0.4.0`、`rusty-fork 0.3.1`、`unarray 0.1.4`、`wait-timeout 0.2.1`。它们由 `gpui-pre` 新增 `proptest` 边及其依赖链带入；`gpui-pre` 另新增指向既存 `backtrace` 的边。

**这 8 个身份只是候选闭包，不是已验证删除结果。** 主会话在移除 feature 后确认最终入边，限定移除独占包/边；保留共享包版本、source、checksum，不全局删除 `backtrace`、`tempfile` 或随机数依赖。既存 `convert_case 0.10.0`、`quick-error 2.0.1` 的标签消歧变化不是新增生产依赖。

### 不属于本卡清理的保护范围

冻结比较中 `.github/`、`.config/`、`scripts/`、`vendor/`、`xtask/` 无任务差异，不借此更改通用平台。尤其保留：

- `.github/workflows/pr-check.yml`、`.github/workflows/cicd.yml`、`.github/workflows/release.yml`、`.config/nextest.toml`（含 `retries = 0`、`fail-fast = false`）。
- `crates/vega_ui/src/conversation_stream/bench.rs`、benchmark 模式及依赖；vendor manifest 既有 `doctest = false`。
- `scripts/release_tests.py`、`scripts/prepare_computer_use_tests.py`、`scripts/tests/test_delta_update.py`、`scripts/tests/test_harness_native_prompt_surface.py`、`scripts/tests/test_harness_pilot.py`，以及其他现存 Python 测试/fixture、打包发布能力、生产 migrations 和资源。

## 5. 现有 Actions 与实际证据缺口

`.github/workflows/pr-check.yml` 的命令如下，**仅用于记录现有云端流程，不是本地执行指令**：

| 阶段 | 现有命令/规则 |
|---|---|
| 格式 | `cargo fmt --all -- --check` |
| Clippy | `cargo clippy --workspace --all-targets -- -D warnings` |
| Python | `python3 scripts/release_tests.py`；`python3 scripts/prepare_computer_use_tests.py`；`python3 -m unittest discover -s scripts/tests -v` |
| 发现 | `cargo nextest list --workspace --message-format json`，写 `$RUNNER_TEMP/nextest-test-list.json`，读取顶层 `test-count` |
| 非空执行 | `cargo nextest run --workspace` |
| 空集执行 | `cargo nextest run --workspace --no-tests=pass`；成功后才写 `Rust: no Rust tests (0 discovered, 0 executed).` |
| 必需汇总 | `check (fmt, clippy, test)`，依赖 Clippy 和 Nextest 均成功 |

非空 discovery 包括 ignored/skipped，不等于实际执行；执行/通过/失败/忽略/跳过须读原始输出。Nextest 不覆盖 doctest，最终仍需静态确认无可执行 doctest。发现 JSON 当前仅在 runner 临时目录，未上传或完整打印；PR workflow 没有原始测试源码/patch 归档步骤。release artifact 不是开发测试恢复存档。缺证据由主会话留痕解决，不新增通用脚本或修改 workflow。

### 已知云端快照（本阶段未查询）

- 历史开发 head `2f141493bdd9e4cc9bbb82ea54d7fdbee0a88e86`：[run 40876047594](https://github.com/puzige/vega/actions/runs/40876047594)，Clippy `123455002791` success，Nextest `123455003186` 和 required `123457583978` failure。原始汇总 **191 执行、188 通过、3 失败、0 跳过**；独立发现数、忽略数未取得。
- 三项 `automation_entry_*` 在后台启动前因非法 fixture 线程 ID 返回 `CorruptRow(...)`，未达到封存断言。d404 修复该 fixture 为合法 ULID，未放宽断言；冻结 `attribution_entry_tests.rs` SHA-256：`fa6ef7bdbec41855e4aeafeb2fdc2f0744cf02ef3bd6599aaf3eb3a5b37b0753`。
- 主会话已读取 d404 [run 40892550296](https://github.com/puzige/vega/actions/runs/40892550296) 终态及原始失败日志：Clippy `123512939446` success，Nextest `123512939243` 与 required `123518160679` failure，Python successful。原始汇总 **195 执行、192 通过、3 失败、0 跳过（28.013s）**；独立发现清点与忽略数本次未取得。四项 delegated-history／checkpoint／stage 新增回归实际 PASS。三项 automation_entry 现为 `RunPreparation / Io / io_kind Other`，已越过旧 ULID 错误，但仍未达到后台封存断言；准备链路的责任层正在诊断，不能删测或冒充整轮通过，也不覆盖此后新增组合回归。

## 6. 主会话最小安全交付顺序（未完成项）

1. **固定完整开发版本并闭环失败。** 集成两条组合回归，记录完整 SHA/tree 和相对本冻结清单的新增/变化路径；对该精确版本运行现有完整 Actions，取得三项入口修复、四项历史委派结果增量及新组合回归的实际结果。失败先保留原日志并修责任层；不放宽断言、ignore、删除未决失败或重跑旧 head 换绿。
2. **删前固定可恢复原始证据。** 在 PR/持久证据历史保存原始测试源码链接及完整恢复 patch 链接，记录源提交 SHA/tree、patch 应用基准 SHA/tree、每个归档/patch 的 SHA-256 内容标识。范围须包含全套测试、混合文件内测试、helper、manifest/lock 接线及后续新增回归；核实下载与恢复可行，清理/squash/任务分支删除后仍可访问。不只依赖本机文件、过期 artifact 或未经可达性核实的 SHA。`claude-2.1.292-*` 是参考取证，不是 Vega 测试归档。**原始源码/patch 可恢复链接与归档身份：待主会话填写。**
3. **固定实际开发验证账。** 保存实际命令、开发完整 SHA/tree、run URL、各 job ID/结论、发现/执行/通过/失败/忽略/跳过数及原始输出/summary 的可访问位置。当前 d404 run 已固定失败终态和四项新增历史取回 PASS，新入口失败须闭环，新组合回归尚待完整版本 CI；历史 188-pass 与静态 Clear 不能补齐。**发现 JSON 的可恢复证据位置：未取得。**
4. **之后才做限定清理。** 基于本清单加已固定增量，删除 Rust 测试、可执行 doctest 及独占支持；混合文件只动测试部分，锁文件只删独占闭包。保护第 3/4 节生产、Python、benchmark、打包和通用 CI，禁止改名/移目录/复制源码留在最终树。
5. **清理后固定最终版本并静态清点。** 记录完整最终 SHA/tree、清理 diff、无测试/独占接线残留的清点和保护范围核对。**最终版本、最终静态零残留结果：未取得。** 不能把本冻结清单当作未来最终 head 的验证。
6. **对最终精确 head 跑完整 required CI。** 取得 fmt、全目标 Clippy、Python、Nextest 和 required 汇总全部成功，明确 **0 discovered / 0 executed**、`no Rust tests`；记录实际 run/job/命令/结果，不缩小 discovery 或跳过 job。**最终 0/0 与完整 required CI：未取得。** 空集门禁与开发功能验证分账，不消解旧失败。
7. **完成最终版本独审与用户验收边界。** 仍须完整最终版本独立审查；CI/合并不代表用户真实模型/系统手测通过。

本文件仅持久化历史静态清单、当前规格待补范围及已核实云端结果。写入阶段不执行 Vega、编译、测试、Clippy，不查询网络/CI，不变更 Git；只对本 Markdown 做读回、差异/空白及 SHA-256 静态核对。主规格及并行测试产物不在本阶段写入范围。

# ✦ Vega — S7 验收报告（Token 经济：定价、校准、流式计数与任务汇总）

**版本** v1.0 · 2026-08-31 · 状态词表遵循 [vega-s8-sdd.md](vega-s8-sdd.md) §1（七状态词，不可互换）

## 1. 证据冻结与交付台账

- **证据时间窗**：2026-08-31T09:54Z – 10:17Z（本地 17:54–18:17 +08:00）
- **分支 / worktree**：`feat/s7-t41-acceptance-report`（worktree `vega-s7-t41`）
- **门禁实测 HEAD**：`36a8f5d25d0de27118215a3024c7d2aba527a930`（test/clippy/build/bench/tree/scan 均在该树上实跑）
- **本卡 PR**：**PENDING**（分支 commit `36a8f5d`；squash hash 合并后由 T43 勘误回填）
- **raw 日志**：仅留本机 `/tmp/vega-s7-t41-{test,clippy,build,bench}.log`（SHA-256 前 16 位：`4dd927c8ab9807bb` / `45e72f89e7398cad` / `87bdd85faa0bd06c` / `ed8fa62b8c28be64…`）

S7 交付台账（`gh pr list --repo puzige/vega --state merged` 实时核对，非快照）：

| PR | squash hash | 卡 | 标题 | merged (UTC) |
|---|---|---|---|---|
| #35 | `dd67071` | S7 SDD | feat(A10-03): define Sprint 7 token economy | 08-30T16:51:24Z |
| #36 | `54589e8` | T36 | feat(A10-03): add integer pricing engine | 08-30T17:32:18Z |
| #40 | `298732e` | T37 | feat(A1-12): S7-T37 定价设置 + 持久化 | 08-31T01:25:08Z |
| #41 | `b96fcef` | T38 | feat(A10-01): persist priced provider usage | 08-31T05:14:38Z |
| #43 | `429cb2d` | T39 | feat(A10-05): show calibrated live token costs | 08-31T08:07:42Z |
| #44 | `696f1a3` | T40 | feat(A10-06): add task cost summaries | 08-31T09:10:22Z |
| 本卡 | **PENDING** | T41 | docs(S7): close sprint 7 with acceptance report | — |

（#45/de1ebf9 为 S8-T42 契约冻结，#42 为 S8 任务拆卡，均不计入 S7。）

## 2. T41 收口 E2E（`engineering fixture passed`）

`crates/vega_conversation/tests/s7_acceptance_e2e.rs`（603 行，1 例）：
`two_call_tool_journey_matches_synthetic_invoice_with_zero_error` — 全程真实生产链路
`run_thread_task_with_pricing`（与 `main.rs:1984` 生产调用点同入口）→ runtime/provider 事件 →
meter/summary 投影 → durable `token_usage`/`tool_calls`/`messages`，目录来自 owned temp data root
的真实严格 `pricing_v1` 文件：

1. 两轮 provider call + 一次工具轮：两个独立 frozen call-start / profile，exact usage/cost 落库
   （round-1 = 150 µ¢，round-2 = 205 µ¢，总计 355 µ¢）。
2. Composer 计数：CJK/emoji 估算 provisional → round-1 usage 原位校准 → round-2 usage 后
   provisional 清零，全程显示值逐帧断言。
3. 任务 summary 六项完整（token 四项、cost、duration 1,700ms、tool count 1、cache hit 38% half-up）。
4. **模拟账单零误差**：手写 oracle 355 µ¢，DB aggregate `abs_error = 0`，percent error = 0。
5. run 结束后修改 catalog（加倍价格）**不重定价**已存行；restart 后 seed/summary 与 run 前逐位一致
   （`425 tok · US$0.000355`）。
6. schema 恰好六表 + `PRAGMA user_version = 3`（仅 0001–0003 授权迁移）。

文件头注明：**synthetic invoice ≠ 真实 provider 账单证据**（MockProvider，零 key/网络/费用）。

## 3. 四门禁、测试总数与 bench（本机实跑）

| gate | time (local +08:00) | result |
|---|---|---|
| `cargo fmt --all -- --check` | 17:54 | PASS, exit 0 |
| `cargo clippy --all-targets -- -D warnings` | 18:03:53 | PASS, exit 0（39.3s） |
| `cargo test --workspace` | 18:16:54 | PASS, exit 0，**748 passed / 0 failed / 1 ignored** |
| `cargo build --workspace` | 18:04:26 | PASS, exit 0 |
| `cargo xtask bench`（release） | 18:06:53 | PASS, exit 0 |
| `cargo tree` | — | PASS, exit 0（无版本冲突/环） |
| `git diff --check` | — | PASS, 空输出 |

`block v0.1.6` future-incompatibility 为上游提示，非 Clippy 失败项。

计数只由最终 workspace log 的每条 `test result:` 机器求和（S7 相对 S6 报告 **687 → 748**）：

| suite | passed | ignored |
|---|---:|---:|
| `vega` controller（main.rs） | 37 | 0 |
| `vega_conversation` unit | 252 | 0 |
| conversation integrations（s5/s6/**s7_acceptance_e2e=1**/stream_estimate=14/task_cost_summary=7/todo/usage_pricing） | 27 | 0 |
| `vega_runtime` | 89 | 0 |
| `vega_store` | 75 | 1（真实 Keychain roundtrip，手动） |
| `vega_token` | 25 | 0 |
| `vega_markdown` / `vega_tools` / `vega_ui` / `vega_theme` | 32 / 90 / 110 / 6 | 0 |
| doctests | 5 | 0 |
| **合计** | **748** | **1** |

bench 原始值（60Hz dev 机；S6 括号对比）：

```text
cold_start     p50=2014ms p99=2017ms      spawn-to-exit placeholder（S6: 2025/2029ms）
memory_idle    111.2 MB                   RSS after 5s（S6: 108.7 MB）
render_frame   fps=60                     60Hz vsync cap，probe-binary 模式
stream_phase   fps=61 build p50=42µs p99=218µs ~500δ/s; frozen_remat=0（S6: 88µs/285µs）
```

P1–P8 判定主体在 T43/T48（SDD §1）；本表仅为 dev 机原始值，原样记录不漂白。

## 4. T36–T40 逐卡 DoD 对照

| 卡 | 卡面验收要点 | 证据（引用既有文件，不重复展开） | 结论 |
|---|---|---|---|
| T36 | 三系列内置目录；missing seed/reopen；exact override；malformed/overflow 矩阵；half-up；atomic save byte-identical；crate 无 GPUI/SQLite/network | PR #36；本报告 §3 复跑 `vega_token` 25/25 与 `cargo tree -p vega_token` 零 `gpui\|rusqlite\|reqwest\|keyring\|tiktoken`；§7 价格来源 | `engineering fixture passed` |
| T37 | owned temp data-root 生命周期 E2E（seed→CRUD→restart→malformed Invalid byte-preserve→repair+Reload）；Settings/app journey；byte/semantic、keyboard/960×600/theme/CJK 窄测 | [vega-s7-t37-e2e.md](vega-s7-t37-e2e.md)（E2E-REAL：`owned_data_root_crud_restart_and_explicit_recovery_e2e`）；本报告 §3 复跑 conversation 27 例全绿 | `engineering fixture passed` |
| T38 | MockProvider 两 call + tool；两独立 call-start/profile；exact usage/cost/event/两行 DB/restart aggregate；frozen selection 不受 mid-run authority 影响；retry 不改 timestamp；删线程后 usage 仍可聚合 | PR #41 + [vega-s7-t38-p2-carryforward 核销 §7](#7-carryforward-核销专章)；`usage_pricing_e2e` 1/1（frozen call-start 逐行断言）；T41 E2E 增补 run 后 catalog 变更不重定价 | `engineering fixture passed` |
| T39 | ASCII/CJK/emoji/空 delta/exact cap；estimate→calibrate 不双算；route switch/reopen/late event；`≈`/`—`/US$ 语义；1000δ/s 无 IO | [vega-s7-t39-e2e.md](vega-s7-t39-e2e.md)（14 例 E2E + 2 例 GPUI）；本报告 §3 复跑 stream_estimate 14/14 | `engineering fixture passed` |
| T40 | 0/1/N calls、0/N tools、cache ratio、无 usage、interrupted/error、restart 持久与 duration 降级、删线程保留审计；卡片 token/Light-Dark/CJK/键盘 | [vega-s7-t40-e2e.md](vega-s7-t40-e2e.md)（6/6 `summary::`）；本报告 §3 复跑 task_cost_summary 7/7；T41 E2E 六项 summary + restart | `engineering fixture passed` |

## 5. ui-spec §6 检查矩阵（SDD §1 状态词汇）

| 检查项 | 自动化证据 | 状态（未自动化项） |
|---|---|---|
| token | 色值 rg 六位 scan **0 命中**（§6）；counter/summary 全走 theme/Typography token（T39/T40 GPUI tests） | `engineering fixture passed`；真实字体/金额对齐观感 `human pending` |
| Light/Dark | 双 theme render/state tests（`vega_theme` 6 + `vega_ui` theme suites） | `engineering fixture passed`；真实切换无闪烁 `human pending` |
| CJK | CJK/emoji 估算与布局不 panic（stream_estimate/meter/markdown tests + T41 E2E 中文轮） | `engineering fixture passed`；fallback/豆腐块真实窗口 `human pending` |
| keyboard | Settings custom pricing + Composer/summary 可达（T37/T40 GPUI keyboard tests；T41 E2E 会话导航不受阻） | `engineering fixture passed`；完整真实窗口链路 `human pending` |
| 960×600 | layout constraints/compact formatter tests | `engineering fixture passed`；像素截图 `human pending` |
| P1 120fps | bench fps=60（60Hz vsync cap，frame-build margin p50=42µs） | 120Hz 实测 `hardware pending` |
| P2 `<16ms` / P5 `<100ms` | 有界 channel/first-wins owner tests；build p99=218µs | received→paint 真机分布 `hardware pending` |
| P3 frozen zero-remat | `frozen_remat=0`（bench probe） | `engineering fixture passed` |
| P7 first frame `<50ms` | 2014ms 为 spawn-to-exit placeholder，明确不冒充首帧 | 插桩后实测 `human pending`（S8） |
| P8 idle `<100MB` | 实测 111.2 MB，未达目标，原样记录 | `performance gate failed`（T43/T48 判定主体；S8 调优） |
| competitor | 无自动化替代 | Codex/ZCode 并排截图未做 `human pending` |

## 6. 红线对照（scan 逐条分类，非"零输出即完成"）

| redline | 结果与分类 |
|---|---|
| 无 tiktoken | `rg tiktoken Cargo.lock` **0 命中**。`rg 'tiktoken\|API[_ -]?KEY\|sk-…' crates docs` 命中全部为：① `vega_token/src/tests.rs` 的 `sk-secret` **redaction 测试 fixture**（断言 NOT rendered）；② `vega_ui/…/bench.rs:6` "ta**sk-c**ard" 的 `sk-` 误报；③ 文档对该偏离本身的记载（features/s8-sdd/t39/t40 e2e）。零真实 key、零网络调用 |
| 无越界 DDL | migrations 恰好 `0001/0002/0003`；0003 = C5 唯一授权 `ALTER TABLE token_usage` 三列追加；0001 恰好六 `CREATE TABLE`；T41 E2E 断言 `user_version = 3` |
| 无 test-only 生产 seam | T41 E2E 走生产入口 `run_thread_task_with_pricing`（`main.rs:1984` 同源）；T39 `run_approved_plan_task_with_pricing`（`main.rs:2000`）经审阅确认为真实生产 wiring；`RejectPermissionHook` 为真实生产默认 hook（T38 P2-4） |
| 非测试 unwrap/expect 零新增 | 全仓 scan 2674 hits；对 5 个 S7 squash commit（54589e8/298732e/b96fcef/429cb2d/696f1a3）逐 commit diff 审阅：**新增 140 行全部位于 `#[cfg(test)]` 模块内，生产代码 0 新增**（与 T36-T40 四次 MERGE_OK 审阅一致） |
| 依赖方向 | `cargo tree -p vega_ui`：`vega_runtime` 仅经 `vega_conversation` 间接可达（无直接依赖边）；`vega_token`/`vega_runtime` 树内 `gpui` 零命中；`vega_ui` 源码 `rusqlite`/`pricing.json` 零命中（rusqlite 仅经既有 `vega_store` 传递，UI 零直接 SQLite/定价文件 IO） |
| 色值/字号 token | `rg '#[0-9a-fA-F]{6}' crates/vega_ui` **0 命中** |

## 7. carryforward 核销专章

输入：T38 五项、T39 三项、T40 三项、T42 P2-1/P2-3、F2。逐项结论（resolved / carried-to-S8 / carried-to-T43 / documented）：

| # | 项 | 结论 | 依据 |
|---|---|---|---|
| T38 P2-1 | mid-run authority / retry timestamp 两条 E2E 断言未显式测试 | **documented** | 结构保证（catalog 被 run 按值拥有、无换绑 API；timestamp 每 logical call 冻结）；`usage_pricing_e2e` 逐行断言 frozen `call_started_at`；T41 E2E 增补「run 后 catalog 变更不重定价」；显式 mid-run 变更 E2E 未新增（结构上无换绑入口），无生产缺陷 |
| T38 P2-2 | `usage_pricing_e2e.rs:169` 注释失实（称有 retry 单测） | **resolved** | 本卡已改写为 by-construction 表述（并入本分支 commit 1 `36a8f5d`，仅测试注释） |
| T38 P2-3 | `unix_utc_seconds()` `unwrap_or_default` 回退 0 | **documented** | accepted：实际不可达、不违反 unwrap 红线；typed error 改造随 S8+ |
| T38 P2-4 | pub 化最小签名扩张 | **documented** | accepted：`RejectPermissionHook` 即真实生产默认 hook，T41 E2E 亦复用 |
| T38 P2-5 | `main.rs` 生产调用点未接计价 | **resolved** | T39 app wiring 落地：`main.rs:1984/2000` 已接 `run_thread_task_with_pricing`/`run_approved_plan_task_with_pricing`；T41 E2E 以同源生产入口全链路验证 |
| T39 P2-1 | `types.rs:764` compact_tokens `1000.0k` | **carried-to-S8** | 与 T40 P2-1 同根（k/M 边界），S8 一并修 |
| T39 P2-2 | meter fence 无条件放行 ToolCall* | **documented** | accepted：meter 镜像 stream 接受规则，in_run 门足够 |
| T39 P2-3 | 重复 MessageStarted meter desync | **documented** | accepted：下一 fenced terminal 事件自愈，校准数据不受污染 |
| T40 P2-1 | `summary_card.rs:151` `1000.0k` | **carried-to-S8** | 同 T39 P2-1 同根一并修 |
| T40 P2-2 | t40-e2e 测试过滤写法欠精确 | **documented** | accepted：数字无误（`summary::` 才是 6/6），证据文件为冻结记录不改写 |
| T40 P2-3 | duration 含有界 poll 延迟 | **documented** | accepted：已登记偏离，重启恢复落点 thread-open 投影 |
| T42 P2-1 | ui-spec 头部版本行 v0.3 vs changelog v0.4 | **carried-to-T43** | 一行勘误，归 S8 T43 文档勘误卡 |
| T42 P2-2 | PR #45 body commit subject 失实 | **resolved** | 合并前已更正（审阅记录） |
| T42 P2-3 | SDD §11.1 缺「T41 报告后同步勘误」标注 | **carried-to-T43** | §12 已覆盖，仅标注位置不齐，归 T43 勘误 |
| T42 P2-4 | SDD 变更记录未逐版递增 | **documented** | accepted：squash 后无影响 |
| F2 | `branch_controller_close_during_preflight…` 全量并发偶发 leaked-handles | **documented（owner S8）** | deflake 待安排（建议随 S8 某卡或独立小 PR）；本会话两次全量 workspace run 均 0 failed；失败即重试一次的协议见 `/tmp/vega-flaky-tests.md` |

## 8. 官方价格来源与核验日期

- 内置价目：`vega_token/src/catalog.rs` `BUILTIN_JSON` 五个 model（`gpt-5.6-terra`、`gpt-5.6-luna`、`deepseek-v4-flash`/`-pro`（base+peak UTC 周档）、`claude-sonnet-5`），doc comment 标注 "five source-verified built-in entries"。
- 核验日期：**2026-08-31 snapshot**（PR #36 描述原文 "official prices are a 2026-08-31 snapshot"）。
- 仓库内**未逐条留存官方 URL 台账**（S7 SDD C1 要求实现卡记录，T36 以 snapshot 声明交付）；per-price URL 台账属文档残项，S8 T43 文档勘误卡补录。价格属时点数据，SDD 不伪造数值。

## 9. 真实账单 `<5%` KPI — `real provider/billing pending`

⚠️ **human dogfood pending**：§2 的 mock 误差 0 是 `engineering fixture passed`，**不得冒充**真实账单 KPI。零真实 key/请求/费用由 executor 保持；以下为不含 key 的人工操作步骤：

1. 系统钥匙串配置真实 provider key（不写入仓库/配置文件/环境变量存档）。
2. 确认所用 model 在 Settings 定价列表（内置或 custom）且价格与 provider 当期账单同源。
3. 运行真实任务若干轮（含工具轮）；结束后面板读取 task summary cost，或导出 `token_usage` aggregate。
4. 于 provider 控制台导出**同一 UTC 时间窗**的 usage/billing 记录（model/currency 匹配）。
5. 计算 `abs(vega − invoice) / invoice × 100`；**<5% 且 nonzero billed cost** 方为达成（SDD §13 定义，最终判定 T50）。

记录模板（每行一次 dogfood）：

| 日期 | provider/model | UTC 窗口 | invoice (USD) | vega cost (USD) | error % | 备注 |
|---|---|---|---|---|---|---|
| | | | | | | |

## 10. 已知偏离与后置（任务卡五项原样进入）

1. `tiktoken-rs` 因白名单红线未引入；流式 v1 是字符近似，只有 API usage 是权威值。
2. ui-spec §4.4 的 `¥` 样例改为 `US$`（Microcents 与内置官方价格均按 USD；Phase 1 不做 FX）。
3. 真实账单 `<5%`、真实 provider、key 与 dogfood 属人类活动；executor 只提供 mock E2E 与操作模板（§9）。
4. A10-07 dashboard、A10-08 预算、A10-09 跨模型对比、A10-10 优化、A10-11 闲时联动、A10-12 导出均不在 S7。
5. T37 从合并 S6 T35 的 `90e5e35` 基线开工；T37-T41 只按当前真实 Settings/Composer/diff/route seam 接入，不反向修改已冻结 S6 契约。

S7 过程中新登记（已在 §7 核销）：四字段 token 显示把 cache_read 计入总量的 display-scope 观感（成本不受影响，T41 E2E 固定现值，S8 决策）、compact_tokens k/M 边界（S8）、F2 flaky（S8 deflake）、per-price URL 台账（T43）。

## 11. 结论

S7 六卡（T36–T41）全部交付：T36-T40 squash merge、T41 E2E `engineering fixture passed`（模拟账单零误差 + restart 一致 + schema 冻结）、四门禁 748/0/1 全绿、红线全过、carryforward 16 项逐条核销（resolved 3、carried-to-S8 2、carried-to-T43 2、documented 9）。S8 开工条件（T42 契约冻结 + 本报告）就绪；真实账单/硬件/首帧项按 §5/§9 保持 pending 状态词，不漂白。

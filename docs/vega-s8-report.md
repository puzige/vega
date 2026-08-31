# ✦ Vega — S8 验收报告（打磨 & 里程碑 · 确定性 Phase 1 验收）

**版本** v1.0 · 2026-08-31 · 状态词表遵循 [vega-s8-sdd.md](vega-s8-sdd.md) §1（七状态词，不可互换）
**本卡**：S8-T49（docs-only + README；本报告自身 squash 标 **PENDING**）
**主人决策框架**：本 Sprint 执行中，主人于 2026-08-31 作出三项长期决策（性能统一推迟 / 测试从简 / 1000 行重构并入 S8），T44/T48 两张性能卡未逐卡执行，性能 gate 全部按 T43 冻结基线如实呈现并标注 **deferred-to-final-optimization**。详见 §9 偏离专章——本报告不冒充按原卡纪律执行。

## 1. 证据冻结与交付台账

- **证据时间窗**：2026-08-31T22:38Z – 22:46Z（本地 09-01 06:38–06:46 +08:00）
- **分支 / worktree**：`feat/s8-t49-acceptance-report`（worktree `vega-s8-t49`）
- **门禁实测 HEAD**：`2f7a85392ade6263d1034bc1cc67065d7d3050a4`（fmt/clippy/test/build/tree/scan 均在该树上实跑；本报告只在其上追加 docs）
- **本卡 PR**：**PENDING**（squash hash 合并后回填；禁止引用未来 hash）
- **raw 日志**：本机 `/tmp/vega-s8-t49-{test,fmt-clippy,test-build,tree}.log`（repo-external，不入库）
- **性能证据**：全部引用 [vega-s8-t43-baseline.md](vega-s8-t43-baseline.md) 冻结数字（cutoff UTC `2026-08-31T12:35:36Z`，原始 JSON SHA-256 `459ebb2dee38a38c7b8178e41bd2d8e2e78378487cd843f9e7fc6561905fce11`）——本卡**不重测、不伪造**性能数字。

S8 交付台账（`gh pr list --repo puzige/vega --state merged` 实时核对，非快照；S8 范围 #42/#45/#47/#48/#49/#50/#51）：

| PR | squash hash | 卡 | 标题 | merged (UTC) |
|---|---|---|---|---|
| #42 | `cc38a53` | 拆卡 | docs(S8): add sprint 8 task breakdown | 08-31T06:44:46Z |
| #45 | `de1ebf9` | T42 | docs(S8-T42): freeze sprint 8 acceptance contracts | 08-31T09:10:34Z |
| #47 | `852e52d` | T46 | feat(A2-17): add recoverable task interruption | 08-31T11:25:24Z |
| #48 | `2a1019f` | T45 | feat(A11-03): add message pagination and hydration | 08-31T13:09:08Z |
| #49 | `ed1cab1` | T43 | feat(A2-04): replace probe instrumentation with truthful baselines | 08-31T13:35:04Z |
| #50 | `8978dab` | T47 | feat(A2-12): close phase 1 p0 items | 08-31T17:22:00Z |
| #51 | `2f7a853` | T51 | refactor: enforce 1000-line limit across workspace | 08-31T22:26:41Z |
| 本卡 | **PENDING** | T49 | docs(S8): close sprint 8 with acceptance report | — |

（#46/ddfae57 为 S7-T41 报告，#35/#36/#40/#41/#43/#44 为 S7 六卡，均不计入 S8。）

**S8 合并序列事实**：原卡序 T42→T43→T44→T45→T46→T47→T48→T49 因主人决策 6/9 调整为 T42→(#46 S7 报告)→T46→T45→T43→T47→T51→T49；T44（虚拟化）与 T48（调优）**未派卡**，归期末统一优化批（§9 偏离 1）。

## 2. 门禁实测（本机实跑）

| gate | time (local +08:00) | result |
|---|---|---|
| `cargo fmt --all -- --check` | 06:38 | PASS, exit 0 |
| `cargo clippy --all-targets -- -D warnings` | 06:38–06:40 | PASS, exit 0（2m 00s） |
| `cargo test --workspace` | 06:38–06:41 | PASS, exit 0，**823 passed / 0 failed / 1 ignored** |
| `cargo build --workspace` | 06:41 | PASS, exit 0 |
| `cargo tree` | 06:41 | PASS, exit 0（无版本冲突/环） |
| `git diff --check` | — | PASS, 空输出 |

`block v0.1.6` future-incompatibility 为上游提示，非 Clippy/build 失败项。

计数只由本机 log 的每条 `test result:` 机器求和（S7 报告 748 → S8 **823**，+75 为 S8 新增的 pagination 10 / stop-resume 10 / P0 与 file-selector / T47 主干 E2E / T43-T51 测试模块拆分后用例）：

| suite | passed | ignored |
|---|---:|---:|
| `vega`（含 T47 主干 E2E） | 41 | 0 |
| `vega_conversation` unit | 252 | 0 |
| conversation integrations（pagination_hydration=10 / restart_repair=6 / s5=1 / s6=2 / **s7_acceptance_e2e=1** / stop_repair_resume=4 / stream_estimate=14 / task_cost_summary=7 / todo=1 / usage_pricing=1） | 47 | 0 |
| `vega_runtime` | 89 | 0 |
| `vega_store` | 85 | 1（真实 Keychain roundtrip，手动） |
| `vega_token` | 25 | 0 |
| `vega_markdown` / `vega_tools` / `vega_ui` / `vega_theme` | 32 / 93 / 118 / 6 | 0 |
| `xtask`（T43 契约/percentile/guard-band 窄测） | 30 | 0 |
| doctests | 5 | 0 |
| **合计** | **823** | **1** |

## 3. T42-T51 逐卡 DoD 对照

判定词取 SDD §1 七状态词；"deferred" 一律指 **deferred-to-final-optimization**（期末统一优化批，主人决策 6）。

| 卡 | 主题 | 合并 PR | DoD 结论 | deferred 标注 |
|---|---|---|---|---|
| T42 | S8 SDD 契约冻结（C1-C8、七状态词、P0 审计 49 项、零 DDL 裁决） | #45 → `de1ebf9` | **`engineering fixture passed`**：C1-C8 冻结、P1-P8 owner 映射、ui-spec §5 勘误回写 | — |
| T43 | 性能埋点真值化 + 冻结基线 | #49 → `ed1cab1` | **`engineering fixture passed`**（埋点重写 + 基线冻结）；P7/P8 数字本身 `performance gate failed`、P2 短跑通过、P1 `hardware pending`（§4） | P7/P8 调优归期末批；5 分钟 soak 终证同批 |
| T44 | 变高会话虚拟化（万行 P1） | **未执行** | **deferred**（主人决策 6）；顶层会话仍为 `uniform_list` + 24px 定高（S3 形态），残余 24px 全部为卡内 compact-subrow 或既有等高实现，无未归类漂入 | deferred-to-final-optimization |
| T45 | 会话历史分页与重启水合 | #48 → `2a1019f` | **`engineering fixture passed`**：keyset cursor 分页（200 硬上限、0/201 fail-closed）、单一读快照、每页 SQL 常数（trace 断言）、UI 零 SQLite、A→B→A 丢晚到页、恰好六表 | 10k 行最新页打开 <100ms 的 host-provenance 测量未做（无可复现主机测量环境，如实记录，归期末批） |
| T46 | Stop / 启动修复 / 显式 Resume E2E | #47 → `852e52d` | **`engineering fixture passed`**：100 例确定性延迟矩阵 p99=189.7ms（<1s KPI，测试侧 Instant 口径）、四取消域、first-wins、repair 先于投影、恰一条 continuation 零 replay、crash-after-effect 残差显式 | UI 层可见 Stop 键盘链 gate（原 T48 范围）随期末批 |
| T47 | Phase 1 P0 收口 | #50 → `8978dab` | **`engineering fixture passed`**：A2-12 `@file`（8 files/16 KiB each/48 KiB 上限、fail-closed）、A2-14 模型选择器（config seam 持久化）、Composer 1-8 行内滚；thinking 档位会话内生效不持久化（如实记录） | thinking 持久化 deferred（config 模板连带，期末批顺手项） |
| T48 | memory_idle / 渲染 / UI 调优 | **未执行** | **deferred**（主人决策 6）；P1-P8 收口 gate 与 ui-spec §6 全收口未做，基线数字原样呈现（§4/§6） | deferred-to-final-optimization（整卡） |
| T51 | 1000 行重构（决策 9 并入 S8，原 T51a/T51b 撤回后改写） | #51 → `2f7a853` | **`engineering fixture passed`**：workspace 零 .rs >1000（最大 996）、API facade 逐字冻结、测试断言原样只挪、零行为变更、零新依赖；F3 deflake（10/10 连跑通过） | 546 处 facade `#[allow(unused_imports)]` 收敛为 glob（期末批顺手项） |
| T49 | 本卡（确定性 Phase 1 验收 + report） | **PENDING** | `engineering fixture passed`（S8 收口）；非 `Phase 1 milestone passed`（仅 T50 可用） | — |

## 4. 性能基线表（P1-P8 · T43 冻结数字 · deferred-to-final-optimization）

**判定主体**：以下各项的收口 gate 原属 T48；因主人决策 6，T44/T48 未执行，判定主体改为**期末统一优化批**。数字全部引自 [vega-s8-t43-baseline.md](vega-s8-t43-baseline.md)（冻结，schema `vega-s8-t43.baseline.v1`，本卡不重测）：

| 准线 | 冻结数字 | 阈值 | 状态词 | 判定主体 |
|---|---|---|---|---|
| **P7** 首帧可交互 | p50=166,166µs · **p95=1,027,906µs** · p99=max=1,273,207µs（20 轮） | p95 < 50,000µs | **`performance gate failed`**（不漂白） | 期末统一优化批 |
| **P8** 空闲 release RSS | **p95=109,084,672 B**（=109.085 MB 候选 A 口径 / 104.031 MiB 候选 B 口径；**双口径均超阈值**，单位裁决维持 OPEN(OWNER: human)） | < 100,000,000 B | **`performance gate failed`**（不漂白） | 期末统一优化批 |
| **P2** 流式上屏 | p99=**14,406µs** · p50=6,822µs · 1,441 批 · queue_max=0 @1,000 δ/s 10s 短跑 | p99 < 16,000µs | 短跑通过（`engineering fixture passed`，**非终证**——5 分钟 soak 未跑） | 期末统一优化批（soak 终证） |
| **P1** 万行滚动 margin | frame_build p50=6.541µs / p99=25.833µs（预算 8,333µs）· frozen remat=0 · 实测 60.0 Hz | median ≥120fps + 任一秒窗 ≥100fps @≥120Hz | **`hardware pending`**（60Hz 主机只证 margin） | T50 ProMotion 实测；变高迁移（T44）归期末批 |
| **P3** 冻结几何/remat | frozen remat=0（S3 行模型 probe） | 冻结区 remat=0 | `engineering fixture passed`（现实现下） | 期末批随变高迁移复测 |
| **P4** 滚动锚定 | anchored prepend 像素补偿（`anchored_prepend_offset`）+ owner 测试 | 锚点 <1px | `engineering fixture passed`（现实现下）；变高语义复测归期末批 | 期末批 |
| **P5/P6** 交互 <100ms / 动效白名单 | 无自动化基准（T48 范围） | p95 <100ms / 仅 150ms/120ms 白名单动效 | `human pending`（无证据不写 ✅） | 期末批（T48 范围） |

历史 S6 数字（108.7MB、spawn_to_exit 冷启动等）全部 **noncomparable**（T43 §1 已核实测量失真），本报告不引用。

## 5. T49 确定性验收 E2E 现状（决策 7 口径）

原 T49 卡要求新写一条全链路 E2E fixture。**主人决策 7（测试从简，2026-08-31）**：以 E2E 为主、不追认覆盖矩阵、确定性 mock E2E 不再新写。Phase 1 全链路的确定性证据由**既有已合并 E2E 承担**，本卡如实引用而非重复建造：

| 链路环节 | 既有 E2E（已合并） | 覆盖内容 |
|---|---|---|
| 变高/流式会话 + 成本可见 | `s7_acceptance_e2e::two_call_tool_journey_matches_synthetic_invoice_with_zero_error` | 真实生产入口 `run_thread_task_with_pricing` 全链路 → durable usage/tool_calls/messages → **模拟账单零误差**（oracle 355µ¢，abs_error=0）→ restart 逐位一致 → 恰好六表 `user_version=3` |
| 分页/水合 | `pagination_hydration_e2e`（10 例） | 10k 行 50 页无 gap/无重复连续翻页、MockProvider run 水合 tools/costs/summary/redaction、重启 repair 后 interrupted 可见、每页 SQL 常数 |
| Stop/repair/Resume | `stop_repair_resume_e2e`（4 例）+ `restart_repair_e2e`（6 例） | 部分文本+权限等待+自有进程组的生产 Stop、恰一条 terminal durable、repair 先于水合、一次 Resume 恰一个新 run 零 replay、crash-after-effect 残差显式 |
| P0 主干 | `vega` tests 41 例（含 T47 `@file` 主干 E2E） | fresh thread → `@` 注入 → 持久化 → 重启保持 |

diff→commit 真实 Git 链路由 S6-T34 两段式 commit E2E 与 T51 `commit_proof` 测试族覆盖（零 `/usr/bin/open`、零 developer repo/global Git config，owner 证据见 s6-report）。**上表为 `engineering fixture passed` 的证据边界**：mock 账单误差 0 ≠ 真实账单（§10）；"Phase 1 milestone passed" 状态词本报告不得使用（SDD §1 机械判定）。

## 6. ui-spec §6 检查矩阵（SDD §1 状态词汇；T48 未执行，自动化项引各卡已有证据）

| 检查项 | 自动化证据 | 状态（未自动化项） |
|---|---|---|
| token 色扫描 | `rg '#[0-9a-fA-F]{6}' crates/vega_ui/src/` **0 命中**（本卡 HEAD 复跑）；卡片族全走 theme/Typography token（T39/T40/T47 GPUI tests） | `engineering fixture passed`；真实字体/金额对齐观感 `human pending` |
| Light/Dark | 双 theme render/state tests（`vega_theme` 6 + `vega_ui` theme suites，本卡复跑全绿） | `engineering fixture passed`；真实切换无闪烁 `human pending` |
| CJK | CJK/emoji 估算与布局不 panic（stream_estimate 14 例 / markdown 32 例 / T41 E2E 中文轮；本卡复跑全绿） | `engineering fixture passed`；fallback/豆腐块真实窗口 `human pending` |
| keyboard | file_selector 键盘（T47）、Composer/Stop/Resume GPUI 窄测（T46/T47）、permission 卡焦点链（S5）——本卡复跑全绿 | `engineering fixture passed`；完整真实窗口链路（建会话→发消息→批准→diff→提交不碰鼠标）`human pending` |
| 960×600 | layout constraints/compact formatter tests（本卡复跑全绿） | `engineering fixture passed`；像素截图 `human pending` |
| P1 120fps | 60Hz 主机 frame-build margin p50=6.541µs/p99=25.833µs（T43 §6） | 变高迁移 + 120Hz 实测：`hardware pending`（T50）+ `deferred-to-final-optimization`（T44） |
| P2 `<16ms` | p99=14,406µs 短跑（T43 §5，10s @1,000δ/s） | 短跑通过非终证；5 分钟 soak 终证 `deferred-to-final-optimization` |
| P3/P4/P5/P6 | frozen remat=0（probe）；anchored prepend owner tests（T45） | P3/P4 现实现 `engineering fixture passed`、变高复测归期末批；P5/P6 `human pending`（T48 未执行，无证据不写 ✅） |
| P7 `<50ms` | p95=1,027,906µs（T43 §3） | **`performance gate failed`** + `deferred-to-final-optimization` |
| P8 `<100MB` | p95=109,084,672 B 双口径超阈（T43 §4） | **`performance gate failed`** + `deferred-to-final-optimization`；单位裁决 `human pending`（SDD §10） |
| competitor 走查 | 无自动化替代 | Codex/ZCode 并排截图未做 `human pending` |

## 7. 红线对照（scan 逐条分类，非"零输出即完成"）

| redline | 结果与分类 |
|---|---|
| 无 tiktoken | `rg tiktoken Cargo.lock` **0 命中**（本卡 HEAD 复跑）。`rg tiktoken crates docs` 命中全部为文档对该白名单裁决本身的记载（phase1-plan/features/s7-tasks/s7-report 等）——`vega_token` 不含 tokenizer，流式为字符近似 + API usage 校准（主人决策 2，S7 偏离 #1） |
| migration 恰 3 | `crates/vega_store/migrations/` 恰 `0001/0002/0003`（本卡复列）；`rg 'CREATE TABLE\|ALTER TABLE' crates`（非 tests）12 条语句与 S7 基线逐条相同，零新增（T45 PR 已作 DDL 对照，本卡复核 migrations 目录清单）；恰好六表 |
| 无 test-only 生产 seam | T45 走 `vega_store::messages::page_before` + `restart_history_page` 生产查询；T46 测试-only（`vega/src/main.rs` 仅 `#[cfg(test)]` gpui fence 测试）；T47 主干 E2E 走生产 worker 提交链路；T43 只改 `xtask/`（工具 crate，非生产运行时） |
| 无真实 key/网络 | S8 全部卡零真实 key、零真实请求、零费用；T43 bench 临时 HOME + provider=none/network=none 子进程 attestation（baseline §2） |
| API 冻结（T51 facade） | T51 逐字节核验：types/agent/conversation_stream/sidebar/git_workspace re-export 路径不变、`actions!` 不动、visibility 扩张仅 `pub(crate)`；3 个抽检函数逐字节一致、197 条断言零漂移（T51 PR 记录） |
| 非测试 unwrap/expect | S8 新增代码仅 T43 xtask `probe.rs:136` 一处 `expect`（T43 P2-7 accepted：工具 crate、方向安全）；生产 crates 无新增 |
| 1000 行硬上限（决策 9） | `find crates xtask -name '*.rs'` **0 个文件 >1000 行**，最大 `vega_runtime/src/agent/tools_exec.rs` 996 行（本卡 HEAD 复跑；T51 自报 906 系精度 nit，如实更正） |
| 色值/字号 token | `rg '#[0-9a-fA-F]{6}' crates/vega_ui` **0 命中** |
| 零新外部依赖 | S8 七个 merged PR 均零新依赖；`cargo tree` 无版本冲突（§2）；T43 仅复用既有依赖边（xtask 内） |

## 8. carryforward 核销专章

输入：T38 五项（S7 已核销，本卡复认终态）、T39 三项、T40 三项、T41 两项、T42 三项、T43 五项（审阅清单 1-5）、T45 四项、T46 四项、T51 一项、F2/F3。逐项结论（resolved / carried-to-final-opt / documented）：

| # | 项 | 结论 | 依据 |
|---|---|---|---|
| T38 P2-1 | mid-run authority / retry timestamp E2E 断言未显式测试 | **documented**（S7 §7 已核销） | 结构保证（catalog 按值拥有、timestamp 每 call 冻结）；T41 E2E 增补「run 后 catalog 变更不重定价」 |
| T38 P2-2 | `usage_pricing_e2e.rs:169` 注释失实 | **resolved**（S7） | T41 分支 commit 改写为 by-construction 表述 |
| T38 P2-3 | `unix_utc_seconds()` `unwrap_or_default` 回退 | **documented**（S7） | accepted：不可达、typed error 改造随期末批 |
| T38 P2-4 | pub 化最小签名扩张 | **documented**（S7） | accepted：RejectPermissionHook 即真实生产默认 hook |
| T38 P2-5 | 生产调用点未接计价 | **resolved**（S7/T39） | `main.rs` 生产 wiring + T41 E2E 同源验证 |
| T39 P2-1 | `format_compact_tokens` 999,999→`1000.0k`（types/meter.rs） | **carried-to-final-opt** | 本卡核实现码仍 `<1_000_000` 判 k；修复涉 .rs，docs-only 不动 |
| T39 P2-2 | meter fence 无条件放行 ToolCall* | **documented**（S7） | accepted：meter 镜像 stream 接受规则，in_run 门足够 |
| T39 P2-3 | 重复 MessageStarted meter desync | **documented**（S7） | accepted：下一 fenced terminal 自愈，校准不受污染 |
| T40 P2-1 | `summary_card.rs` compact_tokens 同款 k/M 边界 | **carried-to-final-opt** | 与 T39 P2-1 同根，期末批一并修（本卡核实仍现症） |
| T40 P2-2 | t40-e2e 测试过滤写法欠精确 | **documented**（S7） | accepted：数字无误，证据文件为冻结记录不改写 |
| T40 P2-3 | duration 含有界 poll 延迟 | **documented**（S7） | accepted：已登记偏离，重启落点 thread-open 投影 |
| T41 P2-1 | s7-report §11 结转汇总计数错位（写 4/8，实际 3/9） | **carried-to-final-opt** | 报告为冻结证据不改写；随期末批 docs 勘误 |
| T41 P2-2 | s7-report "604 行"实为 603 | **carried-to-final-opt** | 精度 nit，随期末批 docs 勘误 |
| T42 P2-1 | ui-spec 头部版本行 v0.3 vs changelog v0.4 | **carried-to-final-opt** | 本卡核实 `docs/vega-ui-spec.md` 头部仍 v0.3；一行勘误归期末批 docs 勘误 |
| T42 P2-2 | PR #45 body commit subject 失实 | **resolved**（S7） | 合并前已更正 |
| T42 P2-3 | SDD §11.1 缺「T41 报告后同步勘误」标注 | **carried-to-final-opt** | §12 已覆盖，标注位置不齐，随期末批 docs 勘误 |
| T43 P2-1 | xtask c2_gate 灰区扩展 `rounds==C2_ROUNDS` 恒 false | **carried-to-final-opt** | 失效方向保守（宁漏扩展不误扩展）；期末批随 P8 gate 重启用 |
| T43 P2-2 | canonical JSON p2 块缺 run_completed/per_second 字段 | **carried-to-final-opt** | schema 勘误随期末批（T48 消费前） |
| T43 P2-3 | baseline doc "40 轮"实为 20 轮 | **carried-to-final-opt** | 文档笔误，随期末批 docs 勘误 |
| T43 P2-4 | report.rs 控制台 "performance gate passed" 不在七词表 | **carried-to-final-opt** | 仅 console 字符串（JSON 产物合规）；期末批统一改用词表 |
| T43 P2-5 | render probe 以真实 HOME spawn，未核查 profile 触碰 | **carried-to-final-opt** | 期末批核查项（probe 只读渲染、无写入证据；T48 消费前核查） |
| T45 P2-1 | prepend 未平移 demo `InjectionState.entry_index` | **carried-to-final-opt** | 本卡核实 `composer.rs` 注入 index 无 prepend 平移路径；demo 注入+在顶组合才触发，无数据损坏 |
| T45 P2-2 | 一致快照测试缺页读/批读交错写入用例 | **carried-to-final-opt** | 结构保证（deferred 读事务）成立、证明缺失；决策 7 下不追认矩阵，归期末批 |
| T45 P2-3 | CorruptRow 判定依赖 store 错误文案子串 | **documented** | accepted：fail-closed 不破；typed error chain 随期末批 |
| T45 P2-4 | 为测试扩只读 production API（hydrated_entry_count 等） | **documented** | accepted：只读观察口，无行为面 |
| T46 P2-1 | 分支基点滞后两点 diff 伪影 | **documented** | accepted：merge 无影响 |
| T46 P2-2 | 100 例矩阵计时口径注释（"Stop→terminal"实为 run-start→terminal，更保守） | **documented（本报告核销）** | **T49 勘误**：矩阵实测口径为 run-start→terminal（Stop 于权限边界等待时触发），比卡面口径更保守，p99=189.7ms KPI 仍成立；测试内注释已注明与 T43 口径不可比 |
| T46 P2-3 | `resume_refusal_on_stale_rows…` 测试名与行为不符（实为先修复再续跑） | **carried-to-final-opt** | 现名 `resume_refusal_on_stale_rows_happens_before_any_provider_round` 已可读但改名涉 .rs，归期末批 |
| T46 P2-4 | pending 行 audit 括注 | **documented（本报告核销）** | E2E 断言为解析级（terminal 状态/审计列）；字节级由 recovery owner 单测覆盖 |
| T51 P2-2 | 546 处 facade `#[allow(unused_imports)]` 待收敛 glob | **carried-to-final-opt** | 机械项，期末优化批顺手收敛 |
| F2 | `branch_controller_close_during_preflight…` GPUI leaked-handles 偶发 | **carried-to-final-opt** | 本卡 HEAD 全量跑未触发（823/0/1 一次通过）；deflake 归期末批；在此之前 pre-push 偶发失败 kill 重试一次（决策 8） |
| F3 | `trusted_mutation_runner…` 500ms spawn 竞态 | **resolved** | T51 commit `a165647` deflake：不确定超时重试至 5 次新 fixture；10/10 连续通过（T51 PR 记录） |

**汇总**：32 项——resolved 4（T38 P2-2、T38 P2-5、T42 P2-2、F3）、carried-to-final-opt 14、documented 14（其中 T38 五项与 T39/T40 大部分为 S7 已核销项的终态复认）。S7→S8 无一项被静默丢弃。

## 9. 偏离专章（主人决策 6/7/9 + F2/F3 历史）

本卡与原 T42-T49 卡面的全部差异，均源于主人 2026-08-31 三项长期决策（[/tmp/vega-project-decisions.md](/tmp/vega-project-decisions.md)，登记于项目层）——**如实记录，不冒充按原卡纪律执行**：

1. **决策 6（性能统一推迟）**：T44（变高虚拟化）与 T48（调优）未逐卡执行，无 PR。原 T49 要求的"性能 gate 收口"改为引用 T43 冻结基线：P7 `performance gate failed`（p95 1,027,906µs）、P8 `performance gate failed`（109,084,672 B 双口径超阈）、P2 短跑通过（非 soak 终证）、P1 `hardware pending`。各卡 DoD 表（§3）中所有 "deferred" 项的判定主体为**期末统一优化批**；T44/T48 卡面语义（C4 变高迁移、C2 gate、ui-spec §6 全收口）并入该批，不丢失。
2. **决策 7（测试从简）**：S8 各卡测试覆盖即实际合并状态，本卡不追认覆盖矩阵；确定性 mock E2E 不再新写（§5 引用既有四族 E2E 为 Phase 1 主干证据）；GPUI 窄测试仅保留既有最小集。红线不放松部分（fail-closed、无真实 key/网络、无未批准 DDL、非测试 unwrap）照常执行（§7）。
3. **决策 9（1000 行硬上限）**：T51 作为原卡面之外的增卡并入 S8（原 T51a/T51b agent 按主人指示撤回，标准改写后重开）；交付为**零行为变更**的机械重构 + F3 deflake。附带精度 nit：T51 自报最大文件 906 行，实际 996 行（本卡 §7 复核更正）。
4. **T47 卡内偏离**（PR #50 自报）：A2-14 thinking 档位会话内生效但不持久化（`Defaults` 无 thinking 字段，加字段超卡面）；`@file` 注入失败降级为原始消息不阻塞 run。
5. **F2/F3 flake 历史**：F1（S7-T38 修复）、F3（T51 修复，10/10）已结；F2（branch_controller leaked-handles）**未 deflake**，归期末批——本卡门禁一次通过未触发；此前协议（决策 8）：pre-push 偶发失败 kill 重试一次。`/tmp/vega-flaky-tests.md` 登记与本报告一致（F2 条目以本报告为最终状态）。
6. **合并序列**：#47（T46）先于 #48（T45）合并（T45 rebase 至组合树复跑全部门禁，PR 记录在案）——SDD 契约未因此漂移（T45 消费 C7、T46 消费 C5，互不引用实现）。

## 10. human / 硬件项清单（T50 输入）

以下全部 `human`/`hardware`/`real provider/billing pending`，**T49/T50-executor 均不执行**（不索取 key、不发真实请求、不产生费用）。另有一项人类裁决（非执行）：

| # | 项 | 状态词 | 操作模板（不含 key） |
|---|---|---|---|
| 0 | **P8 阈值单位裁决**（SDD §10 OPEN） | `human pending`（裁决，非执行） | 阅读 SDD §3.1 利弊表，答复候选 A（decimal MB，当前字面权威）或 B（100 MiB）；裁决后 docs 勘误 ui-spec §5 P8 行 + phase1-plan E4（如需），schema 常量随期末批落定，测后永不换 |
| 1 | ProMotion 120fps 实测 | `hardware pending` | ≥120Hz 真机 → `cargo xtask bench`（P1 场景）→ 断言 median ≥120fps 且任一秒窗 ≥100fps；记录 CPU/GPU/显示器 provenance |
| 2 | 真实账单 <5% | `real provider/billing pending` | ① App Settings 经 Keychain 配置真实 provider key（不入仓库/环境变量存档）；② 确认 model 在定价列表且价格与账单同源；③ 运行真实任务若干轮（含工具轮），读 task summary cost 或导出 `token_usage` aggregate；④ provider 控制台导出同一 UTC 窗口 usage；⑤ 计算 `abs(vega−invoice)/invoice×100 < 5%` 且 nonzero billed cost |
| 3 | 真实仓库任务（里程碑主体） | `real provider/billing pending` | 授权真实 repo → 发起改码任务 → diff 审阅 → 两段式 commit → 成本全程可见；记录 task/diff/commit hash/cost |
| 4 | 7 天 dogfood | `human pending` | 七个**独立 dated** 日逐日记录 task/result/failure/perf/UX；不足七天不得宣称完成（SDD C8） |
| 5 | 真实窗口走查（ui-spec §6 人工项） | `human pending` | Light/Dark 切换无闪烁、CJK/emoji 豆腐块检查、键盘全链路（建会话→发消息→批准→diff→提交不碰鼠标）、960×600 最小窗无破裂、动效质感（150ms/120ms 白名单）、Codex/ZCode 并排对比 |

记录模板（每行一次 dogfood / 账单对照）：

| 日期 | provider/model | UTC 窗口 | invoice (USD) | vega cost (USD) | error % | 备注 |
|---|---|---|---|---|---|---|
| | | | | | | |

全部四项真实证据齐备后，T50 方可将状态收口为 `Phase 1 milestone passed`（SDD §1 机械判定；在此之前任何报告/README 使用该词即违规）。

## 11. 结论

S8 收口为 **`engineering fixture passed`**（非 `Phase 1 milestone passed`）：

- **交付**：7 个 PR squash 合并（#42/#45/#47/#48/#49/#50/#51）——契约冻结（T42）、性能埋点真值化 + 冻结基线（T43）、分页水合（T45）、Stop/Resume E2E（T46）、P0 收口（T47）、1000 行重构 + F3 deflake（T51）、本报告（T49）。
- **门禁**：fmt/clippy/test/build/tree/diff-check 全绿，**823 passed / 0 failed / 1 ignored**（S7 748 → 823）。
- **性能如实**：P7/P8 `performance gate failed`（T43 冻结数字，不漂白），P2 短跑通过、P1 `hardware pending`——全部 `deferred-to-final-optimization`（主人决策 6），判定主体期末统一优化批；T44/T48 未执行如实记录。
- **红线全过**：无 tiktoken、migration 恰 3 / 零 DDL、零 test-only 生产 seam、零真实 key/网络、API facade 冻结、零文件 >1000 行、零新依赖。
- **carryforward 32 项逐条核销**（resolved 4 / carried-to-final-opt 14 / documented 14），无静默丢弃。
- **T50 输入就绪**：P8 单位裁决 + ProMotion/真实账单/真实仓库任务/7 天 dogfood/真实窗口走查六项清单与不含 key 操作模板（§10）；全部齐备前状态词保持 pending，不漂白。

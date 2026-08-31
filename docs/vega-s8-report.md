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

## 6. ui-spec §6 检查矩阵

（TBD-EXPAND）

## 7. 红线对照

（TBD-EXPAND）

## 8. carryforward 核销专章

（TBD-EXPAND）

## 9. 偏离专章（主人决策 6/7/9 + F2/F3）

（TBD-EXPAND）

## 10. human / 硬件项清单（T50 输入，不含 key 操作模板）

（TBD-EXPAND）

## 11. 结论

（TBD-EXPAND）

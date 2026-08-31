# ✦ Vega — S8 SDD：Phase 1 收口契约冻结（S8-T42）

**版本** v1.0 · 2026-08-31 · 状态：冻结（P8 单位一项 OPEN，见 §10）
**来源卡**：[vega-s8-tasks.md](vega-s8-tasks.md) T42（本文档为其唯一产出 SDD；卡面与本文冲突时以卡面为准并勘误本文）
**关联**：[vega-exec-guide.md](vega-exec-guide.md)（红线/证据分级）· [vega-phase1-plan.md](vega-phase1-plan.md)（S8 行与 DoD）· [vega-ui-spec.md](vega-ui-spec.md) §5/§6（P1-P8 准线）· [vega-features.md](vega-features.md)（P0 清单）· [vega-tech-risks.md](vega-tech-risks.md)（等高列表不能表达会话消息）

> **契约地位**：C1-C8 与状态词汇自本文合并起冻结；T43-T50 只消费，不得改判据自证。任何修订须走 docs 勘误并注明动机，测后禁换单位/轮次/阈值/percentile 数学。

## 0. 证据基线与 S7 收口状态

- 本文依据 `origin/master` `429cb2d`（T36-T39 已合并：#35 SDD、#36 T36 定价引擎、#40 T37 定价设置、#41 T38 usage 持久化、#43 T39 实时校准成本）。
- **T40（A10-06 汇总卡）实现进行中未合并、T41（S7 报告）未产出**：凡引用 S7 最终数字/状态处一律标注 **（T41 报告后同步勘误）**，本文不写死任何 S7 最终数字。
- 已在 `429cb2d` 核实的代码事实（作为契约动机，非数字基线）：`xtask` `cold_start` 现为 `spawn_to_exit`、`memory_idle` 单次 debug 采样且 `rss_mb` 实为 MiB、渲染输出硬编码 60Hz、流式 probe ~500 deltas/s；`crates/vega_ui/src/conversation_stream.rs` 顶层会话 `uniform_list` + `ROW_HEIGHT = 24.0` 且截断长内容。

## 1. 状态词汇（normative，不可互换）

冻结 vega-s8-tasks.md 文首状态词汇表：七个状态词各绑定唯一判定主体，禁止互相替代或漂白。（待扩：逐词定义与可用者）

## 2. C1 — P7 首帧可交互（`process_start_to_first_rendered_interactive`）

**一行结论（占位）**：20 个全新进程 nearest-rank p95 <50.000ms；语义为 GPUI next-frame 回调 flushed 恰一条严格 JSON milestone 后正常退出，next-frame ≠ 物理 present。（待扩）

## 3. C2 — P8 release RSS 与单位裁决

**一行结论（占位）**：阈值字面权威 <100,000,000 bytes（decimal MB）为默认，改 104,857,600（100 MiB）须人类批准——单位 **OPEN(OWNER: human)**，裁决后测后永不换；协议为 20 个全新 release 进程 +5/+10/+15s median 的 nearest-rank p95。（待扩）

## 4. C3 — Provenance 与隔离

**一行结论（占位）**：每个产物记录全量 provenance 字段并无条件重建 release 或解析 exact artifact；bench 持临时 HOME、预 seed 安全 profile、禁 Keychain/provider/network、写 /tmp、不弄脏 repo。（待扩）

## 5. C4 — 24px = compact-subrow 规则

**一行结论（占位）**：24px 仅为 diff 行/卡内行等真正等高 compact-subrow 规则；顶层会话一项=一个语义 item 自然高度，仅 mutable tail 或显式失效 item 可 rematerialize，禁截断凑高度。（待扩）

## 6. C5 — Stop / 启动修复 / 显式 Resume

**一行结论（占位）**：Stop 可见、键盘可达、first-wins、取消贯穿四个 ownership 域，durable 行达 terminal 且矩阵 p99 <1s；Resume 在旧行全 terminal 后追加 auditable continuation 开新 run，永不 replay mutating tool；crash-after-effect 残差显式记录为非 exactly-once。（待扩）

## 7. C6 — P1/P2 测量与显示边界

**一行结论（占位）**：P1 记录 build/layout/paint 分布对 8.33ms + 队列界 + 冻结 remat，字面 120fps 需实测 ≥120Hz 且 60Hz 只报 hardware pending；P2 终证为 5 分钟 release soak @1,000 deltas/s p99 <16.000ms，receive-to-render 不可被更早时间戳替代。（待扩）

## 8. C7 — 会话历史分页

**一行结论（占位）**：`messages.seq` typed headless cursor/page + batch 关联 `tool_calls` + 一致读快照 + 默认/硬上限 200；UI 零 SQLite 调用；六表内无第七表、无 N+1。（待扩）

## 9. C8 — 报告真值

**一行结论（占位）**：状态词汇按 §1；mock 账单误差 0 ≠ 真实账单；一周=七个独立 dated dogfood 日；报告有 evidence cutoff、列已合并 squash hash、自标 PENDING，禁未来 hash。（待扩）

## 10. OPEN(OWNER: human) 裁决项

- **P8 单位**：100,000,000 bytes（decimal MB）vs 104,857,600 bytes（100 MiB）——两候选利弊对照见 §3.2；裁决前按卡面字面权威 decimal MB 执行，裁决后写入 T43 schema，测后永不换。

## 11. P0 审计与 owner 映射

- 全量 P0（49 项，以 [vega-features.md](vega-features.md) HEAD 为准）逐项映射 owner 卡或已合并证据；无新增 deferral。
- P1-P8 唯一 owner：P7/P2/P8 埋点与测量→T43；P1 变高实现→T44；P1-P8 全项收口 gate→T48；汇总与报告→T49；human/hardware 证据→T50。
- 六表 migration 裁决：见 §11.3（保守裁决，零 DDL）。（待扩）

## 12. T41 报告后同步勘误清单

- 列出本文所有依赖 T40/T41 合并后事实的引用点（待扩）。

## 13. 验收自查（T42 验收条目对照）

- （待扩：八要素对照、内链可解析、依赖/六表扫描无变化。）

## 变更记录

- v1.0 (2026-08-31) 骨架：章节与每契约一行结论占位。

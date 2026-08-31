# ✦ Vega — S8 任务卡（Sprint 8 · 打磨 & 里程碑 · W15-16）

**版本** v0.1（骨架）· 2026-08-31 · 使用方式：每张任务卡 + [vega-exec-guide.md](vega-exec-guide.md) = 一条完整的执行 prompt

**S8 目标**（phase1-plan §2）：主题完善；中断/恢复；内存与渲染调优；dogfood。**里程碑**：自研 Runtime 在真实仓库完成任务（改码→diff→commit），成本全程可见；dogfood 一周。

**Sprint DoD（草案，T42 冻结）**：确定性 mock 任务完整证明 Phase 1 全链路（变高虚拟化滚动、流式 <16ms、冷启动首帧 <50ms、空闲 RSS <100MB、中断/恢复、diff→commit、成本全程可见），`docs/vega-s8-report.md` 如实记录；真实仓库任务、真实账单 <5%、ProMotion 120fps、7 天 dogfood 明确标 **human/hardware pending**，由 T50 人类收口，executor 不索取 key、不发真实请求、不产生费用。

> **实现基线**：S8 规划基于 `master` `b96fcef`（S7 T36/T37/T38 已合并 #35/#36/#40/#41）。T39/T40/T41 尚未合并——凡依赖 S7 最终 API/基线数字的卡，开工前必须**（S7 合并后复核冻结基线）**，不得从预检报告虚构 T39-T41 seam。

> **测量真值裁决**：现有 `xtask bench` 的 `cold_start=spawn_to_exit`（进程退出≠首帧可交互）、`memory_idle` 单次 debug 采样且 `rss_mb` 实为 MiB、渲染输出硬编码 60Hz、流式 ~500 deltas/s 均不可作为 P1/P2/P7/P8 终证。T43 先修语义与埋点并冻结基线；T48 只对冻结 schema 调优，**不得改判据自证**。P8 单位（100 MB=100,000,000 bytes vs 100 MiB=104,857,600 bytes）由 T42 人类裁决后冻结，测后永不换单位。

> **状态词汇（不可互换）**：`engineering fixture passed` / `performance gate failed` / `human pending` / `hardware pending` / `real provider/billing pending` / `dogfood in progress (day N/7)` / `Phase 1 milestone passed`（仅 T50 在全部真实证据齐备后可用）。

---

## 卡片总览

| # | 卡片 | 前置 | 一行范围 |
|---|---|---|---|
| T42 | S8 SDD 与 Phase 1 收口契约冻结 | S7 T41 合并 | docs-only：冻结 P7/P8/P1/P2 测量语义、24px 规则、Resume 语义、P0 审计与状态词汇 |
| T43 | 性能埋点真值化 + 冻结基线 | T42 | xtask 埋点重写：首帧可交互 P7、1k/s 流式 P2、release RSS P8 语义，产出可比基线 |
| T44 | 变高会话虚拟化（万行 P1） | T43 | 弃用顶层 `uniform_list`+`ROW_HEIGHT=24.0`，迁移变高语义项，10k 混合内容基准 |
| T45 | 会话历史分页与重启水合 | T44 | `messages.seq` cursor 分页 + typed 投影 + UI 水合，六表内不 N+1 |
| T46 | Stop / 启动修复 / 显式 Resume E2E | T45 | Composer Stop 全链路取消 + 重启 repair + 显式 Resume 新 run，p99 <1s 矩阵 |
| T47 | Phase 1 P0 收口 | T46 | provider/model/thinking 选择器、bounded `@file`、Composer >8 行内滚、全 P0 审计 |
| T48 | memory_idle / 渲染 / UI 调优 | T47 | 对 T43 冻结 schema 调优至 P8 <100MB + ui-spec 自动化项全收口 |
| T49 | 确定性 Phase 1 验收 + report 草稿 | T42-T48 | 一条全链路 E2E + `docs/vega-s8-report.md` + README 真值化 |
| T50 | 人类/硬件/dogfood 收口（HUMAN PENDING） | T49 合并 | ProMotion 实测、真实账单 <5%、真实仓库任务、7 天 dogfood——人类执行 |

**并行红线**：同一时刻只允许一张卡持有一个 sibling worktree/PR；conversation 列表、app controller、`xtask`、s8-report 不允许多卡并行编辑。

---

## T42 · S8 SDD 与 Phase 1 收口契约冻结（docs-only）

- **前置**：S7 T41 squash 合并；复核 T37-T41 真实 API（T37/T38 已合并可直查，T39/T40/T41 以合并后代码为准）。
- **范围（一行）**：本文件定稿 + 必要 normative 文档修订（仅 docs）；冻结 C1-C9 测量/渲染/Resume/报告契约与状态词汇。

<!-- T42 正文待扩充：前置/参考/范围/产出/验收/禁区/命令/commit -->

## T43 · 性能埋点真值化 + 冻结基线（P7/P2/P8）（A2-04）

- **前置**：T42。
- **范围（一行）**：重写 `xtask` 埋点为真值语义（首帧可交互、receive-to-render、release RSS raw bytes），不调优生产代码，产出 S8 冻结基线。**（S7 合并后复核冻结基线）**

<!-- T43 正文待扩充 -->

## T44 · 变高会话虚拟化：万行会话滚动 P1（A2-04）

- **前置**：T43 基线；S7 meter/summary UI 已合并。
- **范围（一行）**：顶层会话从 `uniform_list` 24px 定高迁移为变高语义项（markdown/卡片/CJK/代码），10k 混合 fixture 基准，冻结区零重排。**（S7 合并后复核冻结基线）**

<!-- T44 正文待扩充 -->

## T45 · 会话历史分页与重启水合（A11-03）

- **前置**：T44。
- **范围（一行）**：`messages.seq` cursor 分页查询 + batch tool_calls 关联 + typed 投影 + UI 异步水合/prepend，六表内、无 N+1、UI 零 SQLite。

<!-- T45 正文待扩充 -->

## T46 · Stop / 启动修复 / 显式 Resume E2E 完整覆盖（A2-17/A3-10）

- **前置**：T45。
- **范围（一行）**：Composer Stop 可见可键盘可达、全链路取消、durable terminal p99 <1s；重启先修复后水合；显式 Resume = 一次性 auditable 新 run，禁 replay。

<!-- T46 正文待扩充 -->

## T47 · Phase 1 P0 收口（A1-05/A2-14/A2-12 等）

- **前置**：T46；T42 批准的 P0 清单与 S7 真实 API。
- **范围（一行）**：fresh provider/model/thinking 选择器、bounded `@file`、Composer >8 行内滚、全量 P0 审计闭环（>3 commits 则串行拆卡）。

<!-- T47 正文待扩充 -->

## T48 · memory_idle / 渲染 / UI 调优 + ui-spec 自动化收口（A2-04）

- **前置**：T43-T47 形状稳定。
- **范围（一行）**：对 T43 冻结 schema profile+调优 memory_idle 至 P8 <100MB（历史数字 107MB 不可比），收口 P1-P8 自动化项与 ui-spec §6 可自动化部分。**（S7 合并后复核冻结基线）**

<!-- T48 正文待扩充 -->

## T49 · 确定性 Phase 1 验收 + s8-report 草稿（A3-10）

- **前置**：T42-T48 全部 squash 合并。
- **范围（一行）**：一条 E2E-first 全链路 fixture（改码→diff→commit→成本→中断→Resume）+ `docs/vega-s8-report.md` + README 真值化（engineering fixture，非 milestone passed）。

<!-- T49 正文待扩充 -->

## T50 · 人类/硬件/dogfood 收口（HUMAN PENDING）

- **前置**：T49 squash 合并；provider 授权。
- **范围（一行）**：ProMotion 实测 120fps、真实账单 <5%、真实仓库任务、7 天独立 dated dogfood——**人类执行，缺硬件/账号/周期即 pending，不伪造**。

<!-- T50 正文待扩充 -->

---

## S8 完成定义（DoD 草案，T42 冻结）

<!-- 待扩充 -->

## 变更记录

- v0.1 (2026-08-31) 骨架：Sprint 目标/DoD 草案、状态词汇、T42-T50 九卡编号/前置/一行范围。基于预检 `/tmp/vega-s8-sdd-preflight-v2.md` 并在 `b96fcef` 核实（spawn_to_exit、rss_mb MiB 标签混淆、ROW_HEIGHT=24.0、uniform_list 现状）；S7 T36/T37/T38 已合并，T39-T41 未合并故标复核点。

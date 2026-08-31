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

- **前置**：S7 T41 squash 合并；复核 T37-T41 真实 API（T37/T38 已合并可直查 #40/#41；T39/T40/T41 以合并后代码为准）。
- **参考**：[vega-phase1-plan.md](vega-phase1-plan.md) §2 S8 行与缓冲策略；[vega-features.md](vega-features.md) P0 清单（A2-12/A2-14/A2-17/A3-10/A11-03）；[vega-ui-spec.md](vega-ui-spec.md) §5/§6；[vega-tech-risks.md](vega-tech-risks.md)（等高列表不能表达会话消息）。
- **范围**：
  - 定稿本文档 T42-T50 卡面；只做必要的 normative 修订（P7/P8 测量语义写入 ui-spec §5 或 phase1-plan 勘误），**零 .rs/Cargo.toml/migration 改动**。
  - 冻结测量契约：
    - **C1（P7）**：指标名 `process_start_to_first_rendered_interactive`——父进程 spawn 前 timestamp；隔离子进程构建真实根视图/route/focus/空 Composer，注册 pinned GPUI next-frame callback，flush 恰一条严格 JSON milestone 后正常退出；20 个全新进程 nearest-rank p95 `<50.000ms`，整数微秒保留 p50/p95/p99/max。next-frame ≠ 物理 present；若要求物理 present 须先有公开语义 seam，否则 stop。milestone 缺失/重复/畸形、早退、超时、真实 profile/Keychain 访问、provider/network 访问、固定 sleep 或 kill-当-成功均判失败。
    - **C2（P8）**：release RSS raw bytes（`proc_pidinfo pti_resident_size`）；阈值字面权威 `<100,000,000` bytes（decimal MB），或人类批准改 `104,857,600`（100 MiB）——**T42 裁决后冻结，测后永不换单位**；MB/MiB 仅作显示字段。空单窗口、无任务、隔离 preseeded profile、provider/key/network 禁用；20 个全新 release 进程在 C1 采样后 +5/+10/+15s 各采样取 per-process median，nearest-rank p95 决定；`+15s−+5s > 2MiB` 的轮次超过 1 则标记不稳；p95 落 `[98,000,000, 102,000,000)` 灰区须同机同二进制补 20 轮合并 40。物理 footprint 可作诊断，不能替代 RSS。
    - **C3（provenance/隔离）**：每个产物记录 Git HEAD/dirty、release profile、绝对二进制路径+size/mtime/SHA-256、构建命令/exit、`rustc -Vv`、OS/CPU/GPU/显示器刷新率、场景与 fixture hash、轮次/原始样本、local+UTC cutoff、结果文件 SHA-256；无条件重建 release 或解析 exact Cargo artifact（文件存在≠provenance）。bench 持临时 `HOME`（macOS 数据在 `$HOME/Library/Application Support/ai.vega`）、预 seed 安全 profile、bypass Keychain/provider/network、写 `/tmp`、不弄脏 repo。
    - **C4（24px 规则）**：24px 仅为 compact-subrow（diff 行/卡内行）规则；顶层会话一项=一个语义 user/assistant/tool/permission/plan/artifact/summary item 自然高度；仅 mutable tail 或显式失效 item 可 rematerialize，冻结 ID/render node 稳定；禁止以截断凑高度。
    - **C5（Stop/Resume）**：Stop 可见、键盘可达、first-wins，取消 provider 流/权限等待/tool future/自有进程组，durable 行达 terminal interrupted/cancelled/error 且确定性矩阵 p99 `<1s`；启动修复先于 transcript 投影，部分文本保持可见且不可变；Resume 在旧行全 terminal 后追加一条 auditable continuation 开新 run；永不把旧消息改回 streaming、不自动 replay 成功/拒绝/取消/完成未知的 mutating tool；crash-after-effect 残差显式记录为非 exactly-once；generation/run/route/window fence 丢弃一切晚到旧回调。
    - **C6（P1/P2）**：P1 自动化记录 build/layout/paint 分布对 8.33ms + 队列界 + 冻结 remat；字面 120fps 需实测 ≥120Hz provenance 且 median ≥120fps、任一秒窗 ≥100fps，60Hz 主机只证 CPU/build margin 并报 `hardware pending`；P2 在生产 controller 入口对每个 bounded/coalesced batch timestamp，关联最高 sequence 与首个包含帧；终证为 5 分钟 release soak @1,000 deltas/s p99 `<16.000ms`、队列/通知有界、UI 线程零 DB/定价 IO、RSS 无无界斜率；10s 短跑供日常 PR 反馈。parser/enqueue/build 时间不得替代 receive-to-render。
    - **C7（分页）**：`messages.seq` typed headless cursor/page + batch 关联 `tool_calls` + 一致读快照 + 默认/硬上限 200；含 interrupted/failed 行与 S7 summary 引用，无 raw tool inputs/secrets；UI 收 typed 投影零 SQLite 调用；无第七表、无 N+1。
    - **C8（报告真值）**：状态词汇不可互换（见文首）；mock 账单误差 0 ≠ 真实账单；一周=七个独立 dated dogfood 日；报告有 evidence cutoff、列已合并 squash hash、自标本 PR/squash `PENDING`，禁未来 hash。
  - **P0 审计**：复核 vega-features.md 全部 P0（重点 A2-12 `@file`、A2-14 provider/model/thinking 选择器、A2-17 Stop、A3-10 恢复、A11-03 分页），逐项映射 T44-T47 或显式 human deferral；若 model/thinking 持久化需在 S7 migration 后追加列，须显式六表 migration 裁决。
- **产出**：本文档定稿；ui-spec/phase1-plan 必要勘误（docs）。
- **验收**：T42-T50 每卡含前置/参考/范围/产出/验收/禁区/命令/commit；P1-P8 各有唯一 owner 卡；文档内链全部可解析；`git diff --check` 干净；依赖/六表扫描无变化。
- **禁区**：改任何 .rs/Cargo.toml/migration；虚构 T39-T41 API；把预检报告当圣经照抄（数字/seam 以合并代码为准）。
- **命令**：`export PATH="$HOME/.cargo/bin:$PATH" && cargo fmt --all -- --check && git diff --check`（docs-only 卡不跑全量测试）。
- **commit**：`docs(A2-17): define Sprint 8 Phase 1 closure`（≤3）。
- **Stop**：任何裁决（MB/MiB、rendered vs physical present、Resume residual、120Hz 边界、P0 deferral/DDL）或 S7 seam 未定。

## T43 · 性能埋点真值化 + 冻结基线（P7/P2/P8）（A2-04）

- **前置**：T42 合并。
- **参考**：`xtask/src/main.rs`（`measure_cold_start`/`measure_memory_idle`/render probe，注意 xtask 在仓库根而非 crates/）；T42 冻结的 C1/C2/C3/C6；ui-spec §5 P1/P2/P7/P8；[vega-s6-report.md](vega-s6-report.md) 历史数字（标记 noncomparable）。
- **范围**：
  - 替换四类不可信测量（已核实于 `b96fcef`）：
    1. `cold_start` 目前 `spawn_to_exit`（`method: "spawn_to_exit"`）→ 改为 C1 首帧可交互；
    2. release 助手接受既有 target 二进制 → 无条件重建 release 或解析 exact Cargo artifact；
    3. `memory_idle` 单次 debug 5s 采样、`rss_mb` 实为 bytes/2^20（MiB 冒充 MB 标签）→ 改为 C2 release 20 进程协议 + 单位先行裁决；
    4. 渲染输出硬编码 60Hz、流式 probe ~500 deltas/s → 真实刷新率探测 + 1,000/s 流。
  - provenance 与隔离按 C3 全量落进 JSON 输出；bench 临时 `HOME`、`/tmp` 输出、零真实数据/key/network。
  - P2 埋点：生产 controller 入口 timestamp per bounded/coalesced batch，关联最高 sequence 与首个包含帧；10s 生产 controller 流 @1,000/s 供日常 PR 反馈，5 分钟 soak 留 T48/T49。
  - **冻结基线 schema**：字段名/单位/轮次/percentile 数学/threshold 写死进代码与文档；T48 只消费不得改判据。
  - 不调优任何生产代码（那是 T48 的事）。
- **产出**：xtask 埋点重写；repo-external JSON 基线（raw samples + provenance + 真实状态词）；基线对照表（历史 S6 数字标 noncomparable）。
- **验收**：
  - E2E：一个隔离 release 子进程构建真实 root/action/Composer、发恰一条 next-frame milestone、供三个 RSS 样本、正常退出；10s 生产 controller 流 @1,000/s 全 sequence 记录。
  - 窄测：percentile/guard-band 数学；milestone 缺失/重复/畸形；timeout/早退；stale provenance 拒绝；MB/MiB 显示分离；temp-HOME/输出清理；有界 render 关联。
  - 状态词真实：未达标写 `performance gate failed`，不漂白。
- **禁区**：本卡调优生产代码；sleep/kill 当成功；为好看结果改 percentile 数学或单位；把 next-frame 宣称成物理 present。
- **命令**：

  ```sh
  export PATH="$HOME/.cargo/bin:$PATH" && cargo fmt --all -- --check
  export PATH="$HOME/.cargo/bin:$PATH" && cargo clippy --workspace --all-targets --all-features -- -D warnings
  export PATH="$HOME/.cargo/bin:$PATH" && cargo test --workspace --all-features
  export PATH="$HOME/.cargo/bin:$PATH" && cargo build --workspace --all-features
  export PATH="$HOME/.cargo/bin:$PATH" && cargo xtask bench
  git diff --check
  ```
- **commit**：`feat(A2-04): make Phase 1 benchmarks truthful`（≤3）。
- **Stop**：无有效 frame seam；隔离失败；成功仍需 sleep/kill。
- **（S7 合并后复核冻结基线）**：T39-T41 若新增流式/meter 路径，埋点须覆盖其真实 controller 入口与事件链，基线数字在 T43 开工时以合并后 master 重测冻结。

## T44 · 变高会话虚拟化：万行会话滚动 P1（A2-04）

- **前置**：T43 合并（基线已冻结）；S7 meter/summary UI 已合并。
- **参考**：`crates/vega_ui/src/conversation_stream.rs`（`ROW_HEIGHT: f32 = 24.0`、`uniform_list`、truncation——已核实于 `b96fcef`）；[vega-tech-risks.md](vega-tech-risks.md)（等高列表不能表达会话消息）；pinned GPUI 变高列表真实 API（开工时先验证 identity/tail-follow/splice/anchor 能力，不照抄旧 sketch）。
- **范围**：
  - 顶层会话从 `uniform_list`+24px 定高迁移为 C4 语义项：一项=一个 user/assistant/tool/permission/plan/artifact/summary item，自然高度；markdown/换行 CJK/emoji/代码/全部卡型皆完整呈现，禁截断。
  - 仅 mutable tail 或显式失效 item 可 rematerialize；冻结 ID/render node 稳定。
  - 保留 S7 事件链（UsageUpdated/meter/summary card）与全部 fence（generation/run/route/window），不反向改 S7 契约。
  - 24px 保留给 diff 行、卡内行等真正等高 compact-subrow；迁移后每个残余 24px 命中逐一分类。
  - 基准迁移到生产语义：P1 记录 10k 语义项 build/layout/paint 分布对 8.33ms + 队列界 + 冻结 remat。
- **产出**：变高虚拟化实现；10k 混合 fixture（markdown/wrapped CJK/emoji/代码/全卡型）。
- **验收**：
  - E2E：10k 混合 fixture 走 tail streaming/detach/prepend/resize/卡展开/return-to-bottom/CJK+代码换行；断言稳定 ID、冻结区 remat=0、无截断、follow 正确、锚点漂移 `<1px`。
  - 窄测：offscreen 高度失效；400→1600px resize；晚到事件 fence；冻结几何。
  - 扫描：无顶层定高 24px 列表残余（残余命中全部归类 compact-subrow 并记录）。
  - 60Hz 主机只证 build/layout/paint margin，报 `hardware pending`（P1 字面 120fps 归 T50 ProMotion）。
- **禁区**：压平会话为行模型凑等高；引入新列表依赖；截断内容；改 S7 事件/fence 契约；本卡顺带做 RSS 调优（归 T48）。
- **命令**：同 T43 全量门禁 + `cargo xtask bench`；扫描 `rg -n "uniform_list|ROW_HEIGHT|24\.0" crates/vega_ui/src/` 并逐条分类。
- **commit**：`feat(A2-04): virtualize variable-height conversation items`（≤3）。
- **Stop**：pinned GPUI 无法保持 identity/anchor，或需要压平/新依赖。
- **（S7 合并后复核冻结基线）**：T39/T40 的 meter/summary card UI 合并后，10k fixture 须包含 summary card；P1/P8 基线随变高渲染重测（旧 24px 数字标 noncomparable）。

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

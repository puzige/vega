# ✦ Vega — S8 任务卡（Sprint 8 · 打磨 & 里程碑 · W15-16）

**版本** v0.5 · 2026-08-31 · 使用方式：每张任务卡 + [vega-exec-guide.md](vega-exec-guide.md) = 一条完整的执行 prompt

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

- **前置**：T44 合并。
- **参考**：`vega_store` schema（`messages.seq`、`tool_calls.message_id`/`seq`——已核实支持 cursor 分页）；[vega-features.md](vega-features.md) A11-03；T42 冻结的 C7；S5/S7 recovery seam（`vega_store::recovery`）。
- **范围**：
  - `vega_store`：typed headless cursor/page 查询——`messages.seq` 有界 page（默认/硬上限 200）、batch 关联 `tool_calls`、单一一致读快照；含 interrupted/failed 行与 S7 summary 引用；排除 raw tool inputs/secrets（redaction 沿用既有 owner 测试，不重造）。
  - `vega_conversation`：typed 投影组装（不做 N+1）；重启时 controller 重建后先 repair 再投影。
  - `vega_ui`：异步水合/prepend——UI 只收 typed 投影，**零 SQLite 调用**；页边界保 anchor；A→B→A 路由切换丢弃晚到页。
  - 恰好六表；无第七表、无新依赖。
- **产出**：分页查询 + typed 投影 + 水合 UI；10k 行 seed fixture（含 tools/plans/interrupted/failed/S7 summaries）。
- **验收**：
  - E2E：seed 10k 行 → 重建 controller → 打开最新 200 → 无 gap/重复/N+1 连续翻页 → anchor 保持 → A→B→A 切换 → 拒绝晚到页；重启后 durable transcript（含成本/中断态/summary）完整可见。
  - 窄测：page size 0/1/199/200/201 边界；seq boundary/overflow/corruption；快照一致性；每页 SQL 次数断言（≤常数）；redaction 断言。
  - 性能：最新页打开 <100ms（记录 host provenance，报测量值不漂白）。
- **禁区**：UI 线程同步查库；UI 读 SQLite/定价文件；第七表；为省查询丢 interrupted/failed 行；把 S7 聚合行重新展开成 raw 输入。
- **命令**：同 T43 全量门禁；`rg -n "rusqlite" crates/vega_ui/src/`（须零命中）。
- **commit**：`feat(A11-03): hydrate paged conversation history`（≤3）。
- **Stop**：安全一致性要求新表、N+1 或 UI DB 访问。
- **（S7 合并后复核冻结基线）**：summary 引用格式以 T40 合并后的真实落库形态为准。

## T46 · Stop / 启动修复 / 显式 Resume E2E 完整覆盖（A2-17/A3-10）

- **前置**：T45 合并。
- **参考**：[vega-features.md](vega-features.md) A2-17（Stop）/A3-10（恢复）；`vega_runtime`/`vega_conversation` 层级取消与 route/run/generation fence（S4-S5 已合并 seam）；`vega_store::recovery` 原子修复；T42 冻结的 C5。
- **范围**：
  - Composer Stop：可见、键盘可达、first-wins；生产链路取消 ownership 贯穿 provider 流、权限等待、tool future、自有进程组。
  - Durable 行达 terminal（interrupted/cancelled/error）后 UI 呈现；startup repair（先于 transcript 投影）保部分文本可见且不可变。
  - 显式 Resume：旧行全 terminal 后追加一条 auditable continuation 开新 run；禁 replay 已成功/拒绝/取消/完成未知的 mutating tool；crash-after-effect 残差显式标注（非 exactly-once，自动 replay 永远禁止）。
  - 复用既有低层测试 seam；本卡新增的是**可见端到端旅程**与 restart hydration，不重造 S3-S7 owner 测试。
- **产出**：Stop/repair/Resume 全链路实现补齐 + 100 例确定性延迟矩阵。
- **验收**：
  - E2E：delayed MockProvider 产出部分文本 + 权限等待 + 自有子进程 → 生产 Stop → 恰一条 terminal durable interruption + 清理 → controller 重启 → repair 先于水合 → 一次 Resume 开恰一个新 run 零 replay。
  - 窄测：100 例延迟矩阵 p99 `<1s` terminal 化；duplicate Stop/Resume；并发 terminal；close/switch route；stale generation fence；crash-after-effect 的呈现。
  - 全部旧 provider/tool 调用达 terminal；无 fabricated success。
- **禁区**：把 Resume 做成自动续传/replay；吞掉第二例 Stop（非 first-wins）；crash 残差洗成 exactly-once；Stop 后仍允许旧回调上屏（fence 失守）。
- **命令**：同 T43 全量门禁。
- **commit**：`feat(A2-17): add recoverable task interruption`（同时覆盖 A3-10；≤3）。
- **Stop**：安全 Resume 需要 replay，或进程组所有权达不到 p99 KPI。
- **（S7 合并后复核冻结基线）**：中断行的 token/cost 呈现以 T38/T39 合并后的真实 usage/provisional 语义为准（无 usage 显 `—` 不冒充 0）。

## T47 · Phase 1 P0 收口（A2-12/A2-14 等）

- **前置**：T46 合并；T42 批准的 P0 审计清单与 S7 真实 API。
- **参考**：[vega-features.md](vega-features.md) A2-12（`@file`）/A2-14（provider/model/thinking 选择器）/A2-17；ui-spec §4 Composer 规格；[vega-prd.md](vega-prd.md) P0 行为定义。
- **范围**：
  - fresh provider/model/thinking 选择：全新 temp profile 精确选 fake provider/model/effort，重启后精确保持；选择器与 thinking 档位若有持久化需求，按 T42 的六表 migration 裁决执行（禁止未批准 DDL）。
  - bounded `@file` 注入：确定性文件序、ignore 规则、repo root 边界、symlink、non-UTF8、数量/字节上限；escape/超限/未定价 model 时零 provider 请求。
  - Composer >8 行内滚（inner scroll），键盘/焦点链不断。
  - 全量 P0 审计闭环：T42 清单逐项 ✅（附证据）/ deferral（显式记录+理由），不许含糊。
  - **超过 3 个 cohesive commits 必须先串行拆卡再实现，禁止部分关闭 P0 凑数。**
- **产出**：上述 P0 功能实现 + 每项 owner E2E/窄测。
- **验收**：
  - E2E：fresh temp profile → 精确选择 → 注入一个有界 in-repo 文件 → >8 行 Composer 全控件可渲染可达 → 重启选择精确保持；unknown/unpriced/escape/oversize 全部零 provider 请求。
  - 窄测：deterministic file order/ignore/root/symlink/non-UTF8/count/bytes；route/restart/late selection；keyboard/focus/inner scroll。
  - 零真实 discovery/credential/请求。
- **禁区**：未批准 migration；不安全上下文注入（越 repo root、超字节）；P0 留白不标 deferral；拆卡时顺手改无关功能。
- **命令**：同 T43 全量门禁；`rg -n 'CREATE TABLE|ALTER TABLE' crates migrations` 逐条对照 T42 裁决。
- **commit**：exact feature-ID Conventional Commit subjects（≤3）。
- **Stop**：未批准 migration、不安全上下文、缺 fresh runnable path、或存在未 deferral 的开放 P0。

## T48 · memory_idle / 渲染 / UI 调优 + ui-spec 自动化收口（A2-04）

- **前置**：T43-T47 合并（形状稳定）；T43 冻结的基线 schema 不得改动。
- **参考**：T43 基线 JSON；ui-spec §5/§6 全表；[vega-s6-report.md](vega-s6-report.md) 历史数字（107MB 上下、MiB/MB 标签混淆——noncomparable，仅作方向参考）。
- **范围（profile 先于调优，顺序固定）**：
  1. 用 exact binary/scene 复现 C2 raw RSS，用既有/系统工具归因 retained 对象/缓存；
  2. 仅在 ownership 证明不必要后移除 eager/stale view state（closed route/thread/project、过时 diff/artifact/summary/page、test-probe state）；
  3. bound 队列/页/缓存，丢弃 superseded 投影——不弱化 durable audit/recovery/redaction/fence；
  4. 同 provenance/scene 对比；保留失败尝试记录；**永不调单位/轮次/threshold**；
  5. 跑 C2 20/40 gate + 非规范 idle soak 观察增长。
- **收口清单**：P7（C1 20 进程）、P8（C2）、P1（变高 10k 滚动，60Hz=margin+hardware pending）、P2（5 分钟 1k/s soak p99 <16ms）、P3（冻结几何/remat 0）、P4（锚点 <1px）、P5（key/click p95 <100ms）、P6（只允许卡展开 150ms ease-out / 权限滑入 120ms，无装饰动画）；ui-spec §6 可自动化项：token 色扫描分类、Light/Dark render/state tests、CJK 不 panic、keyboard action/focus 链（含 Stop/Resume/diff/commit）、960×600 layout constraints。
- **产出**：优化 commits + 全套基准 JSON（raw samples + provenance）+ ui-spec 自动化证据表。
- **验收**：C1/P7 与 C2/P8 gate 通过（字面阈值，T42 冻结单位）；P2 soak 通过；扫描族逐条分类（硬编码色值、UI SQLite/定价 IO、GPUI 越界、第七表、`spawn_to_exit`/startup sleep/`rss_mb`/硬编码 60Hz/~500/s 残留、secret/生产 unwrap）。
- **禁区**：以截断 transcript/tool audit、禁用检查、开非交互空壳、压制 theme/font 工作、把工作挪到首帧 milestone 之后来"达标"；改判据自证；掩盖失败尝试。
- **命令**：同 T43 全量门禁 + `cargo xtask bench`（全套场景）；T43/T48 额外跑 exact release probes（temp HOME + `/tmp` 输出）。
- **commit**：`perf(A2-04): meet Phase 1 performance gates` + 至多两个 exact-fix commits（≤3）。
- **Stop**：任何绝对 gate 失败，或达标需要 stale/debug/定高二进制、改判据、丢 audit、放宽安全。
- **（S7 合并后复核冻结基线）**：全部 P1-P8 数字以 T43 在合并后 master 重测的基线为对照。

## T49 · 确定性 Phase 1 验收 + s8-report 草稿（A3-10）

- **前置**：T42-T48 全部 squash 合并；S7 T41 squash 已合并。
- **参考**：[vega-s7-tasks.md](vega-s7-tasks.md) T41 report 结构与门禁写法；S4-S7 owner 证据（temp repo/diff/artifact/fake-launcher/两段式 commit/事件序/取消）；T42 冻结的 C8。
- **范围**：
  - 一条 E2E-first 全链路 fixture（fresh temp repo/HOME + MockProvider）：安全 edit/tool → provisional usage 校准到 exact durable priced rows/summary → diff/artifact/fake-open eligibility → dirty branch 拒绝 → 可信两段式 commit 到 exact tree → clean switch → 停掉 delayed 下一 run → 重建/水合 transcript/summary/cost/interrupted 态 → Resume 一次零 replay。零真实 provider/key/network、零 `/usr/bin/open`、零 developer repo/global Git config。
  - 断言：provider/tool/permission/event 计数与顺序 exact；校准成本与模拟账单误差 0；commit parent/tree/ref/clean state；fake-launcher owner 证据；恰好六表/迁移清单；fence 生效；恰一次中断/Resume。
  - `docs/vega-s8-report.md`：evidence cutoff、命令/exit、test/doctest/probe 计数分开（不重复计 doctest、不接受过滤后零测试命令）、已合并 PR/squash 表、branch commits + 自身 squash `PENDING`、release provenance/原始 JSON hash、P1-P8 逐项、ui-spec §6 矩阵（自动化/人工/硬件三分，不能自动化的不写 ✅）、P0 审计/deferral、schema/依赖/红线扫描分类、deviations/residuals。
  - README 状态行真值化：S8 收口为 `engineering fixture passed`，非 `Phase 1 milestone passed`（后者仅 T50）。
- **产出**：全链路 E2E + s8-report + README 更新。
- **验收**：fixture 全断言绿；报告每节可追溯到命令/hash；门禁 discovery 与 execution 分开捕获：

  ```sh
  set -o pipefail
  export PATH="$HOME/.cargo/bin:$PATH"
  cargo test --workspace --all-features -- --list 2>&1 | tee /tmp/vega-s8-tests-list.log
  cargo test --workspace --all-features -- --format terse 2>&1 | tee /tmp/vega-s8-tests-run.log
  cargo fmt --all -- --check && git diff --check
  ```
- **禁区**：用 mock/硬件 pending 冒充 milestone；发明未来 hash；隐藏 deviation；为凑数把 ignored/platform-gated 测试计入 pass。
- **commit**：`test(A3-10): cover Phase 1 milestone end to end` + `docs(A3-10): report Phase 1 engineering acceptance`（≤3）。
- **Stop**：需要真实数据/key/launcher；把 mock 称为 dogfood；或只能靠虚构未来 squash 才能收口。

## T50 · 人类/硬件/dogfood 收口（HUMAN PENDING）

- **前置**：T49 squash 合并；provider 账号授权。**本卡人类所有，不得指派给 executor**（executor 不索取 key、不发真实请求、不产生费用）。
- **参考**：T49 报告的 pending 清单与操作模板（无 key 版）；T42 冻结的 C8；PRD/phase1-plan 里程碑验收行。
- **范围（全部人类执行）**：
  - **ProMotion 实测**（HARDWARE PENDING）：≥120Hz 真机跑 P1 10k 滚动，median ≥120fps 且任一秒窗 ≥100fps；60Hz 主机的 `hardware pending` 状态由本卡消除或维持。
  - **真实账单**（BILLING PENDING）：授权的非秘密真实仓库任务（改码→diff→commit、成本全程可见）；nonzero billed cost 与匹配 provider/model/currency/time window，`abs(vega − invoice) / invoice × 100 < 5%`。
  - **7 天 dogfood**：七个独立 dated dogfood 日/构建，逐日记录 task/result/failure/perf/UX。
  - 报告/README 收口为 `Phase 1 milestone passed`（仅当全部真实/硬件/周期证据齐备）。
- **验收（可自动复核部分）**：重算 billing 百分比/时间窗；校验七个日期独立；≥120Hz 下无任一秒 P1 样本 <100fps；hash/cutoff/T49 squash 一致；全部门禁与红线扫描仍绿。
- **禁区**：未授权消费；零/不匹配账单充数；60Hz 冒充 ProMotion；<7 天；敏感证据入库；证据缺失时提前宣布 milestone passed。
- **commit**：`docs(A3-10): close Phase 1 milestone evidence`（≤3），cutoff 处自身 squash 仍标 `PENDING`。
- **Stop**：账号/硬件/周期任一缺失 → 状态保持 pending，不伪造、不算工程失败。

---

## S8 完成定义（DoD）

- [ ] T42-T49 全部 squash merge；master 门禁全绿；`Cargo.lock` 与内部依赖接线一致；零新外部依赖、零第七表。
- [ ] P7/P8/P1/P2 判据以 T42 冻结语义测量：首帧可交互 20 进程 p95 `<50ms`；release RSS raw-byte p95 `<100MB`（单位按 T42 裁决）；变高 10k 滚动 build/layout/paint 分布达标（60Hz 报 hardware pending）；5 分钟 1k/s 流 p99 `<16ms`、队列有界、零 UI 线程 IO。
- [ ] 顶层会话零定高 24px 列表；残余 24px 命中全部归类 compact-subrow；冻结区 remat=0、锚点 <1px、无截断。
- [ ] 分页/水合：六表内 cursor 分页无 gap/重复/N+1；UI 零 SQLite；重启 durable transcript 完整。
- [ ] Stop/Resume：确定性矩阵 p99 `<1s` terminal；一次 Resume 一个新 run 零 replay；crash-after-effect 残差显式记录。
- [ ] P0 审计闭环：每项 P0 有证据或显式 deferral；无未批准 DDL。
- [ ] ui-spec §6：token 色/Light-Dark/CJK/keyboard/960×600 自动化证据齐；人工/硬件项如实标注。
- [ ] `docs/vega-s8-report.md`：cutoff/命令/计数/PR 表/P1-P8/ui-spec/P0/扫描分类/偏离残差齐全，自身 squash 标 `PENDING`；README 标 `engineering fixture passed`。
- [ ] 真实账单 <5%、ProMotion 120fps、真实仓库任务、7 天 dogfood 标 **human/hardware pending** → 由 T50 收口；零真实 key/请求/费用。

## S7 合并后需复核的冻结基线清单

T39/T40/T41 合并前，下列数字/API 不得当权威引用；各卡开工时以合并后 master 复核并冻结：

1. **T43**：流式/meter controller 入口与事件链（T39/T40 可能新增路径）→ 基线 JSON 重测冻结。
2. **T44**：meter/summary card UI 高度语义进变高 item → P1/P8 基线随变高渲染重测（旧 24px 数字 noncomparable）。
3. **T45**：summary 引用落库形态（T40）→ 分页投影字段对齐。
4. **T46**：中断行的 usage/provisional 呈现（T38/T39）→ 无 usage 显 `—`。
5. **T48**：全部 P1-P8 对照基线 = T43 复核后的冻结数字。
6. **T42 契约本身**：若 T41 报告更新了 ui-spec/phase1-plan normative 文本，C1-C8 引用行号/措辞同步勘误。

## 变更记录

- v0.5 (2026-08-31) 定稿：T42-T50 全卡扩充（前置/参考/范围/产出/验收/禁区/命令/commit/Stop）；DoD 定稿；新增 S7 合并后复核基线清单。相对预检 v2 的修正见 v0.2/v0.3/v0.4。
- v0.4 (2026-08-31) T48-T50 扩充：调优顺序固化（profile 先于删除）；ui-spec 自动化收口清单入 T48；T49 门禁 discovery/execution 分开；T50 明确人类所有与自动复核部分。
- v0.3 (2026-08-31) T45-T47 扩充：分页边界值与性能验收量化；Stop/Resume 100 例矩阵；P0 收口加六表 migration 红线。
- v0.2 (2026-08-31) T42-T44 扩充：C1-C8 契约落卡；xtask 路径勘误（仓库根 `xtask/src/`，非 `crates/xtask/`）；S7 进度更新（T36/T37/T38 已合并 #35/36/40/41，T39/T40/T41 未合并标复核点）。
- v0.1 (2026-08-31) 骨架：Sprint 目标/DoD 草案、状态词汇、T42-T50 九卡编号/前置/一行范围。基于预检 `/tmp/vega-s8-sdd-preflight-v2.md` 并在 `b96fcef` 核实（spawn_to_exit、rss_mb MiB 标签混淆、ROW_HEIGHT=24.0、uniform_list 现状）；S7 T36/T37/T38 已合并，T39-T41 未合并故标复核点。

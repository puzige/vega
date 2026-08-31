# ✦ Vega — S8 SDD：Phase 1 收口契约冻结（S8-T42）

**版本** v1.0 · 2026-08-31 · 状态：冻结（P8 单位一项 OPEN，见 §10）
**来源卡**：[vega-s8-tasks.md](vega-s8-tasks.md) T42（本文档为其唯一产出 SDD；卡面与本文冲突时以卡面为准并勘误本文）
**关联**：[vega-exec-guide.md](vega-exec-guide.md)（红线/证据分级）· [vega-phase1-plan.md](vega-phase1-plan.md)（S8 行与 DoD）· [vega-ui-spec.md](vega-ui-spec.md) §5/§6（P1-P8 准线）· [vega-features.md](vega-features.md)（P0 清单）· [vega-tech-risks.md](vega-tech-risks.md)（等高列表不能表达会话消息）

> **契约地位**：C1-C8 与状态词汇自本文合并起冻结；T43-T50 只消费，不得改判据自证。任何修订须走 docs 勘误并注明动机，测后禁换单位/轮次/阈值/percentile 数学。
>
> **用语约定**：`MUST` = 契约必备，违反即判失败；`MAY` = 允许的 diagnostics，不得替代主判据；`FORBIDDEN` = 出现即判失败。判定均为可机械复核（脚本/断言可重放）。

## 0. 证据基线与 S7 收口状态

- 本文依据 `origin/master` `429cb2d`（T36-T39 已合并：#35 SDD、#36 T36 定价引擎、#40 T37 定价设置、#41 T38 usage 持久化、#43 T39 实时校准成本）。
- **T40（A10-06 汇总卡）实现进行中未合并、T41（S7 报告）未产出**：凡引用 S7 最终数字/状态处一律标注 **（T41 报告后同步勘误）**，本文不写死任何 S7 最终数字。
- 已在 `429cb2d` 核实的代码事实（作为契约动机，非数字基线）：`xtask` `cold_start` 现为 `spawn_to_exit`、`memory_idle` 单次 debug 采样且 `rss_mb` 实为 MiB、渲染输出硬编码 60Hz、流式 probe ~500 deltas/s；`crates/vega_ui/src/conversation_stream.rs` 顶层会话 `uniform_list` + `ROW_HEIGHT = 24.0` 且截断长内容。
- vega-s8-tasks.md 文首「测量真值裁决」段即本契约的动机陈述；T48 只对冻结 schema 调优，不得改判据自证。

## 1. 状态词汇（normative，不可互换）

以下七个状态词各绑定唯一判定主体；任何报告/README/JSON 产物只允许使用本表词汇，禁止同义词替代、漂白或把 pending 写成 pass：

| 状态词 | 含义 | 唯一可用者 |
|---|---|---|
| `engineering fixture passed` | 确定性 mock/temp 环境下全链路 fixture 通过 | T49（S8 收口）；此前各 Sprint 报告沿用 |
| `performance gate failed` | 按冻结判据测量未达标（原样记录，不漂白） | T43/T48 测量产物 |
| `human pending` | 需人类执行且未执行 | T49/T50 |
| `hardware pending` | 需 ≥120Hz 等特定硬件实测（60Hz 主机只证 margin） | T43/T48/T49 |
| `real provider/billing pending` | 需真实 provider/key/账单（executor 永不索取） | T49/T50 |
| `dogfood in progress (day N/7)` | 七个独立 dated dogfood 日未满 | T50 |
| `Phase 1 milestone passed` | 全部真实/硬件/周期证据齐备后的最终状态 | **仅 T50** |

机械判定：报告文本中出现 `Phase 1 milestone passed` 而无 T50 人类证据（真实账单对照、ProMotion 实测、七个 dated 日）即违规。

## 2. C1 — P7 首帧可交互（`process_start_to_first_rendered_interactive`）

**指标名**：`process_start_to_first_rendered_interactive`（写入 JSON schema，永不更名）。

**测量协议（MUST）**：
1. 父进程在 spawn 当前 release 二进制**前**取 timestamp（`Instant`/monotonic）。
2. 隔离子进程构建**真实**根视图/route/focus/action 树与启用态空 Composer（非空壳、非 test-only 视图）。
3. 注册 pinned GPUI **next-frame 回调**；回调内 flush **恰一条**严格 JSON milestone（单行、无额外输出、无 trailing 噪声）后请求正常退出。
4. 阈值：20 个全新进程 nearest-rank p95 `<50.000ms`；整数微秒记录并保留 p50/p95/p99/max。

**判失败（任一即 FAIL）**：milestone 缺失/重复/畸形（非严格 JSON）、进程早退、超时、访问真实 profile/Keychain、访问 provider/network、以固定 sleep 冒充首帧、kill-当-成功。

**语义边界（冻结）**：next-frame ≠ 物理 present（Metal present 无公开语义 seam）。若未来要求物理 present，须先落地公开语义 seam，否则 stop——本条为卡面保守裁决，非 OPEN 项。

**owner**：埋点与基线 T43；收口 gate T48；字面复核 T49 报告。

## 3. C2 — P8 release RSS 与单位裁决

**测量协议（MUST）**：
1. raw bytes 采集：`proc_pidinfo(...).pti_resident_size`（macOS）；单位为 raw bytes，**不经任何 2^20/10^6 换算入库**。
2. 场景：空单窗口、无任务、隔离 preseeded profile、provider/key/network 禁用。
3. 20 个全新 release 进程；每个进程在 C1 采样点之后 +5/+10/+15s 各采样一次，取 per-process median；nearest-rank p95 决定 gate。
4. 稳定性：`+15s − +5s > 2 MiB` 的轮次超过 1 → 标记不稳（`unstable`），gate 不通过，须先归因。
5. 灰区：p95 落 `[98,000,000, 102,000,000)` → 同机同二进制补 20 轮，合并 40 样本重算 p95（轮次数学不变）。
6. 物理 footprint（Mach footprint 等）MAY 记录为诊断字段，**不得替代 RSS 作为判据**。

**阈值字面权威**：**OPEN(OWNER: human)** —— 两候选利弊见 §3.1。裁决前按卡面字面权威 `<100,000,000` bytes（decimal MB）执行；若人类批准改 `104,857,600`（100 MiB），在 T43 冻结 schema 前落勘误。**裁决后写入 schema，测后永不换单位**；`MB`/`MiB` 仅作显示字段（显示换算与入库数值分离，`rss_mb` 命名残余随 T43 清除）。

### 3.1 P8 单位两候选利弊（人类裁决材料）

| | 候选 A：`<100,000,000` bytes（decimal MB） | 候选 B：`<104,857,600` bytes（100 MiB） |
|---|---|---|
| 利 | 与 phase1-plan §0 E4「空闲内存 <100MB」、ui-spec §5 P8「<100MB」字面一致，**零 normative 勘误**；更严 4.86%，对「内存仅为 Electron 1/10」叙事更保守；与 macOS 活动监视器等十进制口径对齐 | 与内存行业工具常报的 2^20 直觉一致；预算宽 ~4.86 MiB，若真实 RSS 贴线可减少假阳性 |
| 弊 | 若真实 RSS 落在 [100,000,000, 104,857,600) 会被判 fail（预算更紧） | 需勘误 ui-spec §5/phase1-plan E4 normative 文本 `100MB`→`100 MiB`；与「MB 标签 MiB 实」的历史混淆（本次要消灭的问题）同构；对外叙事 <100MB 不再字面成立 |
| 历史 | S6 报告 108.7MB 数字（MiB/MB 标签混淆，noncomparable）不能为任一候选背书 | 同左 |

裁决程序：人类在 T43 冻结 schema 前答复 A 或 B；本文 §3 阈值行与 ui-spec §5 P8 行同步勘误；T43 起判据字面不可再变。

**owner**：埋点与基线 T43；收口 gate T48；报告 T49；单位裁决人类。

## 4. C3 — Provenance 与隔离

**每个测量产物 MUST 记录**（缺一即产物无效，接收方可拒收）：
Git HEAD + dirty 状态；release profile；绝对二进制路径 + size/mtime/SHA-256；构建命令与 exit code；`rustc -Vv`；OS/CPU/GPU/显示器刷新率；场景与 fixture hash；轮次数与全部原始样本；local + UTC evidence cutoff；结果文件 SHA-256。

**二进制来源（MUST 二选一）**：无条件重建 release，或解析 exact Cargo artifact；**文件存在 ≠ provenance**（禁接受 stale target 二进制——`429cb2d` 现状缺陷，T43 修复对象）。

**隔离（MUST）**：bench 全程持临时 `HOME`（macOS 数据根 `$HOME/Library/Application Support/ai.vega` 随之隔离）；预 seed 安全 profile；bypass Keychain/provider/network（零真实凭据、零真实请求、零费用）；输出写 `/tmp`；不弄脏 repo（工作树 clean，产物不入库）。

**owner**：T43（落 JSON schema）；T48（exact binary/scene 复现）；T49（报告引用 hash 链）。

## 5. C4 — 24px = compact-subrow 规则

**冻结语义**：
1. `24px`（`ROW_HEIGHT = 24.0`）**仅为 compact-subrow 规则**：真正等高的 diff 行、卡内行等次级行。
2. 顶层会话一项 = 一个语义 item 的**自然高度**：user/assistant/tool/permission/plan/artifact/summary 七类卡型各自完整呈现；markdown/换行 CJK/emoji/代码均完整渲染，**禁止以截断凑高度**。
3. rematerialize 白名单：仅 mutable tail（流式尾项）或显式失效 item（offscreen 失效/resize/卡展开）；冻结区 remat = 0；冻结 ID 与 render node 稳定（跨滚动/resize/晚到事件）。
4. 等高列表不能表达会话消息（[vega-tech-risks.md](vega-tech-risks.md)）；迁移由 T44 执行，迁移后扫描 `rg -n "uniform_list|ROW_HEIGHT|24\.0" crates/vega_ui/src/` 的每个残余命中逐一归类 compact-subrow 或修复，不得留未归类命中。

**机械判定**：T44 E2E 断言稳定 ID、冻结区 remat=0、无截断、锚点漂移 `<1px`；扫描残留分类表入报告。

**owner**：实现 T44；扫描收口 T48；P1 基准随变高语义重测（旧 24px 数字 noncomparable）。

## 6. C5 — Stop / 启动修复 / 显式 Resume

**Stop（MUST）**：可见、键盘可达、first-wins（重复 Stop 吞掉且不产生第二条 terminal）；取消 ownership 贯穿四个域——provider 流、权限等待、tool future、自有进程组；durable 行达 terminal `interrupted|cancelled|error` 后 UI 才呈现终态；确定性延迟矩阵（100 例）p99 `<1s`。

**启动修复（MUST）**：startup repair **先于** transcript 投影执行；部分文本保持可见且不可变（不得因修复丢字/改字）。

**Resume（MUST）**：旧行**全部** terminal 后，追加**恰一条** auditable continuation 行并开新 run；**FORBIDDEN**：把旧消息改回 streaming；自动 replay 已成功/已拒绝/已取消/完成状态未知的 mutating tool。

**crash-after-effect 残差（冻结）**：外部效果已发生但 terminal 未落库的窗口，残差显式记录为**非 exactly-once**；Resume 先检视当前状态再行动——本条为卡面保守裁决，自动 replay 永远禁止，非 OPEN 项。

**fence（MUST）**：generation/run/route/window 四类 fence 丢弃一切晚到旧回调（旧回调不得上屏、不得写库）。

**owner**：实现与可见旅程 T46（复用 S3-S7 owner 测试 seam，不重造）；gate T48（keyboard 链含 Stop/Resume）。

## 7. C6 — P1/P2 测量与显示边界

**P1（万行滚动）MUST**：
- 自动化记录 build/layout/paint 分布对 8.33ms 帧预算 + 队列界 + 冻结 remat（C4）；场景为 10k 混合语义项（markdown/wrapped CJK/emoji/代码/全卡型）。
- 字面 120fps 判定：须有**实测 ≥120Hz** 的 provenance 且 median ≥120fps、任一秒窗 ≥100fps；60Hz 主机只证 CPU/build margin 并报 `hardware pending`（字面 120fps 归 T50 ProMotion 实测）——本条为卡面冻结，非 OPEN 项。

**P2（流式上屏）MUST**：
- 在**生产 controller 入口**对每个 bounded/coalesced batch 打 timestamp，关联该批最高 sequence 与首个包含它的帧；parser/enqueue/build 时间**不得替代** receive-to-render。
- 终证：5 分钟 release soak @1,000 deltas/s，p99 `<16.000ms`、队列/通知有界、UI 线程零 DB/定价 IO、RSS 无无界斜率。
- 日常 PR 反馈用 10s 短跑（同 schema，非终证）。

**机械判定**：JSON schema 字段名/单位/轮次/percentile 数学/threshold 由 T43 写死；T48 只消费。

**owner**：埋点 T43；P1 变高实现 T44；收口 gate T48；字面 120fps T50。

## 8. C7 — 会话历史分页

**MUST**：
- `vega_store`：`messages.seq` typed headless cursor/page 查询，batch 关联 `tool_calls`，单一一致读快照；默认页大小与硬上限 **200**（page size 0/1/199/200/201 边界全测）。
- 内容完整性：含 interrupted/failed 行与 S7 summary 引用（引用格式以 T40 合并后的真实落库形态为准，**T41 报告后同步勘误**）；排除 raw tool inputs/secrets（redaction 沿用既有 owner 测试）。
- `vega_ui`：只收 typed 投影，**零 SQLite 调用**（`rg -n "rusqlite" crates/vega_ui/src/` 须零命中）；异步水合/prepend，页边界保 anchor，A→B→A 路由切换丢弃晚到页。
- 恰好六表：无第七表、无新依赖、无 N+1（每页 SQL 次数 ≤ 常数，测试断言）。

**owner**：实现 T45；gate T48（UI 零 IO 扫描）；报告 T49。

## 9. C8 — 报告真值

**MUST**：
- 状态词汇按 §1，不可互换；mock 账单误差 0 ≠ 真实账单（真实账单定义：nonzero billed cost + 匹配 provider/model/currency/time window，`abs(vega − invoice) / invoice × 100 < 5%`，归 T50）。
- 「一周 dogfood」= 七个**独立 dated** dogfood 日/构建，逐日记录 task/result/failure/perf/UX；不足七天不得宣称完成。
- 报告必有 evidence cutoff；列出已合并 squash hash 的 PR 台账；分支 commit 与自身 PR/squash 标 `PENDING`；**FORBIDDEN** 引用未来 hash、把 skipped/mock/未执行写成 PASS、重复计 doctest、接受过滤后零测试的命令当全量。

**owner**：T49（s8-report + README 真值化）；可自动复核部分 T50。

## 10. OPEN(OWNER: human) 裁决项

| # | 事项 | 状态 | 裁决前行为 | 裁决材料 |
|---|---|---|---|---|
| 1 | P8 阈值单位：`<100,000,000` bytes（decimal MB）vs `<104,857,600` bytes（100 MiB） | **OPEN(OWNER: human)** | 按卡面字面权威 decimal MB 执行（候选 A）；T43 冻结 schema 前可改判 | §3.1 利弊对照 |

裁决后动作：本表移入 §14 勘误记录；ui-spec §5 P8 行与 phase1-plan E4（如引用单位）同步勘误；T43 schema 写死，测后永不换。

其余卡面 Stop 条目已按卡面原文冻结为保守默认、无需人类再裁：next-frame ≠ 物理 present（§2）、crash-after-effect 非 exactly-once（§6）、60Hz 只证 margin + `hardware pending`（§7）。

## 11. P0 审计与 owner 映射

（待扩：全量 49 项 P0 审计表 / P1-P8 owner 映射表 / 六表 migration 裁决。）

## 12. T41 报告后同步勘误清单

- （待扩：列出本文依赖 T40/T41 合并后事实的全部引用点。）

## 13. 验收自查（T42 验收条目对照）

- （待扩：八要素对照、内链可解析、依赖/六表扫描无变化。）

## 14. 勘误记录

- （空：待 OPEN 项裁决后记入。）

## 变更记录

- v1.0 (2026-08-31) 骨架：章节与每契约一行结论占位。
- v1.0 (2026-08-31) 扩充：状态词汇与 C1-C8 全量契约定义（P8 单位保持 OPEN(OWNER: human)）。
